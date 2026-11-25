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
/// In the browser, the site that served the page (node-os.fr serves the web app at `/cad/app/`):
/// its addresses are relative to the page.
pub fn site() -> String {
    #[cfg(target_arch = "wasm32")]
    return String::new();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(s) = std::env::var("NODE_CAD_IA_SITE").ok().filter(|s| s.starts_with("http")) {
            return s.trim_end_matches('/').to_string();
        }
        SITE.to_string()
    }
}

/// How long one NØDE IA request may take: no time limit (06/11/2026). Its server plans, writes a
/// program, has the engine judge it and asks for corrections, all in one request. At 300 s a CNC
/// lathe's sixth try came too late (04/11/2026); at 420 s a mountain bike from Claude was cut at
/// its third round and all its work was lost. The relay now waits for its model without a limit
/// too; the app waits as long (a day, only against a relay that never answers).
pub const REQUEST_TIMEOUT_S: u64 = super::agents::UNLIMITED_REQUEST_TIMEOUT_S;

/// The relay, as an OpenAI-compatible base URL.
pub fn base_url() -> String {
    format!("{}/api/cad/ia/v1", site())
}

/// Whether `base_url` is NØDE IA's relay (its OCR reads documents, like Mistral's).
pub fn is_node_ia(base_url: &str) -> bool {
    base_url.trim().trim_end_matches('/').ends_with("/api/cad/ia/v1")
}

/// The header that names a chat request to the relay, so that what NØDE IA does meanwhile can be
/// read while the app waits for the answer (`/progres`): its plan, its tries, its reasoning.
pub const REQUEST_HEADER: &str = "X-NodeIA-Requete";
/// The header by which the app marks a conversation confidential (`1`): NØDE IA then answers it
/// with Mistral, in Europe, to the end, like a conversation with a document attached. Without it,
/// the relay may answer with another model (its `routage.ts`).
pub const CONFIDENTIAL_HEADER: &str = "X-NodeIA-Confidentiel";
/// How often the progress of a request is read while it runs.
pub const PROGRESS_EVERY: std::time::Duration = std::time::Duration::from_secs(1);
/// Reads in a row that get no progress back (a relay without the route, say) before the app
/// stops reading it for that request.
const PROGRESS_FAILURES: u32 = 3;
/// A read still unanswered after this long counts as failed (the browser's `fetch` has no
/// timeout of its own), and the next one goes.
const PROGRESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// Lines taken from one progress answer, and characters from one line.
const PROGRESS_LINES: usize = 200;
const PROGRESS_LINE_CHARS: usize = 4_000;

/// A new id for one request to the relay: 32 hexadecimal digits, different for every request.
/// On the desktop the standard library's random keys make it unguessable; in the browser, the
/// clock, a counter and an address make it unique (the relay ties it to the account anyway).
pub fn request_id() -> String {
    use std::fmt::Write;
    use std::hash::{BuildHasher, Hasher};
    static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let wall = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let here = Box::new(n);
    let mut id = String::with_capacity(32);
    for salt in [0x6e6f_6465_u64, 0x6961_6469_7265_6374] {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(salt);
        h.write_u64(n);
        h.write_u128(wall);
        h.write_usize(std::ptr::from_ref::<u64>(&here) as usize);
        #[cfg(not(target_arch = "wasm32"))]
        h.write_u32(std::process::id());
        let _ = write!(id, "{:016x}", h.finish());
    }
    id
}

/// What NØDE IA does while a request runs, read from the relay about every second (`/progres`)
/// until the answer comes: one read at a time, and none once the relay says the request is done
/// or a few reads in a row got nothing back. Its lines go to the chat as dimmed thoughts.
#[derive(Debug)]
pub struct Follow {
    id: String,
    every: std::time::Duration,
    /// The number of the last line shown.
    after: u64,
    /// Lines shown so far: each one after the first goes on a line of its own.
    shown: usize,
    /// When the last read was sent or answered (the start, before the first).
    since: web_time::Instant,
    asking: bool,
    failures: u32,
    done: bool,
}

impl Follow {
    pub fn new(id: String, every: std::time::Duration) -> Follow {
        Follow { id, every, after: 0, shown: 0, since: web_time::Instant::now(), asking: false, failures: 0, done: false }
    }

    /// The follow of `req`, when it names itself to the relay ([`REQUEST_HEADER`]).
    pub fn of(req: &super::backend::HttpRequest, every: std::time::Duration) -> Option<Follow> {
        req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(REQUEST_HEADER)).map(|(_, id)| Follow::new(id.clone(), every))
    }

    /// How long until the next read is due, or until the read in flight counts as failed; `None`
    /// once no read will come.
    pub fn wait(&self) -> Option<std::time::Duration> {
        if self.done || self.failures >= PROGRESS_FAILURES {
            return None;
        }
        Some((if self.asking { PROGRESS_TIMEOUT } else { self.every }).saturating_sub(self.since.elapsed()))
    }

    /// Whether a read is due now ([`Follow::ask`]). A read left unanswered too long counts as
    /// failed first.
    pub fn due(&mut self) -> bool {
        if self.asking && self.since.elapsed() >= PROGRESS_TIMEOUT {
            self.asking = false;
            self.failures += 1;
        }
        !self.asking && self.wait().is_some_and(|w| w.is_zero())
    }

    /// The read to send now (`base_url`: the relay's; `token`: the account's); it is in flight
    /// until [`Follow::answered`].
    pub fn ask(&mut self, base_url: &str, token: Option<&str>) -> super::backend::HttpRequest {
        self.asking = true;
        self.since = web_time::Instant::now();
        let mut headers = Vec::new();
        if let Some(t) = token {
            headers.push(("Authorization".to_string(), format!("Bearer {t}")));
        }
        super::backend::HttpRequest {
            method: "GET",
            url: format!("{}/progres?requete={}&depuis={}", base_url.trim_end_matches('/'), self.id, self.after),
            headers,
            body: None,
            timeout_s: 15,
        }
    }

    /// The answer of the read in flight: the texts to show, in order (dimmed thoughts).
    pub fn answered(&mut self, answer: &super::backend::HttpAnswer) -> Vec<String> {
        self.asking = false;
        self.since = web_time::Instant::now();
        let read = answer.as_ref().ok().filter(|(status, _)| *status == 200).and_then(|(_, body)| serde_json::from_slice::<Value>(body).ok());
        let Some((v, lines)) = read.as_ref().and_then(|v| Some((v, v.get("lignes")?.as_array()?))) else {
            self.failures += 1;
            return Vec::new();
        };
        self.failures = 0;
        self.done = v.get("fini").and_then(Value::as_bool).unwrap_or(false);
        let mut new: Vec<(u64, bool, &str)> = lines
            .iter()
            .take(PROGRESS_LINES)
            .filter_map(|l| {
                let n = l.get("n").and_then(Value::as_u64).filter(|n| *n > self.after)?;
                let text = l.get("texte").and_then(Value::as_str).unwrap_or("").trim();
                Some((n, l.get("genre").and_then(Value::as_str) == Some("reflexion"), text))
            })
            .collect();
        new.sort_by_key(|(n, ..)| *n);
        new.dedup_by_key(|(n, ..)| *n);
        let mut texts = Vec::with_capacity(new.len());
        for (n, reasoning, text) in new {
            self.after = n;
            if text.is_empty() {
                continue;
            }
            let text: String = text.chars().take(PROGRESS_LINE_CHARS).collect();
            // The model's own reasoning between quotes; the steps as the relay words them.
            let line = if reasoning { format!("« {text} »") } else { text };
            texts.push(if self.shown == 0 { line } else { format!("\n{line}") });
            self.shown += 1;
        }
        texts
    }
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
        allow_insecure_http: false,
        reasoning_effort: String::new(),
        version: None,
        login: "",
        found: false,
        base_url: base_url(),
        model: app.preferences.agent_models.get(ID).cloned().filter(|m| !m.is_empty()).unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        key: signed_in(app),
        request_timeout_s: REQUEST_TIMEOUT_S,
        problem: None,
        install: "",
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

/// A request to the site, with the account's token when the route needs one.
fn request(method: &'static str, path: &str, body: Option<Value>, token: Option<&str>) -> super::backend::HttpRequest {
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    if let Some(t) = token {
        headers.push(("Authorization".to_string(), format!("Bearer {t}")));
    }
    super::backend::HttpRequest { method, url: format!("{}{path}", site()), headers, body: body.map(|b| b.to_string().into_bytes()), timeout_s: 30 }
}

/// What the site answered, as the account section reads it.
fn answer(sent: super::backend::HttpAnswer) -> Answer {
    let (status, bytes) = sent.map_err(|e| (0, super::trf("ui.node_ia_unreachable", &[("error", &e)])))?;
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
    job: Option<(Job, std::sync::mpsc::Receiver<Answer>)>,
    pub account: Option<Account>,
    /// A line to show: its text, and whether it is an error.
    pub message: Option<(String, bool)>,
    /// The account was asked for since the page opened.
    pub checked: bool,
}

impl State {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// Start one request to the site without holding the window: from a thread on the desktop,
/// through the page's `fetch` in the browser.
fn start(app: &mut crate::SolveApp, ctx: &egui::Context, job: Job) {
    let token = if matches!(job, Job::Account | Job::Trial) { app.services.secrets.as_ref().and_then(|s| s.get(ID).ok().flatten()) } else { None };
    let s = &app.agent_chat.node_ia;
    let (email, code) = (s.email.trim().to_lowercase(), s.code.trim().to_string());
    let req = match job {
        Job::SendCode => request("POST", "/api/cad/code", Some(json!({"email": email, "application": true})), None),
        Job::Verify => request("PUT", "/api/cad/code", Some(json!({"email": email, "code": code, "application": true})), None),
        Job::Account => request("GET", "/api/cad/ia/compte", None, token.as_deref()),
        Job::Trial => request("POST", "/api/cad/ia/essai", Some(json!({})), token.as_deref()),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let wake = ctx.clone();
    if let Some(fetch) = app.services.fetch.clone() {
        fetch(
            req,
            Box::new(move |sent| {
                let _ = tx.send(answer(sent));
                wake.request_repaint();
            }),
        );
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        let Some(http) = app.services.agent_http.clone() else {
            app.agent_chat.node_ia.message = Some((crate::i18n::tr("ui.ai_no_http").to_string(), true));
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || {
            let _ = tx.send(answer(http(&req)));
            wake.request_repaint();
        });
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (req, tx, wake);
            app.agent_chat.node_ia.message = Some((crate::i18n::tr("ui.ai_no_http").to_string(), true));
            return;
        }
    }
    let s = &mut app.agent_chat.node_ia;
    s.job = Some((job, rx));
    s.message = None;
}

/// Forget the account here: the token leaves the keychain, the address the preferences.
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
pub fn section(app: &mut crate::SolveApp, ui: &mut egui::Ui) {
    // NØDE IA est retiré (25/11/2026) : la version web n'offre plus que l'IA de la personne, branchée
    // par le relais MCP (Aide > Connecter des assistants IA).
    if cfg!(target_arch = "wasm32") {
        crate::ui_kit::text(
            ui,
            crate::i18n::tr_label("NØDE IA is no longer offered. Plug in your own AI: Help > Connect AI assistants, then paste the address into Claude or another MCP-compatible AI."),
            crate::ui_kit::Text::Note("node_ia_retire"),
        );
        return;
    }
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
    fn each_request_gets_an_id_of_its_own_in_the_relays_format() {
        let ids: Vec<String> = (0..64).map(|_| request_id()).collect();
        assert!(ids.iter().all(|id| id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())), "{ids:?}");
        assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), ids.len());
    }

    fn progress(lines: Value, done: bool) -> super::super::backend::HttpAnswer {
        Ok((200, json!({"lignes": lines, "fini": done}).to_string().into_bytes()))
    }

    /// A request names itself; its progress is read one read at a time, once the interval is up;
    /// each line comes once, in order, the model's reasoning between quotes, and no read is due
    /// once the relay says the request is done.
    #[test]
    fn the_progress_of_a_request_is_read_one_read_at_a_time_and_each_line_comes_once() {
        use super::super::backend::HttpRequest;
        use std::time::Duration;
        let id = "0123456789abcdef0123456789abcdef";
        let chat = HttpRequest {
            method: "POST",
            url: format!("{}/chat/completions", base_url()),
            headers: vec![(REQUEST_HEADER.into(), id.into())],
            body: None,
            timeout_s: 300,
        };
        assert!(
            Follow::of(&HttpRequest { headers: Vec::new(), ..chat.clone() }, Duration::ZERO).is_none(),
            "a request that does not name itself is not followed"
        );
        let mut later = Follow::of(&chat, Duration::from_secs(3600)).unwrap();
        assert!(!later.due() && later.wait().is_some_and(|w| w > Duration::from_secs(3000)), "not before its time");
        let mut f = Follow::of(&chat, Duration::ZERO).unwrap();
        assert!(f.due());
        let read = f.ask("https://node-os.fr/api/cad/ia/v1/", Some("jeton"));
        assert_eq!(
            (read.method, read.url.as_str()),
            ("GET", "https://node-os.fr/api/cad/ia/v1/progres?requete=0123456789abcdef0123456789abcdef&depuis=0")
        );
        assert!(read.headers.contains(&("Authorization".to_string(), "Bearer jeton".to_string())) && read.body.is_none());
        assert!(!f.due(), "one read at a time");
        let texts = f.answered(&progress(
            json!([
                {"n": 2, "t": 900, "genre": "reflexion", "texte": "Un bâti au sol, puis le vérin."},
                {"n": 1, "t": 0, "genre": "plan", "texte": "Plan en cours…"},
                {"n": 3, "t": 950, "genre": "info", "texte": "  "},
            ]),
            false,
        ));
        assert_eq!(texts, ["Plan en cours…", "\n« Un bâti au sol, puis le vérin. »"]);
        assert!(f.due());
        assert!(f.ask(&base_url(), None).url.ends_with("&depuis=3"), "an empty line is not asked for again");
        let texts = f.answered(&progress(
            json!([{"n": 2, "texte": "déjà vue"}, {"n": 4, "genre": "controle", "texte": "Essai 1 : 1 pièce en l'air, correction…"}]),
            true,
        ));
        assert_eq!(texts, ["\nEssai 1 : 1 pièce en l'air, correction…"]);
        assert!(!f.due() && f.wait().is_none(), "the request is done: no more reads");
    }

    /// A relay without the progress route (or one that fails) is left alone after a few reads; a
    /// read that never answers counts as failed after a while, and the next one goes.
    #[test]
    fn a_relay_without_progress_is_left_alone_after_a_few_reads() {
        use std::time::Duration;
        let mut f = Follow::new("0123456789abcdef".into(), Duration::ZERO);
        f.ask("http://127.0.0.1:3998/api/cad/ia/v1", None);
        f.since = web_time::Instant::now().checked_sub(PROGRESS_TIMEOUT + Duration::from_secs(1)).unwrap();
        assert!(f.due(), "a read left unanswered too long counts as failed");
        for answer in [Err("refused".to_string()), Ok((404, b"<html>".to_vec()))] {
            assert!(f.due());
            f.ask("http://127.0.0.1:3998/api/cad/ia/v1", None);
            assert!(f.answered(&answer).is_empty());
        }
        assert!(!f.due() && f.wait().is_none(), "three failures in a row: no more reads");
        let mut g = Follow::new("0123456789abcdef".into(), Duration::ZERO);
        for _ in 0..2 {
            g.ask("http://127.0.0.1:3998/api/cad/ia/v1", None);
            g.answered(&Ok((200, b"{\"autre\": 1}".to_vec())));
        }
        g.ask("http://127.0.0.1:3998/api/cad/ia/v1", None);
        assert!(g.answered(&progress(json!([]), false)).is_empty());
        assert!(g.due(), "an answer of the route starts the count again");
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
    /// A new tab gets a chat of its own: the official app still offers NØDE IA alone there (the
    /// 0.28 merge lost it, and a tab opened from the start page talked to a direct Mistral agent).
    fn a_new_tab_keeps_node_ia_alone() {
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.agent_chat.node_ia_only = true;
        app.preferences.agents.push(super::super::agents::AgentConfig { id: "api-1".into(), kind: Kind::Api, ..Default::default() });
        app.agent_chat.swap_document(&mut super::super::AgentChat::default());
        let all = super::super::agents::entries(&app);
        assert_eq!(all.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), [ID]);
    }

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
