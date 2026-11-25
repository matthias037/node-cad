//! Le relais MCP du navigateur : une IA extérieure (Claude, ou tout client MCP) pilote l'onglet de
//! NØDE CAD par node-os.fr.
//!
//! L'onglet ouvre une session sur le site (`POST /api/cad/mcp/session`) et reçoit une adresse MCP
//! secrète, à coller dans l'IA. Le site garde les messages de l'IA ; l'onglet les attend
//! (`GET …/onglet`, attente longue), les traite comme le serveur MCP de l'application de bureau
//! (`solidcraft_mcp`), avec les mêmes outils, le même contrôle des demandes et le même découpage des
//! longs lots image par image que l'assistant du navigateur (`web_client`), et renvoie chaque
//! réponse (`POST …/onglet`). Rien ne tourne sur le site : le dessin reste dans l'onglet.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use serde_json::{Value, json};
use solidcraft_engine::timing::Clock;

use super::backend::{Fetch, HttpAnswer, HttpRequest};
use super::web_client::{FRAME_BUDGET_MS, Frame, InPage, Long, Tape, answer};
use crate::SolveApp;

/// Le site du relais : celui qui sert l'application (node-os.fr en ligne, le serveur d'essai en
/// local), d'où des adresses relatives.
pub const SITE: &str = "";

/// L'attente longue d'un message de l'IA (s) : le site répond au plus tard au bout de 25 s.
const ATTENTE_S: u64 = 40;

/// Ce que l'onglet montre de la liaison.
#[derive(Clone, Debug, PartialEq)]
pub enum Etat {
    /// La session s'ouvre sur le site.
    Ouverture,
    /// L'adresse à donner à l'IA.
    Prete { adresse: String },
    /// Le site ne répond pas : un nouvel essai suit.
    Coupee(String),
}

enum Arrivee {
    Session(HttpAnswer),
    Messages(HttpAnswer),
    Envoi(HttpAnswer),
}

/// Un appel d'outil dont le long lot tourne sur les images suivantes.
struct EnCours {
    rid: String,
    id: Value,
    outil: String,
    arguments: Value,
    tape: Tape,
    long: Long,
}

/// La liaison d'un onglet avec le relais.
pub struct Relais {
    fetch: Fetch,
    wake: Rc<dyn Fn()>,
    pub etat: Etat,
    jeton: Option<String>,
    /// Une attente longue est en cours.
    ecoute: bool,
    /// Pas de nouvel essai avant ce moment (secondes de l'horloge d'egui).
    pas_avant: f64,
    boite: Rc<RefCell<Vec<Arrivee>>>,
    file: VecDeque<(String, Value)>,
    en_cours: Option<EnCours>,
    /// Les outils appelés par l'IA depuis l'ouverture, et le dernier.
    pub appels: u32,
    pub dernier: Option<String>,
    /// La longueur de l'historique d'annulation à l'ouverture de la liaison.
    depart_undo: usize,
}

impl Relais {
    pub fn new(fetch: Fetch, wake: Rc<dyn Fn()>, depart_undo: usize) -> Relais {
        let r = Relais {
            fetch,
            wake,
            etat: Etat::Ouverture,
            jeton: None,
            ecoute: false,
            pas_avant: 0.0,
            boite: Rc::new(RefCell::new(Vec::new())),
            file: VecDeque::new(),
            en_cours: None,
            appels: 0,
            dernier: None,
            depart_undo,
        };
        r.ouvrir();
        r
    }

    fn envoyer(&self, method: &'static str, url: String, body: Option<Value>, timeout_s: u64, faire: fn(HttpAnswer) -> Arrivee) {
        let request = HttpRequest {
            method,
            url,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.map(|b| b.to_string().into_bytes()),
            timeout_s,
        };
        let (boite, wake) = (self.boite.clone(), self.wake.clone());
        (self.fetch)(
            request,
            Box::new(move |reponse| {
                boite.borrow_mut().push(faire(reponse));
                wake();
            }),
        );
    }

    fn ouvrir(&self) {
        self.envoyer("POST", format!("{SITE}/api/cad/mcp/session"), Some(json!({})), 20, Arrivee::Session);
    }

    fn ecouter(&mut self) {
        let Some(jeton) = &self.jeton else { return };
        self.ecoute = true;
        self.envoyer("GET", format!("{SITE}/api/cad/mcp/{jeton}/onglet"), None, ATTENTE_S, Arrivee::Messages);
    }

    fn repondre(&mut self, rid: String, message: Value) {
        let Some(jeton) = &self.jeton else { return };
        let corps = json!({"reponses": [{"rid": rid, "message": message}]});
        self.envoyer("POST", format!("{SITE}/api/cad/mcp/{jeton}/onglet"), Some(corps), 30, Arrivee::Envoi);
    }

    /// Ferme la session sur le site (le bouton Arrêter, l'onglet qui se ferme proprement).
    pub fn fermer(&self) {
        if let Some(jeton) = &self.jeton {
            self.envoyer("DELETE", format!("{SITE}/api/cad/mcp/{jeton}"), None, 10, Arrivee::Envoi);
        }
    }

    /// Ce que le site a répondu depuis l'image précédente.
    fn lire(&mut self, maintenant: f64) {
        let arrivees: Vec<Arrivee> = std::mem::take(&mut *self.boite.borrow_mut());
        for a in arrivees {
            match a {
                Arrivee::Session(Ok((200, corps))) => match serde_json::from_slice::<Value>(&corps) {
                    Ok(v) => match (v.get("jeton").and_then(Value::as_str), v.get("adresse").and_then(Value::as_str)) {
                        (Some(j), Some(adr)) => {
                            self.jeton = Some(j.to_string());
                            self.etat = Etat::Prete { adresse: adr.to_string() };
                        }
                        _ => self.coupure("réponse du site incomplète".into(), maintenant),
                    },
                    Err(e) => self.coupure(e.to_string(), maintenant),
                },
                Arrivee::Session(Ok((code, _))) => self.coupure(format!("HTTP {code}"), maintenant),
                Arrivee::Session(Err(e)) => self.coupure(e, maintenant),
                Arrivee::Messages(r) => {
                    self.ecoute = false;
                    match r {
                        Ok((200, corps)) => {
                            let v: Value = serde_json::from_slice(&corps).unwrap_or(Value::Null);
                            for m in v.get("messages").and_then(Value::as_array).into_iter().flatten() {
                                if let (Some(rid), Some(msg)) = (m.get("rid").and_then(Value::as_str), m.get("message")) {
                                    self.file.push_back((rid.to_string(), msg.clone()));
                                }
                            }
                        }
                        // La session a expiré (site redémarré, onglet resté longtemps caché) : une nouvelle
                        // adresse, à redonner à l'IA.
                        Ok((404, _)) => {
                            self.jeton = None;
                            self.etat = Etat::Ouverture;
                            self.ouvrir();
                        }
                        Ok((code, _)) => self.pause(format!("HTTP {code}"), maintenant),
                        Err(e) => self.pause(e, maintenant),
                    }
                }
                Arrivee::Envoi(_) => {}
            }
        }
    }

    fn coupure(&mut self, raison: String, maintenant: f64) {
        self.etat = Etat::Coupee(raison);
        self.pas_avant = maintenant + 5.0;
    }

    /// Une attente perdue (réseau) : la suivante dans 3 s, l'adresse reste la même.
    fn pause(&mut self, _raison: String, maintenant: f64) {
        self.pas_avant = maintenant + 3.0;
    }
}

/// Le serveur MCP de l'onglet, pour tout ce qui n'est pas un appel d'outil (`initialize`,
/// `tools/list`, `ping`…) : il ne touche pas au dessin.
struct SansDessin;

impl solidcraft_mcp::Backend for SansDessin {
    fn call(&mut self, _method: &str, _params: Value) -> Result<Value, String> {
        Err("read the design with the inspect_design tool".into())
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn describe(&self) -> String {
        "the NØDE CAD app open in the person's browser tab (node-os.fr/cad/app), reached through the NØDE OS relay; the person watches \
         the viewport"
            .into()
    }
}

fn reponse(id: Value, resultat: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": resultat})
}

/// Le relais de l'onglet, chaque image : ce que le site a répondu, l'attente suivante, puis les
/// messages de l'IA, dans le temps d'une image.
pub fn pump(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut r) = app.agent_chat.relais.take() else { return };
    let maintenant = ctx.input(|i| i.time);
    r.lire(maintenant);
    if matches!(r.etat, Etat::Coupee(_)) && maintenant >= r.pas_avant && r.jeton.is_none() {
        r.etat = Etat::Ouverture;
        r.ouvrir();
    }
    if r.jeton.is_some() && !r.ecoute && maintenant >= r.pas_avant {
        r.ecouter();
    }
    let frame = Frame { clock: Clock::start(), budget_ms: FRAME_BUDGET_MS };
    let limite = app.preferences.agent_chat_turn_limit.clamp(1, 1000);
    let mut premier = true;
    while r.en_cours.is_some() || !r.file.is_empty() {
        if !premier && frame.spent() {
            ctx.request_repaint();
            break;
        }
        premier = false;
        if let Some(c) = r.en_cours.take() {
            continuer(app, ctx, &mut r, c, &frame, limite);
        } else if let Some((rid, msg)) = r.file.pop_front() {
            traiter(app, ctx, &mut r, rid, msg, limite);
        }
    }
    if !r.ecoute || r.pas_avant > maintenant {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    app.agent_chat.relais = Some(r);
}

fn traiter(app: &mut SolveApp, ctx: &egui::Context, r: &mut Relais, rid: String, msg: Value, limite: u32) {
    let methode = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let id = msg.get("id").cloned();
    if methode != "tools/call" {
        let mut serveur = solidcraft_mcp::Server::new(Box::new(SansDessin));
        let rep = serveur.handle(msg.clone()).unwrap_or(Value::Null);
        // Une notification n'a pas de réponse : le site attend quand même un mot, pour la libérer.
        r.repondre(rid, rep);
        return;
    }
    let Some(id) = id else { return r.repondre(rid, Value::Null) };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let outil = params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(a) => a.clone(),
    };
    r.appels += 1;
    r.dernier = Some(outil.clone());
    appeler(app, Some(ctx), r, rid, id, outil, arguments, Tape::default(), limite);
}

#[allow(clippy::too_many_arguments)]
fn appeler(app: &mut SolveApp, ctx: Option<&egui::Context>, r: &mut Relais, rid: String, id: Value, outil: String, arguments: Value, mut tape: Tape, limite: u32) {
    tape.read = 0;
    let mut page = InPage { app, ctx, limit: limite, tape: Some(&mut tape), begun: None, relais_undo: Some(r.depart_undo) };
    let resultat = solidcraft_mcp::call_tool(&mut page, &outil, &arguments);
    if let Some(long) = page.begun.take() {
        r.en_cours = Some(EnCours { rid, id, outil, arguments, tape, long });
        return;
    }
    r.repondre(rid, reponse(id, resultat));
}

fn continuer(app: &mut SolveApp, ctx: &egui::Context, r: &mut Relais, mut c: EnCours, frame: &Frame, limite: u32) {
    let obtenu = match c.long {
        Long::Steps(mut lot) => match lot.run(app, || !frame.spent()) {
            Some(rep) => answer(rep),
            None => {
                c.long = Long::Steps(lot);
                r.en_cours = Some(c);
                return;
            }
        },
        Long::Measure(mut m) => match m.go_on(app, ctx, frame) {
            Some(rep) => rep,
            None => {
                c.long = Long::Measure(m);
                r.en_cours = Some(c);
                return;
            }
        },
    };
    c.tape.answered(obtenu);
    appeler(app, Some(ctx), r, c.rid, c.id, c.outil, c.arguments, c.tape, limite);
}

/// Ouvre la liaison (fenêtre « Connecter une IA » du navigateur). `false` : pas de réseau ici.
pub fn demarrer(app: &mut SolveApp, ctx: &egui::Context) -> bool {
    if app.agent_chat.relais.is_some() {
        return true;
    }
    let Some(fetch) = app.services.fetch.clone() else { return false };
    let c = ctx.clone();
    let depart = app.session.undo.len();
    app.agent_chat.relais = Some(Relais::new(fetch, Rc::new(move || c.request_repaint()), depart));
    true
}

/// Coupe la liaison : l'adresse ne sert plus.
pub fn arreter(app: &mut SolveApp) {
    if let Some(r) = app.agent_chat.relais.take() {
        r.fermer();
    }
}
