//! The Help menu (the ? button on the application bar): Documentation (the online book), About
//! NØDE CAD (version, commit, build date, licences, attributions, fonts), the keyboard shortcuts
//! (read only), Messages (every status message of the run, #2763), and Report an Issue, which
//! opens a local editable report draft.

use crate::SolveApp;
use crate::ui_kit;
use crate::ui_kit::{Container, Surface};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMMIT: &str = env!("SOLIDCRAFT_COMMIT");
pub const BUILD_DATE: &str = env!("SOLIDCRAFT_BUILD_DATE");
/// The user and developer guide (docs/book, published by .github/workflows/pages.yml).
pub const DOCS_URL: &str = "https://node-os.fr/cad";
/// NØDE OS, from the same publisher: managing the workshop the parts are drawn for.
pub const NODE_OS_URL: &str = "https://node-os.fr/?utm_source=node-cad&utm_medium=app";
const ATTRIBUTION: &str = include_str!("../../../ATTRIBUTION.md");
use solidcraft_engine::licences::{LICENSE_APACHE, LICENSE_MIT, NOTICE, THIRD_PARTY};

#[derive(Default)]
pub struct HelpState {
    pub menu: bool,
    pub about: bool,
    /// The result of Connect AI assistants, one line per client, while its window is open.
    pub connect: Option<Vec<String>>,
    /// La fenêtre « Connecter une IA » du navigateur (relais MCP par node-os.fr) est ouverte.
    pub connect_web: bool,
    /// The GPU the window renders with (set by the host).
    pub gpu: Option<String>,
}

/// One line per fact, for bug reports.
pub fn diagnostics(app: &SolveApp) -> String {
    let renderer = if app.viewport.gpu.is_some() { "GPU" } else { "CPU" };
    let count = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    format!(
        "NØDE CAD {VERSION} ({COMMIT}, built {BUILD_DATE})\nOS: {} {}\nRenderer: {renderer}{}\nWindow: {}\nDesign: {} ({}, {})",
        std::env::consts::OS,
        std::env::consts::ARCH,
        app.help.gpu.as_ref().map(|g| format!(", {g}")).unwrap_or_default(),
        if app.custom_titlebar { "custom title bar" } else { "system title bar" },
        app.session.doc.name,
        count(app.session.doc.features.len(), "feature", "features"),
        count(crate::open_meshing::session_world(&app.session).bodies.len(), "body", "bodies"),
    )
}

/// Whether a Help menu item can run now: behind an open bug report, the items that open a
/// window would cover it (#7188). Report an Issue… shows the report.
pub fn available(app: &SolveApp, item: &str) -> bool {
    app.issue_report.draft.is_none() || !matches!(item, "about" | "shortcuts" | "messages" | "crashes" | "connect" | "updates")
}

/// Run a Help menu item: `docs`, `about`, `updates`, `version` (the release notes), `shortcuts`, `messages`, `report`, `crashes`,
/// `connect`, `install_cli` (macOS).
pub fn run(app: &mut SolveApp, ctx: &egui::Context, item: &str) {
    app.help.menu = false;
    if !available(app, item) {
        return;
    }
    match item {
        "docs" => ctx.open_url(egui::OpenUrl::new_tab(DOCS_URL)),
        "nodeos" => ctx.open_url(egui::OpenUrl::new_tab(NODE_OS_URL)),
        "about" => app.help.about = true,
        "version" => {
            ctx.open_url(egui::OpenUrl::new_tab(crate::update::release_notes_url(crate::update::current_version(), crate::update::release_build())))
        }
        "updates" => crate::update::check_now(app, ctx),
        "shortcuts" => {
            app.keymap.open = true;
            app.keymap.read_only = true;
        }
        "crashes" => crate::crash_ui::open(app),
        // Dans le navigateur, l'IA se branche par le relais de node-os.fr.
        "connect" if app.services.fetch.is_some() && cfg!(target_arch = "wasm32") => {
            crate::agent_chat::relais_mcp::demarrer(app, ctx);
            app.help.connect_web = true;
        }
        "connect" => app.help.connect = Some(connect_assistants()),
        "install_cli" => {
            let (msg, error) = crate::cli_link::install_from_menu();
            app.set_status(msg, error);
        }
        "messages" => app.messages.open = true,
        "report" => {
            let dialog = app.dialog.clone();
            let error = dialog.as_ref().and_then(|d| d.error.clone()).or_else(|| app.preview.error.clone());
            crate::issue_report::begin(app, ctx, dialog.as_ref(), error.as_deref());
        }
        _ => {}
    }
}

/// Add NØDE CAD's MCP server to the AI assistants installed here (`node-cad-cli mcp setup`),
/// with the `node-cad-cli` beside this program. One line per client.
#[cfg(not(target_arch = "wasm32"))]
fn connect_assistants() -> Vec<String> {
    use solidcraft_engine::agent_setup as setup;
    let Some(cli) = crate::agent_chat::find_cli() else {
        return vec!["node-cad-cli was not found next to NØDE CAD; run `node-cad-cli mcp setup` from a terminal.".into()];
    };
    let cli = cli.canonicalize().unwrap_or(cli).to_string_lossy().trim_start_matches(r"\\?\").to_owned();
    match setup::setup(&[], &setup::Env::from_process(), &cli) {
        Ok((lines, _)) => lines,
        Err(e) => vec![e],
    }
}

#[cfg(target_arch = "wasm32")]
fn connect_assistants() -> Vec<String> {
    vec!["Connecting AI assistants needs the desktop app.".into()]
}

/// The Connect AI assistants result window.
fn connect(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(lines) = app.help.connect.clone() else {
        return;
    };
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_connect_assistants"), "help", crate::i18n::tr_label("Connect AI assistants"))
        .cards(2.0)
        .actions("", "Close", None)
        .show(ctx, true, |ui| {
            ui_kit::text(
                ui,
                crate::i18n::tr_label("Restart each assistant to load the NØDE CAD server. Each file was backed up as .bak before it changed."),
                ui_kit::Text::Note("connect_note"),
            );
            ui_kit::text(ui, &lines.join("\n"), ui_kit::Text::Report("connect", false));
        });
    if closed || action == ui_kit::Action::Cancel {
        app.help.connect = None;
    }
}

/// The Help menu under the ? button, and the About window.
pub fn show(app: &mut SolveApp, ctx: &egui::Context, anchor: egui::Pos2) {
    if app.help.menu {
        let mut pick: Option<&str> = None;
        // NØDE CAD offers the update check only where it can run (update::RELEASE_CHECKS).
        let updates = crate::update::available(app);
        let resp = Container::new(ctx, Surface::Popup)
            .area(egui::Id::unique("sc_help_menu"))
            .fixed_pos(anchor)
            .constrain(true)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                crate::context_menu::menu_frame(ui, |ui| {
                    for (id, label, icon) in [
                        ("docs", "Documentation", "library"),
                        ("nodeos", "Discover NØDE OS", "library"),
                        ("about", "About NØDE CAD", "help"),
                        ("updates", crate::i18n::tr("update.check_for_updates"), "update"),
                        ("shortcuts", "Keyboard Shortcuts", "list"),
                        ("messages", "Messages…", "list"),
                        ("report", "Report an Issue…", "bug_report"),
                        ("crashes", "Crash Reports…", "status_error"),
                        ("connect", "Connect AI assistants…", "settings"),
                        ("install_cli", "Install node-cad-cli command", "settings"),
                    ]
                    .into_iter()
                    .filter(|(id, ..)| *id != "install_cli" || cfg!(target_os = "macos"))
                    .filter(|(id, ..)| *id != "updates" || updates)
                    {
                        let item = crate::context_menu::Item::action(id, label, icon).on_if(available(app, id));
                        if crate::context_menu::list_row(ui, &item).is_some() {
                            pick = Some(id);
                        }
                        // A layout without the version in the application bar lists it here.
                        if id == "about" && app.header.version == crate::toolbar::VersionPlace::HelpMenu {
                            let label = crate::i18n::format("help.version", &[("version", crate::update::current_version())]);
                            if crate::context_menu::list_row(ui, &crate::context_menu::Item::action("version", &label, "update")).is_some() {
                                pick = Some("version");
                            }
                        }
                    }
                })
            });
        if let Some(p) = pick {
            run(app, ctx, p);
        } else if ctx.input(|i| i.pointer.any_pressed() || i.key_pressed(egui::Key::Escape)) && !resp.response.contains_pointer() {
            // Pressed elsewhere (the ? button toggles it itself).
            if !ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_help_opened")).unwrap_or(false)) {
                app.help.menu = false;
            }
        }
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sc_help_opened"), false));
    }
    about(app, ctx);
    connect(app, ctx);
    connect_web(app, ctx);
    crate::crash_ui::show(app, ctx);
    crate::messages::show(app, ctx);
}

fn about(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.help.about {
        return;
    }
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_about"), "help", crate::i18n::tr_label("About NØDE CAD"))
        .cards(2.0)
        .actions("", "Close", None)
        .show(ctx, true, |ui| {
            ui_kit::text(
                ui,
                &crate::i18n::format("help.build", &[("version", VERSION), ("commit", COMMIT), ("date", BUILD_DATE)]),
                ui_kit::Text::Report("version", false),
            );
            ui_kit::text(
                ui,
                crate::i18n::tr_label(
                    "The free, open-source parametric 3D CAD of NØDE, written in Rust by the publisher of NØDE OS. Licensed Apache-2.0.",
                ),
                ui_kit::Text::Note("description"),
            );
            let copy = ui_kit::button(ui, crate::i18n::tr_label("Copy diagnostic information"), ui_kit::Button::Normal);
            crate::scenario::publish_target("about:copy_diagnostics", copy.rect);
            if copy.clicked() {
                ctx.copy_text(diagnostics(app));
            }
            egui::ScrollArea::vertical().max_height(crate::theme::Tokens::get().control_height * 15.0).show(ui, |ui| {
                ui_kit::section(ui, "attributions", crate::i18n::tr("help.attributions"), false, |ui| {
                    ui_kit::text(ui, ATTRIBUTION, ui_kit::Text::Report("attributions", false));
                });
                ui_kit::section(ui, "fonts", crate::i18n::tr("help.fonts"), false, |ui| {
                    ui_kit::text(ui, crate::i18n::tr("help.font_details"), ui_kit::Text::Report("fonts", false));
                });
                ui_kit::section(ui, "licences", crate::i18n::tr("help.licences"), false, |ui| {
                    for (id, title, text) in
                        [("notice", "help.notice", NOTICE), ("mit", "help.mit", LICENSE_MIT), ("apache", "help.apache", LICENSE_APACHE)]
                    {
                        ui_kit::section(ui, id, crate::i18n::tr(title), false, |ui| {
                            ui_kit::text(ui, text, ui_kit::Text::Report(id, false));
                        });
                    }
                    ui_kit::section(ui, "third_party", crate::i18n::tr("help.third_party"), false, |ui| {
                        if ui_kit::button(ui, crate::i18n::tr_label("Copy all licences"), ui_kit::Button::Normal).clicked() {
                            ctx.copy_text(solidcraft_engine::licences::all());
                        }
                        ui_kit::text_lines(ui, "sc_third_party", THIRD_PARTY);
                    });
                });
            });
        });
    if closed || action == ui_kit::Action::Cancel {
        app.help.about = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_knows_its_build_and_diagnostics_say_where_it_runs() {
        assert!(!COMMIT.is_empty() && BUILD_DATE.len() == 10, "{COMMIT} {BUILD_DATE}");
        assert!(ATTRIBUTION.contains("ATTRIBUTION") || ATTRIBUTION.contains("Attribution"));
        let app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let d = diagnostics(&app);
        assert!(d.contains(VERSION) && d.contains(std::env::consts::OS) && d.contains("Renderer: CPU"), "{d}");
    }

    /// One feature and one body are counted in the singular (#9085), more in the plural.
    #[test]
    fn diagnostics_count_one_feature_and_one_body_in_the_singular() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert!(diagnostics(&app).ends_with("(0 features, 0 bodies)"), "{}", diagnostics(&app));
        app.session.execute("solid.box", &serde_json::json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(diagnostics(&app).ends_with("(1 feature, 1 body)"), "{}", diagnostics(&app));
        app.session.execute("solid.box", &serde_json::json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0]})).unwrap();
        assert!(diagnostics(&app).ends_with("(2 features, 2 bodies)"), "{}", diagnostics(&app));
    }
}

/// La fenêtre « Connecter une IA » du navigateur : l'adresse MCP secrète de cet onglet, à coller
/// dans l'IA, et ce que l'IA a fait. Fermer la fenêtre laisse la liaison ouverte ; Arrêter la coupe.
fn connect_web(app: &mut SolveApp, ctx: &egui::Context) {
    use crate::agent_chat::relais_mcp::Etat;
    if !app.help.connect_web {
        return;
    }
    let Some(relais) = app.agent_chat.relais.as_ref() else {
        app.help.connect_web = false;
        return;
    };
    let etat = relais.etat.clone();
    let (appels, dernier) = (relais.appels, relais.dernier.clone());
    let mut copier = None;
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_connect_web"), "help", crate::i18n::tr_label("Connect an AI to this tab"))
        .cards(2.0)
        .actions("Stop the link", "Close", None)
        .show(ctx, true, |ui| {
            ui_kit::text(
                ui,
                crate::i18n::tr_label(
                    "Add this address to your AI as an MCP server: in Claude, Settings > Connectors > Add custom connector. The AI then \
                     works in this tab, under your eyes. Anyone who has the address can drive this tab: give it to no one.",
                ),
                ui_kit::Text::Note("connect_web_note"),
            );
            match &etat {
                Etat::Ouverture => {
                    ui_kit::text(ui, crate::i18n::tr_label("Opening the link…"), ui_kit::Text::Note("connect_web_state"));
                }
                Etat::Coupee(raison) => {
                    let t = format!("{} ({raison})", crate::i18n::tr_label("node-os.fr does not answer; trying again…"));
                    ui_kit::text(ui, &t, ui_kit::Text::Note("connect_web_state"));
                }
                Etat::Prete { adresse } => {
                    ui_kit::text(ui, adresse, ui_kit::Text::Report("connect_web_address", false));
                    if ui_kit::button(ui, crate::i18n::tr_label("Copy the address"), ui_kit::Button::Primary).clicked() {
                        copier = Some(adresse.clone());
                    }
                    let suivi = match (&dernier, appels) {
                        (None, _) => crate::i18n::tr_label("Waiting for the AI…").to_string(),
                        (Some(d), n) => format!("{} {n} · {d}", crate::i18n::tr_label("Tool calls from the AI:")),
                    };
                    ui_kit::text(ui, &suivi, ui_kit::Text::Note("connect_web_state"));
                }
            }
        });
    if let Some(a) = copier {
        ctx.copy_text(a);
    }
    if action == ui_kit::Action::Ok {
        crate::agent_chat::relais_mcp::arreter(app);
        app.help.connect_web = false;
    } else if closed || action == ui_kit::Action::Cancel {
        app.help.connect_web = false;
    }
}
