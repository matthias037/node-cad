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
    assert!(app.agent_chat.items.iter().any(|i| matches!(i, Item::Tool { status: Some(ToolStatus::Failed), .. })), "{:?}", app.agent_chat.items.len());
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
