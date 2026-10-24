//! NØDE IA: the assistant of the official NØDE CAD, sold by subscription on node-os.fr. The app is
//! only a client: it signs in with a NØDE CAD account (an address and a six-digit code, no
//! password), keeps the account's token in the system keychain, and talks to node-os.fr's
//! OpenAI-compatible relay, which checks the subscription or trial and calls the model. The
//! assistant's instructions live there, not in this code. A build with the `agents-libres`
//! feature offers the other agents (installed ones, endpoints with their own key) instead.
use serde_json::{Value, json};

use super::agents::{Entry, Kind};

/// The agent id: also the keychain entry that holds the account's token.
pub const ID: &str = "node-ia";
pub const NAME: &str = "NØDE IA";
/// The model a conversation starts with; the relay offers its list (`/models`).
pub const DEFAULT_MODEL: &str = "mistral-large-latest";
const SITE: &str = "https://node-os.fr";

/// The site that sells NØDE IA: node-os.fr, or `NODE_CAD_IA_SITE` to try a local copy of it.
pub fn site() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(s) = std::env::var("NODE_CAD_IA_SITE").ok().filter(|s| s.starts_with("http")) {
        return s.trim_end_matches('/').to_string();
    }
    SITE.to_string()
}

/// The relay, as an OpenAI-compatible base URL.
pub fn base_url() -> String {
    format!("{}/api/cad/ia/v1", site())
}

/// Whether `base_url` is NØDE IA's relay (its OCR reads documents, like Mistral's).
pub fn is_node_ia(base_url: &str) -> bool {
    base_url.trim().trim_end_matches('/').ends_with("/api/cad/ia/v1")
}

/// The page that sells the subscription and manages it.
pub fn subscribe_page() -> String {
    format!("{}/cad#ia", site())
}

/// Signed in: an address is remembered (its token is in the keychain), or a token is given by
/// `NODE_CAD_IA_JETON` (a test bench: the keychain asks again after every rebuild).
pub fn signed_in(app: &crate::SolveApp) -> bool {
    !app.preferences.node_ia_email.is_empty() || env_token().is_some()
}

/// The token `NODE_CAD_IA_JETON` gives instead of the keychain's.
pub fn env_token() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(t) = std::env::var("NODE_CAD_IA_JETON").ok().filter(|t| !t.trim().is_empty()) {
        return Some(t.trim().to_string());
    }
    None
}

/// NØDE IA as the picker and the chat see it: the only agent of the official app.
pub fn entry(app: &crate::SolveApp) -> Entry {
    Entry {
        id: ID.to_string(),
        name: NAME.to_string(),
        kind: Kind::Api,
        argv: Vec::new(),
        version: None,
        login: "",
        found: false,
        base_url: base_url(),
        model: app.preferences.agent_models.get(ID).cloned().filter(|m| !m.is_empty()).unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        key: signed_in(app),
    }
}

/// What the account page of the relay says (`/api/cad/ia/compte`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub email: String,
    /// NØDE IA answers this account now.
    pub open: bool,
    /// `abonnement` or `essai` while open.
    pub plan: Option<String>,
    /// Why it is closed, in the site's words.
    pub reason: Option<String>,
    /// The end of the paid period or of the trial (`YYYY-MM-DD…`).
    pub until: Option<String>,
    /// Share of the month's allowance used, in percent.
    pub used: Option<u32>,
    /// A Stripe subscription exists (active, unpaid or ending): it is managed on the site.
    pub subscribed: bool,
    pub trial_used: bool,
    pub trial_requested: bool,
}

impl Account {
    pub fn parse(v: &Value) -> Account {
        let s = |p: &str| v.pointer(p).and_then(Value::as_str).map(str::to_string);
        let plan = s("/formule");
        let until = match plan.as_deref() {
            Some("essai") => s("/essai/fin"),
            _ => s("/abonnement/fin"),
        };
        Account {
            email: s("/email").unwrap_or_default(),
            open: v.get("acces").and_then(Value::as_bool).unwrap_or(false),
            plan,
            reason: s("/raison"),
            until,
            used: v.get("utilise").and_then(Value::as_u64).map(|u| u.min(100) as u32),
            subscribed: s("/abonnement/statut").is_some_and(|st| st != "canceled"),
            trial_used: s("/essai/fin").is_some(),
            trial_requested: s("/essai/demandeLe").is_some(),
        }
    }
}

/// `2026-11-24T10:00:00Z` as `24/11/2026`; anything else as it is.
pub fn short_date(iso: &str) -> String {
    match (iso.get(0..4), iso.get(5..7), iso.get(8..10)) {
        (Some(y), Some(m), Some(d)) if iso.as_bytes().get(4) == Some(&b'-') => format!("{d}/{m}/{y}"),
        _ => iso.to_string(),
    }
}

/// An answer of the site's JSON routes: the body, or the status and the site's message.
pub type Answer = Result<Value, (u16, String)>;

#[cfg(not(target_arch = "wasm32"))]
fn call(http: &super::backend::HttpFn, method: &'static str, path: &str, body: Option<Value>, token: Option<&str>) -> Answer {
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    if let Some(t) = token {
        headers.push(("Authorization".to_string(), format!("Bearer {t}")));
    }
    let request = super::backend::HttpRequest {
        method,
        url: format!("{}{path}", site()),
        headers,
        body: body.map(|b| b.to_string().into_bytes()),
        timeout_s: 30,
    };
    let (status, bytes) = http(&request).map_err(|e| (0, super::trf("ui.node_ia_unreachable", &[("error", &e)])))?;
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(v);
    }
    let message = v
        .pointer("/error/message")
        .or_else(|| v.get("message"))
        .or_else(|| v.get("error"))
        .and_then(Value::as_str)
        .map_or_else(|| format!("HTTP {status}"), str::to_string);
    Err((status, message))
}

/// What the settings page is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    SendCode,
    Verify,
    Account,
    Trial,
}

/// The NØDE IA part of Preferences > AI.
#[derive(Default)]
pub struct State {
    pub email: String,
    pub code: String,
    pub code_sent: bool,
    #[cfg(not(target_arch = "wasm32"))]
    job: Option<(Job, std::sync::mpsc::Receiver<Answer>)>,
    pub account: Option<Account>,
    /// A line to show: its text, and whether it is an error.
    pub message: Option<(String, bool)>,
    /// The account was asked for since the page opened.
    pub checked: bool,
}

impl State {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// Start one request to the site off the window thread.
#[cfg(not(target_arch = "wasm32"))]
fn start(app: &mut crate::SolveApp, ctx: &egui::Context, job: Job) {
    let Some(http) = app.services.agent_http.clone() else {
        app.agent_chat.node_ia.message = Some((crate::i18n::tr("ui.ai_no_http").to_string(), true));
        return;
    };
    let token = if matches!(job, Job::Account | Job::Trial) { app.services.secrets.as_ref().and_then(|s| s.get(ID).ok().flatten()) } else { None };
    let s = &mut app.agent_chat.node_ia;
    let (email, code) = (s.email.trim().to_lowercase(), s.code.trim().to_string());
    let (tx, rx) = std::sync::mpsc::channel();
    let wake = ctx.clone();
    std::thread::spawn(move || {
        let answer = match job {
            Job::SendCode => call(&http, "POST", "/api/cad/code", Some(json!({"email": email, "application": true})), None),
            Job::Verify => call(&http, "PUT", "/api/cad/code", Some(json!({"email": email, "code": code, "application": true})), None),
            Job::Account => call(&http, "GET", "/api/cad/ia/compte", None, token.as_deref()),
            Job::Trial => call(&http, "POST", "/api/cad/ia/essai", Some(json!({})), token.as_deref()),
        };
        let _ = tx.send(answer);
        wake.request_repaint();
    });
    s.job = Some((job, rx));
    s.message = None;
}

/// Forget the account here: the token leaves the keychain, the address the preferences.
#[cfg(not(target_arch = "wasm32"))]
pub fn sign_out(app: &mut crate::SolveApp) {
    if let Some(s) = &app.services.secrets {
        let _ = s.delete(ID);
    }
    app.preferences.node_ia_email.clear();
    let s = &mut app.agent_chat.node_ia;
    s.account = None;
    s.checked = false;
    s.code.clear();
    s.code_sent = false;
}

/// Take the answer of the running request, if it came.
#[cfg(not(target_arch = "wasm32"))]
fn poll(app: &mut crate::SolveApp, ctx: &egui::Context) {
    let Some((job, rx)) = &app.agent_chat.node_ia.job else { return };
    let (job, answer) = match rx.try_recv() {
        Ok(a) => (*job, a),
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => (*job, Err((0, "—".to_string()))),
    };
    app.agent_chat.node_ia.job = None;
    match (job, answer) {
        (Job::SendCode, Ok(_)) => {
            let s = &mut app.agent_chat.node_ia;
            s.code_sent = true;
            s.message = Some((super::trf("ui.node_ia_code_sent", &[("email", s.email.trim())]), false));
        }
        (Job::Verify, Ok(v)) => {
            let token = v.get("jeton").and_then(Value::as_str).unwrap_or_default().to_string();
            let email = v.get("email").and_then(Value::as_str).unwrap_or_default().to_string();
            let stored = match &app.services.secrets {
                Some(store) if !token.is_empty() => store.set(ID, &token).map_err(|e| super::trf("ui.ai_keychain_error", &[("error", &e)])),
                Some(_) => Err(crate::i18n::tr("ui.node_ia_no_token").to_string()),
                None => Err(crate::i18n::tr("ui.ai_no_keychain").to_string()),
            };
            match stored {
                Ok(()) => {
                    app.preferences.node_ia_email = email;
                    let s = &mut app.agent_chat.node_ia;
                    s.code.clear();
                    s.code_sent = false;
                    start(app, ctx, Job::Account);
                    app.agent_chat.node_ia.checked = true;
                }
                Err(e) => app.agent_chat.node_ia.message = Some((e, true)),
            }
        }
        (Job::Account, Ok(v)) => app.agent_chat.node_ia.account = Some(Account::parse(&v)),
        // The token no longer opens the account (signed out everywhere, or expired).
        (Job::Account, Err((401, _))) => sign_out(app),
        (Job::Trial, Ok(v)) => {
            let message = v.get("message").and_then(Value::as_str).unwrap_or_default().to_string();
            app.agent_chat.node_ia.message = Some((message, false));
            start(app, ctx, Job::Account);
        }
        (_, Err((_, message))) => app.agent_chat.node_ia.message = Some((message, true)),
    }
}

/// Preferences > AI in the official app: the NØDE account, its subscription or trial, and the
/// buttons that lead to them.
#[cfg(not(target_arch = "wasm32"))]
pub fn section(app: &mut crate::SolveApp, ui: &mut egui::Ui) {
    use super::trf;
    use crate::i18n::tr;
    use crate::ui_kit;
    let ctx = ui.ctx().clone();
    poll(app, &ctx);
    if signed_in(app) && !app.agent_chat.node_ia.checked && !app.agent_chat.node_ia.busy() {
        app.agent_chat.node_ia.checked = true;
        start(app, &ctx, Job::Account);
    }
    let mut action: Option<Job> = None;
    let mut out = false;
    let mut open_page = false;
    let t = crate::theme::Tokens::get();
    ui_kit::settings_section(ui, "node_ia", tr("ui.node_ia_account"), |ui| {
        ui_kit::text(ui, tr("ui.node_ia_intro"), ui_kit::Text::Note("node_ia_intro"));
        let busy = app.agent_chat.node_ia.busy();
        if signed_in(app) {
            ui_kit::text(ui, &trf("ui.node_ia_signed_in", &[("email", &app.preferences.node_ia_email)]), ui_kit::Text::Inline);
            match &app.agent_chat.node_ia.account {
                None => {
                    ui_kit::text(ui, tr("ui.node_ia_checking"), ui_kit::Text::Hint("node_ia_checking"));
                }
                Some(a) => {
                    let date = a.until.as_deref().map(short_date).unwrap_or_default();
                    let status = match a.plan.as_deref() {
                        Some("abonnement") => trf("ui.node_ia_subscription", &[("date", &date)]),
                        Some("essai") => trf("ui.node_ia_trial", &[("date", &date)]),
                        _ => tr("ui.node_ia_none").to_string(),
                    };
                    ui_kit::text(ui, &status, ui_kit::Text::Report("node_ia_status", !a.open));
                    if let (true, Some(used)) = (a.open, a.used) {
                        ui_kit::text(ui, &trf("ui.node_ia_used", &[("percent", &used.to_string())]), ui_kit::Text::Hint("node_ia_used"));
                    }
                    if let (false, Some(reason)) = (a.open, &a.reason) {
                        ui_kit::text(ui, reason, ui_kit::Text::Note("node_ia_reason"));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if a.subscribed {
                            if ui_kit::button(ui, tr("ui.node_ia_manage"), ui_kit::Button::Normal).clicked() {
                                open_page = true;
                            }
                        } else if !a.open || a.plan.as_deref() == Some("essai") {
                            if ui_kit::button(ui, tr("ui.node_ia_subscribe"), ui_kit::Button::Primary).clicked() {
                                open_page = true;
                            }
                            if !a.open
                                && !a.trial_used
                                && !a.trial_requested
                                && ui_kit::button(ui, tr("ui.node_ia_request_trial"), ui_kit::Button::Enabled(!busy)).clicked()
                            {
                                action = Some(Job::Trial);
                            }
                        }
                        if ui_kit::button(ui, tr("ui.node_ia_refresh"), ui_kit::Button::Enabled(!busy)).clicked() {
                            action = Some(Job::Account);
                        }
                        if ui_kit::button(ui, tr("ui.node_ia_sign_out"), ui_kit::Button::Ghost).clicked() {
                            out = true;
                        }
                    });
                    if a.trial_requested && !a.trial_used && !a.open {
                        ui_kit::text(ui, tr("ui.node_ia_trial_requested"), ui_kit::Text::Hint("node_ia_trial_requested"));
                    }
                }
            }
        } else {
            let s = &mut app.agent_chat.node_ia;
            ui_kit::settings_form(ui, t.space_24 * 6.0 + t.space_6, |ui| {
                ui_kit::row(ui, tr("ui.node_ia_email"), |ui| {
                    ui.add_enabled_ui(!s.code_sent, |ui| {
                        ui_kit::text_field(ui, &mut s.email, "vous@entreprise.fr");
                    });
                });
                if s.code_sent {
                    ui_kit::row(ui, tr("ui.node_ia_code"), |ui| {
                        let (enter, _) = ui_kit::text_field(ui, &mut s.code, "");
                        if enter && s.code.trim().len() == 6 && !busy {
                            action = Some(Job::Verify);
                        }
                    });
                }
            });
            ui.horizontal_wrapped(|ui| {
                if s.code_sent {
                    let ready = s.code.trim().len() == 6 && !busy;
                    if ui_kit::button(ui, tr("ui.node_ia_sign_in"), if ready { ui_kit::Button::Primary } else { ui_kit::Button::Enabled(false) })
                        .clicked()
                    {
                        action = Some(Job::Verify);
                    }
                    if ui_kit::button(ui, tr("ui.node_ia_change_email"), ui_kit::Button::Ghost).clicked() {
                        s.code_sent = false;
                        s.code.clear();
                        s.message = None;
                    }
                } else {
                    let ready = s.email.contains('@') && s.email.trim().len() > 4 && !busy;
                    if ui_kit::button(ui, tr("ui.node_ia_send_code"), if ready { ui_kit::Button::Primary } else { ui_kit::Button::Enabled(false) })
                        .clicked()
                    {
                        action = Some(Job::SendCode);
                    }
                }
                if ui_kit::button(ui, tr("ui.node_ia_discover"), ui_kit::Button::Ghost).clicked() {
                    open_page = true;
                }
            });
        }
        if busy {
            ui_kit::text(ui, tr("ui.node_ia_waiting"), ui_kit::Text::Hint("node_ia_waiting"));
        }
        if let Some((text, error)) = &app.agent_chat.node_ia.message {
            ui_kit::text(ui, text, ui_kit::Text::Report("node_ia_message", *error));
        }
    });
    if open_page {
        ctx.open_url(egui::OpenUrl::new_tab(subscribe_page()));
    }
    if out {
        sign_out(app);
    } else if let Some(job) = action {
        start(app, &ctx, job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_relay_is_recognised_and_built_from_the_site() {
        assert!(is_node_ia("https://node-os.fr/api/cad/ia/v1"));
        assert!(is_node_ia("http://localhost:3998/api/cad/ia/v1/"));
        assert!(!is_node_ia("https://api.mistral.ai/v1"));
        assert!(base_url().ends_with("/api/cad/ia/v1") && subscribe_page().ends_with("/cad#ia"));
    }

    #[test]
    fn the_account_page_reads_into_what_the_settings_show() {
        let a = Account::parse(&json!({
            "email": "atelier@exemple.fr", "acces": true, "formule": "essai", "raison": null,
            "abonnement": null, "essai": {"fin": "2026-11-07T10:00:00.000Z", "demandeLe": "2026-10-24T09:00:00.000Z"},
            "utilise": 12,
        }));
        assert!(a.open && a.plan.as_deref() == Some("essai") && !a.subscribed && a.trial_used && a.trial_requested);
        assert_eq!(a.until.as_deref().map(short_date).as_deref(), Some("07/11/2026"));
        assert_eq!(a.used, Some(12));
        let closed = Account::parse(
            &json!({"email": "a@b.fr", "acces": false, "formule": null, "raison": "Abonnez-vous", "abonnement": {"statut": "canceled", "fin": null}}),
        );
        assert!(!closed.open && !closed.subscribed && closed.reason.as_deref() == Some("Abonnez-vous"));
        assert_eq!(short_date("demain"), "demain");
    }

    /// The official app offers NØDE IA alone: no installed agent is looked for, an added endpoint
    /// is not listed, and without an account the chat says where to sign in.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_official_app_has_node_ia_alone_and_asks_to_sign_in() {
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.agent_chat.node_ia_only = true;
        app.preferences.agents.push(super::super::agents::AgentConfig { id: "api-1".into(), kind: Kind::Api, ..Default::default() });
        let all = super::super::agents::entries(&app);
        assert_eq!(all.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), [ID]);
        assert!(!all[0].key && is_node_ia(&all[0].base_url) && all[0].model == DEFAULT_MODEL);
        let ctx = egui::Context::default();
        super::super::refresh(&mut app, &ctx);
        assert!(app.agent_chat.detecting.is_none() && app.agent_chat.detection.is_some(), "nothing is looked for");
        super::super::connect(&mut app, &ctx);
        assert!(app.agent_chat.backend.is_none());
        app.preferences.node_ia_email = "atelier@exemple.fr".into();
        assert!(super::super::agents::current(&app).is_some_and(|e| e.key));
    }
}
