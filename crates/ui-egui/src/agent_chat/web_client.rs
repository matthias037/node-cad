//! NØDE IA in the browser: the tool loop of `api_client`, without a thread or an MCP process.
//! The page's `fetch` (the host's [`Fetch`]) asks the relay, and its answer comes back on the
//! window's thread; there, [`pump`] runs the tool calls the model asks for in the page itself,
//! through the same gate as the desktop's ([`super::judge`]) and with the same tools
//! (`solidcraft_mcp::call_tool`), then sends the next request. A batch runs a few steps a frame
//! ([`crate::control::Batch`]): the page goes on drawing, the person can stop it, and its card
//! counts its steps. Nothing here needs the browser, so the loop is tested on the desktop too.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use serde_json::{Value, json};
use solidcraft_engine::timing::Clock;

use super::acp::ModelInfo;
use super::agents::Protocol;
use super::api_core::{self as core, Call};
use super::backend::{ChatBackend, Event, Fetch, HttpAnswer, HttpRequest, PromptPart, ToolStatus};
use crate::SolveApp;
use crate::control::{Batch, Outcome};

/// How long one model request may take.
const REQUEST_TIMEOUT_S: u64 = 300;

/// The time a frame gives the tool calls (ms): a batch runs its steps within it, a few dozen at a
/// time, and the page draws and answers the person in between.
const FRAME_BUDGET_MS: f64 = 30.0;

/// A page the browser draws seldom (in the background, or its window covered) is not watched:
/// when this long went by since the calls' last frame (ms), a frame gives them
/// [`SELDOM_BUDGET_MS`], or a big batch would take minutes there (3 frames a second, measured).
const SELDOM_MS: f64 = 200.0;
const SELDOM_BUDGET_MS: f64 = 250.0;

/// This frame's time for the tool calls.
struct Frame {
    clock: Clock,
    budget_ms: f64,
}

impl Frame {
    fn spent(&self) -> bool {
        self.clock.ms() >= self.budget_ms
    }
}

/// A tool call answered without running, the message stopped.
const CANCELLED: &str = "cancelled by the user";

/// What the browser's NØDE IA starts with.
pub struct Config {
    pub name: String,
    /// The relay, as an OpenAI-compatible base URL (`node_ia::base_url`).
    pub base_url: String,
    pub model: String,
    /// The account's token (`node_ia`), sent as a bearer.
    pub token: String,
    pub fetch: Fetch,
    /// Wakes the window when an answer arrives.
    pub wake: Rc<dyn Fn()>,
}

/// The conversation and where its turn stands, shared by the panel's backend and [`pump`].
pub struct Core {
    cfg: Config,
    /// The tools as function tools, and their names.
    tools: Vec<Value>,
    tool_names: Vec<String>,
    /// The conversation without the system note (sent with each request).
    history: Vec<Value>,
    events: Vec<Event>,
    /// The request waiting for its answer (its number), if any.
    waiting: Option<u64>,
    /// Numbers the requests: a late answer to a cancelled one is dropped.
    serial: u64,
    /// The history before the request in flight: a cancelled request goes back to it.
    sound: usize,
    /// Model requests in this message.
    rounds: usize,
    /// The failures of this message, to tell a repeated one.
    failures: Vec<String>,
    /// Where `fetch` leaves an answer: kept apart from the rest, as a host may answer at once.
    inbox: Rc<RefCell<Option<(u64, HttpAnswer)>>>,
    /// The tool calls of the last answer still running, from one frame to the next.
    work: Option<Work>,
    /// The time a frame gives them ([`FRAME_BUDGET_MS`]; tests give less).
    budget_ms: f64,
    /// Since when the calls wait for the next frame.
    paused: Option<Clock>,
    /// What NØDE IA does while the request in flight runs, read from the relay
    /// ([`Core::read_progress`]), and how often ([`super::node_ia::PROGRESS_EVERY`]; tests read
    /// it at once).
    follow: Option<super::node_ia::Follow>,
    progress_every: std::time::Duration,
    /// The conversation is confidential: NØDE IA is told so with each request
    /// ([`super::node_ia::CONFIDENTIAL_HEADER`]).
    confidential: bool,
    /// Where `fetch` leaves the progress answers, each with the number of the request it follows
    /// (a late one, of a request answered meanwhile, is dropped).
    progress: Rc<RefCell<Vec<(u64, HttpAnswer)>>>,
}

/// The tool calls of an answer, run in order over as many frames as they take.
struct Work {
    /// The calls not started yet.
    calls: VecDeque<Call>,
    /// The call whose long request is running, and that request.
    running: Option<(Running, Long)>,
}

impl Work {
    fn new(calls: Vec<Call>) -> Work {
        Work { calls: calls.into_iter().take(32).collect(), running: None }
    }
}

/// A tool call whose tool put off a long request.
struct Running {
    call: Call,
    tool_id: String,
    tape: Tape,
}

/// A tool call's requests and their answers, in order, kept from one call of its tool to the
/// next: a long request ([`Long`]) is put off, and once it has its answer the tool is called
/// again and reads its earlier answers back. Nothing runs, nor passes the gate, twice; the
/// tool's own code is the desktop's.
#[derive(Default)]
struct Tape {
    answers: Vec<(String, Value, Result<Value, String>)>,
    /// The answers this call of the tool has read back.
    read: usize,
    /// The long request this call of the tool put off.
    put_off: Option<(String, Value)>,
}

impl Tape {
    /// The answer of the request put off, for the next call of the tool.
    fn answered(&mut self, answer: Result<Value, String>) {
        if let Some((method, params)) = self.put_off.take() {
            self.answers.push((method, params, answer));
        }
    }
}

/// A request that runs over several frames, the gate passed.
enum Long {
    /// A batch, a few steps a frame.
    Steps(Batch),
    /// A measure of many bodies, a few bodies a frame.
    Measure(Measuring),
}

impl Long {
    /// A long request, started (a refusal is its answer at once). `None`: it runs now, as any.
    fn begin(app: &mut SolveApp, method: &str, params: &Value) -> Option<Result<Long, Outcome>> {
        match method {
            // `control::handle` runs a request that names folders inside them; the page's never do.
            "engine.batch" if params.get("allow_paths").is_none() => {
                // What `control::handle` does before it runs one.
                crate::render_workspace::sync_backend(app);
                Some(Batch::start(app, params).map(Long::Steps))
            }
            "engine.execute" => Measuring::start(app, params).map(|m| Ok(Long::Measure(m))),
            _ => None,
        }
    }
}

/// `inspect.measure` of many bodies, one body at a time, and their measures merged as the
/// command merges them ([`solidcraft_engine::cmd::measure_reply`]): a big design is measured
/// on one thread in the browser, for seconds.
struct Measuring {
    /// The command's parameters.
    params: Value,
    /// The bodies left to measure, in the design's order, and the measures so far.
    left: VecDeque<String>,
    done: Vec<Value>,
    total: usize,
    /// The design they are measured in: anything changing it stops the measure.
    doc: std::sync::Arc<solidcraft_engine::doc::Document>,
}

impl Measuring {
    /// The measure an `engine.execute` asks for, when it measures bodies (not `items`), at least
    /// two of them, each named once in the design. `None`: it runs at once.
    fn start(app: &SolveApp, execute: &Value) -> Option<Measuring> {
        if execute.get("command").and_then(Value::as_str) != Some("inspect.measure") {
            return None;
        }
        let params = execute.get("params").cloned().filter(|v| !v.is_null()).unwrap_or_else(|| json!({}));
        if params.get("items").is_some() || !params.is_object() {
            return None;
        }
        // The bodies as the command picks them (a list it refuses runs at once, to say why).
        let Ok(want) = solidcraft_engine::params::string_list("inspect.measure", &params, "bodies") else { return None };
        let Ok(st) = app.session.world_state() else { return None };
        let mut seen = std::collections::HashSet::new();
        if !st.bodies.iter().all(|b| seen.insert(b.name.as_str())) {
            return None;
        }
        let left: VecDeque<String> = st.bodies.iter().filter(|b| want.is_empty() || want.contains(&b.name)).map(|b| b.name.clone()).collect();
        if left.len() < 2 || (!want.is_empty() && left.len() != want.len()) {
            return None;
        }
        Some(Measuring { params, total: left.len(), left, done: Vec::new(), doc: app.session.doc.clone() })
    }

    /// Measure the next bodies, at least one, while the frame's time lasts: the command's reply
    /// once the last is measured, `None` while bodies are left.
    fn go_on(&mut self, app: &mut SolveApp, ctx: &egui::Context, frame: &Frame) -> Option<Result<Value, String>> {
        if !std::sync::Arc::ptr_eq(&app.session.doc, &self.doc) {
            return Some(Err("the design changed while it was being measured (an edit, an undo or another tab): measure it again".into()));
        }
        while let Some(name) = self.left.pop_front() {
            let mut one = self.params.clone();
            one["bodies"] = json!([name]);
            let (req, _reply) = crate::ControlRequest::new("engine.execute", json!({"command": "inspect.measure", "params": one}));
            match answer(crate::control::handle(app, ctx, &req)) {
                Ok(v) => match v["bodies"].as_array().and_then(|b| b.first()) {
                    Some(body) => self.done.push(body.clone()),
                    None => return Some(Err(format!("inspect.measure: `{name}` was not measured"))),
                },
                Err(e) => return Some(Err(e)),
            }
            if !self.left.is_empty() && frame.spent() {
                return None;
            }
        }
        let quick = solidcraft_engine::params::bool_(&self.params, "quick").unwrap_or(false);
        Some(Ok(solidcraft_engine::cmd::measure_reply(std::mem::take(&mut self.done), quick)))
    }
}

impl Core {
    fn new(cfg: Config) -> Core {
        let list = json!({"tools": solidcraft_mcp::tool_definitions()});
        let mut c = Core {
            cfg,
            tools: core::openai_tools(&list),
            tool_names: core::tool_names(&list),
            history: Vec::new(),
            events: Vec::new(),
            waiting: None,
            serial: 0,
            sound: 0,
            rounds: 0,
            failures: Vec::new(),
            inbox: Rc::default(),
            work: None,
            budget_ms: FRAME_BUDGET_MS,
            paused: None,
            follow: None,
            progress_every: super::node_ia::PROGRESS_EVERY,
            confidential: false,
            progress: Rc::default(),
        };
        c.ready();
        c
    }

    fn ready(&mut self) {
        self.events.push(Event::Ready { agent: format!("{} · {}", self.cfg.name, self.cfg.model), images: true });
        // Documents go through NØDE IA's OCR on the desktop; the browser sends pictures only.
        self.events.push(Event::Documents(false));
        let m = self.cfg.model.clone();
        self.events.push(Event::Models { current: m.clone(), available: vec![ModelInfo { id: m.clone(), name: m, description: String::new() }] });
    }

    fn prompt(&mut self, parts: &[PromptPart]) {
        if self.waiting.is_some() {
            return;
        }
        core::shorten_tool_results(&mut self.history, core::OLD_TOOL_TEXT);
        self.history.push(core::user_message(Protocol::OpenAi, parts));
        while self.history.len() > core::MAX_HISTORY {
            self.history.remove(0);
        }
        while self.history.first().is_some_and(|m| !core::starts_a_turn(m)) {
            self.history.remove(0);
        }
        self.rounds = 0;
        self.failures.clear();
        self.send();
    }

    /// Ask the model, with the conversation as it stands (every tool call answered).
    fn send(&mut self) {
        if self.rounds >= core::MAX_REQUESTS_PER_TURN {
            self.events.push(Event::Text { thought: false, text: format!("\n({})", crate::i18n::tr("ui.ai_request_limit")) });
            self.events.push(Event::TurnEnded { reason: "max_turn_requests".into() });
            return;
        }
        self.rounds += 1;
        // Within a message too, only the last results stay whole.
        let cut = core::keep_last_tool_results(&self.history, core::KEPT_TOOL_RESULTS);
        core::shorten_tool_results(&mut self.history[..cut], core::OLD_TOOL_TEXT);
        self.sound = self.history.len();
        let mut messages = Vec::with_capacity(self.history.len() + 1);
        messages.push(json!({"role": "system", "content": super::acp::SYSTEM_NOTE}));
        messages.extend(self.history.iter().cloned());
        let mut body = json!({"model": self.cfg.model, "messages": messages});
        if !self.tools.is_empty() {
            body["tools"] = Value::Array(self.tools.clone());
        }
        let mut headers = vec![("Content-Type".into(), "application/json".into()), ("Authorization".into(), format!("Bearer {}", self.cfg.token))];
        // NØDE IA says what it does while it works: the request names itself, and its progress
        // is read until the answer comes (`read_progress`).
        self.follow = None;
        if super::node_ia::is_node_ia(&self.cfg.base_url) {
            let id = super::node_ia::request_id();
            headers.push((super::node_ia::REQUEST_HEADER.into(), id.clone()));
            self.follow = Some(super::node_ia::Follow::new(id, self.progress_every));
            // A confidential conversation stays with Mistral, in Europe.
            if self.confidential {
                headers.push((super::node_ia::CONFIDENTIAL_HEADER.into(), "1".into()));
            }
        }
        let request = HttpRequest {
            method: "POST",
            url: format!("{}/chat/completions", self.cfg.base_url),
            headers,
            body: Some(body.to_string().into_bytes()),
            timeout_s: REQUEST_TIMEOUT_S,
        };
        self.serial += 1;
        let serial = self.serial;
        self.waiting = Some(serial);
        let (inbox, wake) = (self.inbox.clone(), self.cfg.wake.clone());
        (self.cfg.fetch)(
            request,
            Box::new(move |answer| {
                *inbox.borrow_mut() = Some((serial, answer));
                wake();
            }),
        );
    }

    /// The answer that arrived for the request in flight: the tool calls to run, if any (the
    /// turn ends otherwise).
    fn answered(&mut self) -> Option<Vec<Call>> {
        let (serial, answer) = self.inbox.borrow_mut().take()?;
        if self.waiting != Some(serial) {
            return None;
        }
        self.waiting = None;
        // The answer is here: its progress is no longer read.
        self.follow = None;
        let fail = |c: &mut Core, e: String| {
            c.events.push(Event::Error(e));
            c.events.push(Event::TurnEnded { reason: "error".into() });
            None
        };
        let (status, bytes) = match answer {
            Ok(a) => a,
            Err(e) => return fail(self, crate::i18n::format("ui.node_ia_unreachable", &[("error", &e)])),
        };
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if status != 200 {
            // The relay writes its refusals for the person (subscription, trial, allowance).
            let message = v.pointer("/error/message").and_then(Value::as_str).filter(|m| !m.is_empty()).map(str::to_string);
            return fail(self, message.unwrap_or_else(|| core::http_error(status, &v)));
        }
        let reply = core::reply(Protocol::OpenAi, &v);
        for (thought, text) in reply.text {
            self.events.push(Event::Text { thought, text });
        }
        self.history.push(reply.assistant);
        if reply.calls.is_empty() {
            self.events.push(Event::TurnEnded { reason: String::new() });
            return None;
        }
        Some(reply.calls)
    }

    /// A tool call's result, as the card shows it and as the model reads it.
    fn result(&mut self, call: &Call, tool_id: String, text: String, failed: bool) {
        // The same failure again: say so, so the model changes its approach instead of looping.
        let text = if failed && self.failures.contains(&text) {
            format!("{text}\n(The same failure as before: change your approach, or ask the person.)")
        } else {
            if failed {
                self.failures.push(text.clone());
            }
            text
        };
        self.events.push(Event::Tool {
            id: tool_id,
            title: None,
            kind: None,
            status: Some(if failed { ToolStatus::Failed } else { ToolStatus::Completed }),
            input: None,
            output: Some(text.chars().take(2000).collect()),
        });
        self.history.push(json!({"role": "tool", "tool_call_id": call.id, "content": text}));
    }

    fn cancel(&mut self) {
        self.follow = None;
        if self.waiting.take().is_some() {
            self.history.truncate(self.sound);
            self.events.push(Event::TurnEnded { reason: "cancelled".into() });
        }
    }

    /// What NØDE IA does while the request in flight runs: the answer of the last read, its new
    /// lines as dimmed thoughts, and the next read once it is due, through the page's `fetch`
    /// (the window draws on meanwhile). How long until the next read, when one is to come: a
    /// read in flight wakes the window itself when its answer arrives.
    fn read_progress(&mut self) -> Option<std::time::Duration> {
        let answers = std::mem::take(&mut *self.progress.borrow_mut());
        let follow = self.follow.as_mut()?;
        let waiting = self.waiting?;
        for (_, answer) in answers.iter().filter(|(serial, _)| *serial == waiting) {
            for text in follow.answered(answer) {
                self.events.push(Event::Text { thought: true, text });
            }
        }
        if follow.due() {
            let request = follow.ask(&self.cfg.base_url, Some(&self.cfg.token));
            let (inbox, wake) = (self.progress.clone(), self.cfg.wake.clone());
            (self.cfg.fetch)(
                request,
                Box::new(move |answer| {
                    inbox.borrow_mut().push((waiting, answer));
                    wake();
                }),
            );
        }
        follow.wait()
    }
}

/// The panel's side of the browser's NØDE IA.
pub struct WebBackend {
    core: Rc<RefCell<Core>>,
}

impl WebBackend {
    /// The backend for the panel, and the conversation [`pump`] drives.
    pub fn start(cfg: Config) -> (WebBackend, Rc<RefCell<Core>>) {
        let core = Rc::new(RefCell::new(Core::new(cfg)));
        (WebBackend { core: core.clone() }, core)
    }
}

impl ChatBackend for WebBackend {
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.core.borrow_mut().events)
    }
    fn prompt(&mut self, parts: &[PromptPart]) {
        self.core.borrow_mut().prompt(parts);
    }
    fn cancel(&mut self) {
        self.core.borrow_mut().cancel();
    }
    fn new_session(&mut self) {
        let mut c = self.core.borrow_mut();
        c.cancel();
        // (Stop has answered the calls that were running: `stop`.)
        c.work = None;
        c.paused = None;
        c.history.clear();
        c.ready();
    }
    fn answer_permission(&mut self, _key: u64, _option: Option<&str>) {}
    fn authenticate(&mut self, _method: &str) {}
    fn set_model(&mut self, model: &str) {
        self.core.borrow_mut().cfg.model = model.to_string();
    }
    fn set_confidential(&mut self, on: bool) {
        self.core.borrow_mut().confidential = on;
    }
    fn shutdown(&mut self, _wait: bool) {
        self.core.borrow_mut().cancel();
    }
}

/// The page itself, as the tools' backend: each request passes the gate, then runs here, now.
/// With a [`Tape`], the tool's long requests are put off ([`Long`]) and its earlier requests
/// answered from the tape.
struct InPage<'a> {
    app: &'a mut SolveApp,
    /// `None`: the message was stopped, and nothing more runs.
    ctx: Option<&'a egui::Context>,
    limit: u32,
    tape: Option<&'a mut Tape>,
    /// The long request put off, started.
    begun: Option<Long>,
}

/// Why a tool's requests after one put off are not run (no one reads it: the tool is called
/// again).
const PUT_OFF: &str = "put off: a long request runs over the next frames";

impl InPage<'_> {
    fn gate(&mut self, method: &str, params: &Value) -> Result<(), String> {
        match super::judge(self.app, method, params, self.limit) {
            super::Judged::Run => Ok(()),
            super::Judged::Ask { title, .. } => Err(format!(
                "{title}: this needs the person's approval, which NØDE IA does not ask for in the browser yet. Tell the person what you \
                 wanted to do."
            )),
            super::Judged::Refuse(e) => Err(e),
        }
    }
}

/// What the control channel answered, as the tools' backend returns it.
fn answer(outcome: Outcome) -> Result<Value, String> {
    match outcome {
        Outcome::Done(v) if v.get("ok").and_then(Value::as_bool) == Some(true) => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
        Outcome::Done(v) => Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string()),
        Outcome::Screenshot { .. } => Err("a picture of the window is not available in the browser".into()),
    }
}

impl solidcraft_mcp::Backend for InPage<'_> {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if let Some(tape) = self.tape.as_deref_mut() {
            if tape.put_off.is_some() {
                return Err(PUT_OFF.into());
            }
            if let Some((asked, with, kept)) = tape.answers.get(tape.read) {
                // The tool asks again what it asked before: the same answer, nothing run again.
                let again = if asked == method && *with == params { kept.clone() } else { Err("asked otherwise than before: not run".into()) };
                tape.read += 1;
                return again;
            }
        }
        let Some(ctx) = self.ctx else { return Err(CANCELLED.into()) };
        let got = match self.gate(method, &params) {
            Err(e) => Err(e),
            Ok(()) => match self.tape.is_some().then(|| Long::begin(self.app, method, &params)).flatten() {
                Some(Ok(long)) => {
                    self.begun = Some(long);
                    if let Some(tape) = self.tape.as_deref_mut() {
                        tape.put_off = Some((method.to_string(), params));
                    }
                    return Err(PUT_OFF.into());
                }
                Some(Err(refused)) => answer(refused),
                None => answer(crate::control::handle(self.app, ctx, &crate::ControlRequest::new(method, params.clone()).0)),
            },
        };
        if let Some(tape) = self.tape.as_deref_mut() {
            tape.answers.push((method.to_string(), params, got.clone()));
            tape.read += 1;
        }
        got
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn describe(&self) -> String {
        "connected to the NØDE CAD app in the browser (the user watches the viewport)".into()
    }
    fn in_process(&self) -> bool {
        true
    }
}

/// Run the tool calls of the answer that arrived, in order, a frame's time at a time
/// ([`FRAME_BUDGET_MS`]), then ask the model again. Called each frame.
pub fn pump(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(core) = app.agent_chat.web.clone() else { return };
    // While the model works, what it does (NØDE IA), read about every second.
    let next_read = core.borrow_mut().read_progress();
    if let Some(wait) = next_read {
        ctx.request_repaint_after(wait);
    }
    let kept = core.borrow_mut().work.take();
    let Some(mut work) = kept.or_else(|| core.borrow_mut().answered().map(Work::new)) else { return };
    let (budget_ms, paused) = {
        let mut c = core.borrow_mut();
        (c.budget_ms, c.paused.take())
    };
    let seldom = paused.is_some_and(|p| p.ms() > SELDOM_MS);
    let frame = Frame { clock: Clock::start(), budget_ms: if seldom { budget_ms.max(SELDOM_BUDGET_MS) } else { budget_ms } };
    let limit = app.preferences.agent_chat_turn_limit.clamp(1, 1000);
    let names = core.borrow().tool_names.clone();
    // A call, or part of a long one, each frame; more while its time lasts.
    let mut first = true;
    while work.running.is_some() || !work.calls.is_empty() {
        if !first && frame.spent() {
            let mut c = core.borrow_mut();
            c.work = Some(work);
            c.paused = Some(Clock::start());
            ctx.request_repaint();
            return;
        }
        first = false;
        work.running = match work.running.take() {
            Some((running, long)) => go_on(app, ctx, &core, running, long, &frame, limit),
            None => work.calls.pop_front().and_then(|call| start(app, ctx, &core, call, limit, &names)),
        };
    }
    // Every call is answered: the next request, unless the person stopped the message.
    if app.agent_chat.turn.is_some() {
        core.borrow_mut().send();
    } else {
        core.borrow_mut().events.push(Event::TurnEnded { reason: "cancelled".into() });
    }
    ctx.request_repaint();
}

/// A tool call's card, running: its id.
fn card(core: &Rc<RefCell<Core>>, call: &Call) -> String {
    let args = call.arguments.clone().unwrap_or_else(|_| json!({}));
    let tool_id = format!("{}-{}", call.id, core.borrow().serial);
    core.borrow_mut().events.push(Event::Tool {
        id: tool_id.clone(),
        title: Some(core::tool_title(&call.name, &args)),
        kind: Some(String::new()),
        status: Some(ToolStatus::InProgress),
        input: call.arguments.as_ref().ok().filter(|a| a.is_object()).cloned(),
        output: None,
    });
    tool_id
}

/// Start a tool call: answered now, or running a long request over the next frames (`Some`).
fn start(app: &mut SolveApp, ctx: &egui::Context, core: &Rc<RefCell<Core>>, call: Call, limit: u32, names: &[String]) -> Option<(Running, Long)> {
    let tool_id = card(core, &call);
    let args = call.arguments.clone().unwrap_or_else(|_| json!({}));
    // Stopped meanwhile: the rest is answered without running.
    let (text, failed) = if app.agent_chat.turn.is_none() {
        (CANCELLED.to_string(), true)
    } else if let Err(e) = &call.arguments {
        // A call whose arguments do not parse is answered, never run with `{}`.
        (format!("Not run: {e}"), true)
    } else if names.iter().any(|n| n == &call.name) {
        return call_tool(app, Some(ctx), core, Running { call, tool_id, tape: Tape::default() }, limit);
    } else {
        // A model that calls an engine command as a tool: run it through `execute`, and say so.
        match core::as_execute(names, &call.name, &args) {
            Some((execute, a)) => {
                let mut page = InPage { app, ctx: Some(ctx), limit, tape: None, begun: None };
                let (text, failed) = core::tool_text(&solidcraft_mcp::call_tool(&mut page, &execute, &a));
                (format!("{text}\n(`{}` is not a tool: this ran as execute {{command, params}}.)", call.name), failed)
            }
            None => (format!("unknown tool `{}`", call.name), true),
        }
    };
    core.borrow_mut().result(&call, tool_id, text, failed);
    None
}

/// Call the tool (again, once its long request has its answer): its result for the model, or
/// the next long request it put off (`Some`). With no `ctx` the message was stopped: nothing
/// more runs, the design's check included.
fn call_tool(app: &mut SolveApp, ctx: Option<&egui::Context>, core: &Rc<RefCell<Core>>, mut running: Running, limit: u32) -> Option<(Running, Long)> {
    let args = running.call.arguments.clone().unwrap_or_else(|_| json!({}));
    running.tape.read = 0;
    let mut page = InPage { app, ctx, limit, tape: Some(&mut running.tape), begun: None };
    let r = solidcraft_mcp::call_tool(&mut page, &running.call.name, &args);
    if let Some(long) = page.begun.take() {
        return Some((running, long));
    }
    let (text, failed) = core::tool_text(&r);
    core.borrow_mut().result(&running.call, running.tool_id, text, failed);
    None
}

/// Go on with a tool call's long request while the frame's time lasts; once it has its answer,
/// the tool is called again. `Some` while the call is not answered.
fn go_on(
    app: &mut SolveApp,
    ctx: &egui::Context,
    core: &Rc<RefCell<Core>>,
    mut running: Running,
    long: Long,
    frame: &Frame,
    limit: u32,
) -> Option<(Running, Long)> {
    let got = match long {
        Long::Steps(mut batch) => {
            let reply = if app.agent_chat.turn.is_none() {
                // The message ended meanwhile: the batch stops as Stop stops it.
                batch.stop(app, CANCELLED)
            } else {
                match batch.run(app, || !frame.spent()) {
                    Some(reply) => reply,
                    None => {
                        let (done, total) = batch.progress();
                        progress(core, &running, done, total, "ui.ai_tool_progress");
                        return Some((running, Long::Steps(batch)));
                    }
                }
            };
            answer(reply)
        }
        Long::Measure(mut m) => {
            if app.agent_chat.turn.is_none() {
                Err(CANCELLED.into())
            } else {
                match m.go_on(app, ctx, frame) {
                    Some(reply) => reply,
                    None => {
                        progress(core, &running, m.done.len(), m.total, "ui.ai_tool_measured");
                        return Some((running, Long::Measure(m)));
                    }
                }
            }
        }
    };
    running.tape.answered(got);
    call_tool(app, Some(ctx), core, running, limit)
}

/// How far a call's long request is, on its card.
fn progress(core: &Rc<RefCell<Core>>, running: &Running, done: usize, total: usize, text: &str) {
    let text = crate::i18n::format(text, &[("done", &done.to_string()), ("total", &total.to_string())]);
    core.borrow_mut().events.push(Event::Progress { id: running.tool_id.clone(), done, total, text });
}

/// Stop the tool calls running in the page (Stop, New chat, the AI features turned off): a batch
/// halfway goes back to where it started, as a failed one does, the calls left are answered
/// without running, and the message ends.
pub(crate) fn stop(app: &mut SolveApp) {
    let Some(core) = app.agent_chat.web.clone() else { return };
    let Some(work) = core.borrow_mut().work.take() else { return };
    core.borrow_mut().paused = None;
    let limit = app.preferences.agent_chat_turn_limit.clamp(1, 1000);
    if let Some((mut running, long)) = work.running {
        let got = match long {
            Long::Steps(batch) => answer(batch.stop(app, CANCELLED)),
            Long::Measure(_) => Err(CANCELLED.into()),
        };
        running.tape.answered(got);
        call_tool(app, None, &core, running, limit);
    }
    for call in work.calls {
        let tool_id = card(&core, &call);
        core.borrow_mut().result(&call, tool_id, CANCELLED.into(), true);
    }
    core.borrow_mut().events.push(Event::TurnEnded { reason: "cancelled".into() });
}

#[cfg(test)]
mod tests;
