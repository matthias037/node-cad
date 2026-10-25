//! NØDE IA in the browser: the tool loop of `api_client`, without a thread or an MCP process.
//! The page's `fetch` (the host's [`Fetch`]) asks the relay, and its answer comes back on the
//! window's thread; there, [`pump`] runs the tool calls the model asks for in the page itself,
//! through the same gate as the desktop's ([`super::judge`]) and with the same tools
//! (`solidcraft_mcp::call_tool`), then sends the next request. Nothing here needs the
//! browser, so the loop is tested on the desktop too.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{Value, json};

use super::acp::ModelInfo;
use super::agents::Protocol;
use super::api_core::{self as core, Call};
use super::backend::{ChatBackend, Event, Fetch, HttpAnswer, HttpRequest, PromptPart, ToolStatus};
use crate::SolveApp;

/// How long one model request may take.
const REQUEST_TIMEOUT_S: u64 = 300;

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
        let request = HttpRequest {
            method: "POST",
            url: format!("{}/chat/completions", self.cfg.base_url),
            headers: vec![
                ("Content-Type".into(), "application/json".into()),
                ("Authorization".into(), format!("Bearer {}", self.cfg.token)),
            ],
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
            output: Some(text.chars().take(2000).collect()),
        });
        self.history.push(json!({"role": "tool", "tool_call_id": call.id, "content": text}));
    }

    fn cancel(&mut self) {
        if self.waiting.take().is_some() {
            self.history.truncate(self.sound);
            self.events.push(Event::TurnEnded { reason: "cancelled".into() });
        }
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
        c.history.clear();
        c.ready();
    }
    fn answer_permission(&mut self, _key: u64, _option: Option<&str>) {}
    fn authenticate(&mut self, _method: &str) {}
    fn set_model(&mut self, model: &str) {
        self.core.borrow_mut().cfg.model = model.to_string();
    }
    fn shutdown(&mut self, _wait: bool) {
        self.core.borrow_mut().cancel();
    }
}

/// The page itself, as the tools' backend: each request passes the gate, then runs here, now.
struct InPage<'a> {
    app: &'a mut SolveApp,
    ctx: &'a egui::Context,
    limit: u32,
}

impl solidcraft_mcp::Backend for InPage<'_> {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        match super::judge(self.app, method, &params, self.limit) {
            super::Judged::Run => {}
            super::Judged::Ask { title, .. } => {
                return Err(format!(
                    "{title}: this needs the person's approval, which NØDE IA does not ask for in the browser yet. Tell the person what you \
                     wanted to do."
                ));
            }
            super::Judged::Refuse(e) => return Err(e),
        }
        let (req, _reply) = crate::ControlRequest::new(method, params);
        match crate::control::handle(self.app, self.ctx, &req) {
            crate::control::Outcome::Done(v) if v.get("ok").and_then(Value::as_bool) == Some(true) => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
            crate::control::Outcome::Done(v) => Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string()),
            crate::control::Outcome::Screenshot { .. } => Err("a picture of the window is not available in the browser".into()),
        }
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

/// Run the tool calls of the answer that arrived, then ask the model again. Called each frame.
pub fn pump(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(core) = app.agent_chat.web.clone() else { return };
    let Some(calls) = core.borrow_mut().answered() else { return };
    let limit = app.preferences.agent_chat_turn_limit.clamp(1, 1000);
    let names = core.borrow().tool_names.clone();
    for call in calls.iter().take(32) {
        let args = call.arguments.clone().unwrap_or_else(|_| json!({}));
        let tool_id = format!("{}-{}", call.id, core.borrow().serial);
        core.borrow_mut().events.push(Event::Tool {
            id: tool_id.clone(),
            title: Some(core::tool_title(&call.name, &args)),
            kind: Some(String::new()),
            status: Some(ToolStatus::InProgress),
            output: None,
        });
        // Stopped meanwhile: the rest is answered without running.
        let (text, failed) = if core.borrow().waiting.is_none() && app.agent_chat.turn.is_none() {
            ("cancelled by the user".to_string(), true)
        } else if let Err(e) = &call.arguments {
            // A call whose arguments do not parse is answered, never run with `{}`.
            (format!("Not run: {e}"), true)
        } else if names.iter().any(|n| n == &call.name) {
            core::tool_text(&solidcraft_mcp::call_tool(&mut InPage { app, ctx, limit }, &call.name, &args))
        } else {
            // A model that calls an engine command as a tool: run it through `execute`, and say so.
            match core::as_execute(&names, &call.name, &args) {
                Some((execute, a)) => {
                    let (text, failed) = core::tool_text(&solidcraft_mcp::call_tool(&mut InPage { app, ctx, limit }, &execute, &a));
                    (format!("{text}\n(`{}` is not a tool: this ran as execute {{command, params}}.)", call.name), failed)
                }
                None => (format!("unknown tool `{}`", call.name), true),
            }
        };
        core.borrow_mut().result(call, tool_id, text, failed);
    }
    // Every call is answered: the next request, unless the person stopped the message.
    if app.agent_chat.turn.is_some() {
        core.borrow_mut().send();
    } else {
        core.borrow_mut().events.push(Event::TurnEnded { reason: "cancelled".into() });
    }
    ctx.request_repaint();
}

#[cfg(test)]
mod tests;
