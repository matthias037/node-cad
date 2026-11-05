use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{Value, json};

use super::super::{Item, State, poll, send};
use super::*;

/// The page's `fetch` as a test holds it: the requests sent, each waiting for its answer.
#[derive(Clone, Default)]
struct Net {
    sent: Rc<RefCell<Vec<(HttpRequest, Option<Box<dyn FnOnce(HttpAnswer)>>)>>>,
}

impl Net {
    fn fetch(&self) -> Fetch {
        let sent = self.sent.clone();
        Rc::new(move |req, done| sent.borrow_mut().push((req, Some(done))))
    }
    fn count(&self) -> usize {
        self.sent.borrow().len()
    }
    /// The body of request `i`.
    fn body(&self, i: usize) -> Value {
        serde_json::from_slice(self.sent.borrow()[i].0.body.as_deref().unwrap_or_default()).unwrap()
    }
    /// Answer request `i`.
    fn answer(&self, i: usize, status: u16, body: Value) {
        let done = self.sent.borrow_mut()[i].1.take().expect("answered once");
        done(Ok((status, body.to_string().into_bytes())));
    }
}

fn says(text: &str) -> Value {
    json!({"choices": [{"message": {"role": "assistant", "content": text}}]})
}

fn calls(id: &str, name: &str, args: Value) -> Value {
    json!({"choices": [{"message": {"role": "assistant", "content": "", "tool_calls": [
        {"id": id, "type": "function", "function": {"name": name, "arguments": args.to_string()}}
    ]}}]})
}

/// An app whose agent is the browser's NØDE IA, ready, with `net` as its `fetch`.
fn app_with_web(net: &Net) -> (SolveApp, egui::Context) {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    let (backend, core) = WebBackend::start(Config {
        name: "NØDE IA".into(),
        base_url: "https://node-os.fr/api/cad/ia/v1".into(),
        model: "mistral-large-latest".into(),
        token: "jeton".into(),
        fetch: net.fetch(),
        wake: Rc::new(|| {}),
    });
    app.agent_chat.backend = Some(Box::new(backend));
    app.agent_chat.web = Some(core);
    let ctx = crate::fonts::test_context();
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Ready);
    (app, ctx)
}

/// A message goes to the relay with the account's token, the system note and the tools; a
/// plain answer ends the turn.
#[test]
fn a_message_goes_to_the_relay_and_a_plain_answer_ends_the_turn() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    send(&mut app, &ctx, "bonjour".into());
    assert_eq!(app.agent_chat.state(), State::Busy);
    assert_eq!(net.count(), 1);
    let req = net.sent.borrow()[0].0.clone();
    assert_eq!(req.url, "https://node-os.fr/api/cad/ia/v1/chat/completions");
    assert!(req.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer jeton"));
    let body = net.body(0);
    assert_eq!(body["model"], "mistral-large-latest");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"].as_array().unwrap().last().unwrap()["content"], "bonjour");
    let names: Vec<&str> = body["tools"].as_array().unwrap().iter().filter_map(|t| t.pointer("/function/name").and_then(Value::as_str)).collect();
    assert!(names.contains(&"batch") && names.contains(&"measure"), "the MCP tools are offered: {names:?}");
    net.answer(0, 200, says("Bonjour !"));
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Ready);
    assert!(app.agent_chat.items.iter().any(|i| matches!(i, Item::Agent { text, .. } if text == "Bonjour !")));
    assert_eq!(net.count(), 1, "nothing more was asked");
}

/// A conversation marked confidential tells the relay so with its next requests; unmarked,
/// nothing is said.
#[test]
fn a_confidential_conversation_tells_the_relay() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    let marked = |net: &Net| -> Vec<bool> {
        let sent = net.sent.borrow();
        let posts = sent.iter().filter(|(r, _)| r.method == "POST");
        posts.map(|(r, _)| r.headers.iter().any(|(k, v)| k == super::super::node_ia::CONFIDENTIAL_HEADER && v == "1")).collect()
    };
    send(&mut app, &ctx, "bonjour".into());
    net.answer(0, 200, says("Bonjour !"));
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Ready);
    app.agent_chat.backend.as_mut().unwrap().set_confidential(true);
    send(&mut app, &ctx, "la bride du client".into());
    assert_eq!(marked(&net), [false, true]);
}

/// NØDE IA says what it does while it works: the request names itself, the page reads its
/// progress through `fetch` while it waits (one read at a time, nothing held between frames),
/// each new line shows as a dimmed thought before the answer, and the reads stop with the answer.
#[test]
fn what_node_ia_does_shows_while_the_page_waits_for_its_answer() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    app.agent_chat.web.as_ref().unwrap().borrow_mut().progress_every = std::time::Duration::ZERO;
    send(&mut app, &ctx, "une presse".into());
    let id = net.sent.borrow()[0].0.headers.iter().find(|(k, _)| k == super::super::node_ia::REQUEST_HEADER).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(id.len(), 32);
    poll(&mut app, &ctx);
    assert_eq!(net.count(), 2, "a progress read went");
    let read = net.sent.borrow()[1].0.clone();
    assert_eq!((read.method, read.url), ("GET", format!("https://node-os.fr/api/cad/ia/v1/progres?requete={id}&depuis=0")));
    assert!(read.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer jeton"));
    poll(&mut app, &ctx);
    assert_eq!(net.count(), 2, "one read at a time");
    let lines = json!([{"n": 1, "t": 0, "genre": "plan", "texte": "Plan en cours…"}, {"n": 2, "t": 800, "genre": "reflexion", "texte": "Un bâti, un vérin."}]);
    net.answer(1, 200, json!({"lignes": lines, "fini": false}));
    poll(&mut app, &ctx);
    assert!(
        app.agent_chat.items.iter().any(|i| matches!(i, Item::Agent { thought: true, text } if text == "Plan en cours…\n« Un bâti, un vérin. »")),
        "{:?}",
        app.agent_chat.items
    );
    assert_eq!(app.agent_chat.state(), State::Busy);
    assert_eq!(net.count(), 3, "the next read");
    assert!(net.sent.borrow()[2].0.url.ends_with("&depuis=2"));
    // The answer comes: the reads stop, and a late progress answer changes nothing.
    net.answer(0, 200, says("La presse est faite."));
    poll(&mut app, &ctx);
    net.answer(2, 200, json!({"lignes": [{"n": 3, "t": 900, "genre": "controle", "texte": "Trop tard."}], "fini": true}));
    poll(&mut app, &ctx);
    poll(&mut app, &ctx);
    assert_eq!(net.count(), 3, "no read after the answer");
    assert_eq!(app.agent_chat.state(), State::Ready);
    assert!(!format!("{:?}", app.agent_chat.items).contains("Trop tard."));
    assert!(app.agent_chat.items.iter().any(|i| matches!(i, Item::Agent { thought: false, text } if text == "La presse est faite.")));
}

/// A tool call runs in the page through the gate and changes the design; its result goes back
/// to the model with the next request, and the turn ends on the model's last answer.
#[test]
fn a_tool_call_runs_in_the_page_and_its_result_goes_back() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    send(&mut app, &ctx, "une platine".into());
    let batch = json!({"commands": [{"command": "solid.box", "params": {"length": 120, "width": 80, "height": 10, "body_name": "Platine"}}]});
    net.answer(0, 200, calls("c1", "batch", batch));
    poll(&mut app, &ctx);
    assert_eq!(app.session.world_state().bodies.iter().filter(|b| b.name == "Platine").count(), 1, "the batch ran in the page");
    assert_eq!(net.count(), 2, "the result went back at once");
    let body = net.body(1);
    let last = body["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["role"], "tool");
    assert_eq!(last["tool_call_id"], "c1");
    assert!(last["content"].as_str().unwrap().contains("\"ok\":true"), "{last}");
    assert!(
        app.agent_chat.items.iter().any(|i| matches!(i, Item::Tool { title, status: Some(ToolStatus::Completed), .. } if title.starts_with("batch"))),
        "the card says it is done"
    );
    net.answer(1, 200, says("La platine est faite."));
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Ready);
}

/// The gate holds in the page: a command the agent chat does not offer is refused, and the
/// model reads why.
#[test]
fn the_gate_refuses_in_the_page_too() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    send(&mut app, &ctx, "quitte".into());
    net.answer(0, 200, calls("c1", "execute", json!({"command": "app.quit", "params": {}})));
    poll(&mut app, &ctx);
    let body = net.body(1);
    let last = body["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["role"], "tool");
    assert!(
        app.agent_chat.items.iter().any(|i| matches!(i, Item::Tool { status: Some(ToolStatus::Failed), .. })),
        "{:?}",
        app.agent_chat.items.len()
    );
}

/// The relay's refusal (no subscription, allowance used) is shown as it wrote it, and the turn
/// ends; a late answer to a stopped message is dropped.
#[test]
fn a_refusal_is_shown_and_a_late_answer_dropped() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    send(&mut app, &ctx, "une bride".into());
    net.answer(0, 402, json!({"error": {"message": "Votre essai de NØDE IA est terminé."}}));
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Ready);
    let shown = format!("{:?}", app.agent_chat.items);
    assert!(shown.contains("Votre essai de NØDE IA est terminé."), "{shown}");
    // Stopped while the model thinks: its answer, when it comes, changes nothing.
    send(&mut app, &ctx, "une autre".into());
    super::super::cancel(&mut app);
    poll(&mut app, &ctx);
    net.answer(1, 200, calls("c9", "batch", json!({"commands": [{"command": "solid.box", "params": {"length": 1, "width": 1, "height": 1}}]})));
    poll(&mut app, &ctx);
    assert!(app.session.world_state().bodies.is_empty(), "nothing ran after the stop");
    assert_eq!(net.count(), 2);
}

/// Boxes, each given a colour: two steps a box. The last is the first again (same size, same
/// place), so the design check after the batch has twins to report.
fn boxes(n: usize) -> Value {
    let mut steps = Vec::new();
    for i in 0..n {
        let name = format!("Bloc {i}");
        let height = if i + 1 == n { 10 } else { 10 + i };
        steps.push(json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": height, "body_name": name}}));
        steps.push(json!({"command": "appearance.assign", "params": {"bodies": [name], "color": "#41464d"}}));
    }
    json!({"commands": steps, "verbose": false})
}

/// Timing fields (`ms`) differ from run to run.
fn without_timings(v: &Value) -> Value {
    match v {
        Value::Object(o) => o.iter().filter(|(k, _)| *k != "ms").map(|(k, v)| (k.clone(), without_timings(v))).collect(),
        Value::Array(a) => a.iter().map(without_timings).collect(),
        v => v.clone(),
    }
}

/// The tool result request `i` carried back to the model (its last message), timings left out.
fn tool_result(net: &Net, i: usize) -> Value {
    let body = net.body(i);
    let last = body["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["role"], "tool", "{last}");
    without_timings(&serde_json::from_str(last["content"].as_str().unwrap()).unwrap())
}

/// A step a frame (a body a frame when measuring).
fn a_step_a_frame(app: &SolveApp) {
    app.agent_chat.web.as_ref().unwrap().borrow_mut().budget_ms = 0.0;
}

/// What a tool says when all its requests run at once in the page, as before batches ran a few
/// steps a frame (on a design made by `before`): its result, the design's measure, and the
/// requests and changes the gate counted.
fn at_once(name: &str, args: &Value, before: &[Value]) -> (Value, Value, (u32, u32)) {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    for step in before {
        app.run(step["command"].as_str().unwrap(), step["params"].clone()).unwrap();
    }
    send(&mut app, &ctx, "d'un coup".into());
    let limit = app.preferences.agent_chat_turn_limit.clamp(1, 1000);
    let mut page = InPage { app: &mut app, ctx: Some(&ctx), limit, tape: None, begun: None };
    let (text, _) = core::tool_text(&solidcraft_mcp::call_tool(&mut page, name, args));
    let counted = app.agent_chat.turn.as_ref().map(|t| (t.requests, t.changes)).unwrap();
    let design = app.session.execute("inspect.measure", &json!({})).unwrap();
    (without_timings(&serde_json::from_str(&text).unwrap()), without_timings(&design), counted)
}

/// A big batch runs a few steps a frame: the design grows from frame to frame and the card
/// counts the steps, then the bodies the design check measures; the model is asked again once
/// all is done, and it reads what the batch at once says (the check's twins included). The
/// gate counted each request once.
#[test]
fn a_big_batch_runs_a_few_steps_a_frame_and_answers_as_at_once() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    a_step_a_frame(&app);
    send(&mut app, &ctx, "des blocs".into());
    let args = boxes(12);
    net.answer(0, 200, calls("c1", "batch", args.clone()));
    let (mut bodies, mut counted) = (Vec::new(), Vec::new());
    while net.count() < 2 && bodies.len() < 1000 {
        poll(&mut app, &ctx);
        bodies.push(app.session.world_state().bodies.len());
        if let Some(Item::Tool { progress: Some(p), status: Some(ToolStatus::InProgress), .. }) = app.agent_chat.items.last() {
            counted.push(p.clone());
        }
    }
    // 24 steps, then 12 bodies measured, a frame each.
    assert!(bodies.len() >= 24 + 12, "{} frames", bodies.len());
    assert!(bodies.windows(2).all(|w| w[1] <= w[0] + 1), "more than a box in a frame: {bodies:?}");
    assert_eq!(bodies.last(), Some(&12));
    assert!(counted.iter().any(|(done, total, text)| *total == 24 && (1..24).contains(done) && text.contains("/ 24")), "{counted:?}");
    assert!(counted.iter().any(|(done, total, _)| *total == 12 && (1..12).contains(done)), "the check's measure is counted: {counted:?}");
    let (once, design, gate) = at_once("batch", &args, &[]);
    let read = tool_result(&net, 1);
    assert_eq!(read, once);
    assert_eq!(read["check"]["duplicates"].as_array().map(Vec::len), Some(2), "{read}");
    assert_eq!(without_timings(&app.session.execute("inspect.measure", &json!({})).unwrap()), design);
    assert_eq!(app.agent_chat.turn.as_ref().map(|t| (t.requests, t.changes)), Some(gate), "the gate counts as at once");
}

/// A step that fails a few frames in stops the batch and takes it back, as at once.
#[test]
fn a_batch_that_fails_over_frames_answers_as_at_once() {
    let mut args = boxes(6);
    args["commands"].as_array_mut().unwrap().insert(5, json!({"command": "solid.extrude", "params": {"distance": "bogus +"}}));
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    a_step_a_frame(&app);
    send(&mut app, &ctx, "des blocs".into());
    net.answer(0, 200, calls("c1", "batch", args.clone()));
    let mut frames = 0;
    while net.count() < 2 && frames < 1000 {
        poll(&mut app, &ctx);
        frames += 1;
    }
    assert!(frames > 5, "{frames} frames");
    let read = tool_result(&net, 1);
    assert_eq!(read, at_once("batch", &args, &[]).0);
    assert_eq!((&read["failed"]["index"], &read["rolled_back"]), (&json!(5), &json!(true)), "{read}");
    assert!(app.session.world_state().bodies.is_empty());
}

/// The calls of one answer run in order: a measure after a batch runs once the batch's last
/// step has, and finds every body it made.
#[test]
fn a_call_after_a_batch_waits_for_its_last_step() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    a_step_a_frame(&app);
    send(&mut app, &ctx, "des blocs, puis la mesure".into());
    let call = |id: &str, name: &str, args: Value| json!({"id": id, "type": "function", "function": {"name": name, "arguments": args.to_string()}});
    let both = json!({"choices": [{"message": {"role": "assistant", "content": "", "tool_calls": [call("c1", "batch", boxes(4)), call("c2", "measure", json!({}))]}}]});
    net.answer(0, 200, both);
    let mut frames = 0;
    while net.count() < 2 && frames < 1000 {
        poll(&mut app, &ctx);
        frames += 1;
    }
    let body = net.body(1);
    let messages = body["messages"].as_array().unwrap();
    let (batch, measure) = (&messages[messages.len() - 2], &messages[messages.len() - 1]);
    assert_eq!((&batch["tool_call_id"], &measure["tool_call_id"]), (&json!("c1"), &json!("c2")));
    let measured: Value = serde_json::from_str(measure["content"].as_str().unwrap()).unwrap();
    assert_eq!(measured["body_count"], 4, "{measured}");
}

/// Stop halfway through a batch: it goes back to where it started, as a failed batch does, and
/// the message ends; the model is not asked again, and reads next time that it was stopped.
#[test]
fn stop_halfway_takes_the_batch_back() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    a_step_a_frame(&app);
    send(&mut app, &ctx, "des blocs".into());
    net.answer(0, 200, calls("c1", "batch", boxes(12)));
    for _ in 0..8 {
        poll(&mut app, &ctx);
    }
    let made = app.session.world_state().bodies.len();
    assert!((1..12).contains(&made), "{made} bodies");
    super::super::cancel(&mut app);
    assert!(app.session.world_state().bodies.is_empty(), "the batch is not taken back");
    for _ in 0..3 {
        poll(&mut app, &ctx);
    }
    assert_eq!(app.agent_chat.state(), State::Ready);
    assert_eq!(net.count(), 1, "the model was asked again");
    let core = app.agent_chat.web.clone().unwrap();
    let last = core.borrow().history.last().cloned().unwrap();
    let said = last["content"].as_str().unwrap();
    assert!(said.contains("cancelled by the user") && said.contains("\"rolled_back\":true"), "{said}");
    assert!(app.agent_chat.items.iter().any(|i| matches!(i, Item::Tool { status: Some(ToolStatus::Failed), .. })));
}

/// The person changes the design between two frames of a batch (an undo): the batch stops
/// before its next step and goes back to where it started, and the model reads why.
#[test]
fn an_edit_between_two_frames_stops_the_batch() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    a_step_a_frame(&app);
    send(&mut app, &ctx, "des blocs".into());
    net.answer(0, 200, calls("c1", "batch", boxes(12)));
    for _ in 0..6 {
        poll(&mut app, &ctx);
    }
    app.run("edit.undo", json!({})).unwrap();
    let mut frames = 0;
    while net.count() < 2 && frames < 1000 {
        poll(&mut app, &ctx);
        frames += 1;
    }
    let read = tool_result(&net, 1);
    assert_eq!((&read["ok"], &read["rolled_back"]), (&json!(false), &json!(true)), "{read}");
    assert!(read["failed"]["error"].as_str().is_some_and(|e| e.contains("changed")), "{read}");
    assert!(app.session.world_state().bodies.is_empty());
}

/// A measure of many bodies runs a few bodies a frame and says what the measure at once says.
#[test]
fn a_measure_of_many_bodies_runs_a_few_a_frame() {
    let before: Vec<Value> = (0..6)
        .map(|i| json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10 + i, "body_name": format!("Bloc {i}")}}))
        .collect();
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    for step in &before {
        app.run(step["command"].as_str().unwrap(), step["params"].clone()).unwrap();
    }
    a_step_a_frame(&app);
    send(&mut app, &ctx, "mesure tout".into());
    net.answer(0, 200, calls("c1", "measure", json!({})));
    let mut frames = 0;
    while net.count() < 2 && frames < 1000 {
        poll(&mut app, &ctx);
        frames += 1;
    }
    assert!(frames >= 6, "{frames} frames");
    let read = tool_result(&net, 1);
    assert_eq!(read["body_count"], 6, "{read}");
    assert_eq!(read, at_once("measure", &json!({}), &before).0);
}

/// A page the browser draws seldom (in the background, or its window covered) gives its calls
/// more time a frame: a big batch goes on at speed where nobody watches it.
#[test]
fn a_page_drawn_seldom_gives_its_calls_more_time() {
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    a_step_a_frame(&app);
    send(&mut app, &ctx, "des blocs".into());
    net.answer(0, 200, calls("c1", "batch", boxes(12)));
    poll(&mut app, &ctx);
    poll(&mut app, &ctx);
    assert_eq!(app.session.world_state().bodies.len(), 1, "a step a frame");
    std::thread::sleep(std::time::Duration::from_millis(250));
    poll(&mut app, &ctx);
    let made = app.session.world_state().bodies.len();
    assert!(made > 2, "the frame after a long wait ran one step: {made} bodies");
}

/// A measurement, not a check: how long each frame of the page is held while NØDE IA's batch
/// `NODE_CAD_LOT` (a JSON array of `{command, params}`) runs, until its result goes back. With
/// `NODE_CAD_LOT_SORTIE`, the result the model reads and the bodies made are written there.
/// `NODE_CAD_LOT=lot.json cargo test -p solidcraft-ui-egui --profile ci --lib frozen_page -- --ignored --nocapture`
#[test]
#[ignore = "measurement: NODE_CAD_LOT=<lot.json>"]
fn frozen_page_while_a_big_batch_runs() {
    let Ok(path) = std::env::var("NODE_CAD_LOT") else { return };
    let lot: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let steps = lot.as_array().map_or(0, Vec::len);
    let net = Net::default();
    let (mut app, ctx) = app_with_web(&net);
    send(&mut app, &ctx, "une ligne".into());
    net.answer(0, 200, calls("c1", "batch", json!({"commands": lot, "verbose": false})));
    let start = std::time::Instant::now();
    let mut frames: Vec<f64> = Vec::new();
    while net.count() < 2 && frames.len() < 1_000_000 {
        let t = std::time::Instant::now();
        poll(&mut app, &ctx);
        frames.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let total = start.elapsed().as_secs_f64() * 1000.0;
    let mut longest = frames.clone();
    longest.sort_by(|a, b| b.total_cmp(a));
    let body = net.body(1);
    let result = body["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap_or_default().to_string();
    println!(
        "{steps} steps, {} bodies: {} frames, longest {:.0} ms (then {:?}), all {total:.0} ms",
        app.session.world_state().bodies.len(),
        frames.len(),
        longest[0],
        longest.iter().skip(1).take(4).map(|ms| ms.round()).collect::<Vec<_>>()
    );
    if let Ok(out) = std::env::var("NODE_CAD_LOT_SORTIE") {
        let measured = app.session.execute("inspect.measure", &json!({})).unwrap();
        let written = json!({"result": result, "bodies": measured["bodies"], "frames_ms": frames});
        std::fs::write(out, serde_json::to_string_pretty(&written).unwrap()).unwrap();
    }
}
