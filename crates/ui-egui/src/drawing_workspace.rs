//! The Drawing workspace: the sheet NØDE CAD draws from the design (`drawing.view.base`, the
//! engine's views, dimensions and title block) on the canvas, its settings, and its exports.
//! Tools not built yet keep their agreed options, disabled; source services keep their ownership.

use egui::{Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::ui_kit::{self, Container, Edge, Surface};
use crate::{SolveApp, theme::Tokens};

#[derive(Default)]
pub struct State {
    previous_tab: Option<String>,
    pub settings_open: bool,
    options: Option<&'static str>,
    strip: Option<Rect>,
    strip_sizes: [Option<egui::Vec2>; 2],
    settings: Option<Rect>,
    /// Sheet Settings and Base View as edited, until Apply.
    form: Option<Form>,
}

/// Paper sizes and standard scales offered in Sheet Settings ("Auto": the engine's choice).
const FORMATS: [&str; 6] = ["Auto", "A4", "A3", "A2", "A1", "A0"];
const SCALES: [&str; 10] = ["Auto", "5:1", "2:1", "1:1", "1:2", "1:5", "1:10", "1:20", "1:50", "1:100"];

/// What a drawing is asked for: drawing.view.base's parameters, as the panels edit them.
#[derive(Clone)]
struct Form {
    format: usize,
    portrait: bool,
    third_angle: bool,
    scale: usize,
    hidden: bool,
    /// Front, top, side, isometric.
    views: [bool; 4],
    title: [String; 7],
}

/// The title block's fields, in `Form::title`'s order.
const TITLE_FIELDS: [(&str, &str); 7] = [
    ("titre", "Title"),
    ("numero", "Drawing number"),
    ("matiere", "Material"),
    ("masse", "Mass"),
    ("auteur", "Drawn by"),
    ("date", "Date"),
    ("tolerance", "General tolerances"),
];

impl Form {
    /// The drawing on show as it was asked for, or the defaults for a first one.
    fn of(app: &SolveApp) -> Form {
        let Some(sh) = crate::drawing_window::sheet(app) else {
            let mut title: [String; 7] = Default::default();
            title[0] = app.session.doc.name.clone();
            title[6] = "ISO 2768-m".into();
            return Form { format: 0, portrait: false, third_angle: false, scale: 0, hidden: true, views: [true; 4], title };
        };
        let r = &sh.request;
        let asked = |k: &str| r.get(k).and_then(Value::as_str);
        let has = |n: &str| sh.views.iter().any(|v| v.name == n);
        let t = &sh.title;
        Form {
            format: asked("format").and_then(|f| FORMATS.iter().position(|x| *x == f)).unwrap_or(0),
            portrait: asked("orientation") == Some("portrait"),
            third_angle: sh.projection == "third_angle",
            scale: asked("scale").and_then(|f| SCALES.iter().position(|x| *x == f)).unwrap_or(0),
            hidden: asked("hidden") != Some("omit"),
            views: [has("front"), has("top"), has("left") || has("right"), has("iso")],
            title: [&t.titre, &t.numero, &t.matiere, &t.masse, &t.auteur, &t.date, &t.tolerance].map(|v| v.to_string()),
        }
    }

    fn params(&self) -> Value {
        let side = if self.third_angle { "right" } else { "left" };
        let views: Vec<&str> = ["front", "top", side, "iso"].into_iter().zip(self.views).filter(|(_, on)| *on).map(|(v, _)| v).collect();
        let mut p = json!({
            "orientation": if self.portrait { "portrait" } else { "landscape" },
            "projection": if self.third_angle { "third_angle" } else { "first_angle" },
            "hidden": if self.hidden { "dashed" } else { "omit" },
            "views": views,
            "title": TITLE_FIELDS.iter().zip(&self.title).map(|((k, _), v)| (k.to_string(), json!(v.trim()))).collect::<serde_json::Map<_, _>>(),
        });
        if self.format > 0 {
            p["format"] = json!(FORMATS[self.format]);
        }
        if self.scale > 0 {
            p["scale"] = json!(SCALES[self.scale]);
        }
        p
    }
}

pub fn active(app: &SolveApp) -> bool {
    matches!(app.ui.tab.as_str(), "DRAWING LAYOUT" | "DRAWING ANNOTATE" | "DRAWING PARTS" | "DRAWING REVIEW")
}

pub fn enter(app: &mut SolveApp, drawing: bool) -> Result<(), String> {
    if drawing {
        if app.session.active_sketch.is_some() || app.tool.is_some() || app.dialog.is_some() || app.file_load.busy() {
            return Err("Finish the current operation before entering Drawing".into());
        }
        if !active(app) {
            app.drawing_workspace.previous_tab = Some(app.ui.tab.clone());
            app.ui.tab = "DRAWING LAYOUT".into();
        }
    } else if active(app) {
        app.ui.tab = app.drawing_workspace.previous_tab.take().unwrap_or_else(|| "MODEL".into());
        app.drawing_workspace.options = None;
        app.drawing_workspace.strip = None;
        app.drawing_workspace.settings = None;
        app.drawing_workspace.form = None;
    }
    app.ui.ribbon_context = None;
    app.cancel_view_animation();
    Ok(())
}

pub fn reset_document(app: &mut SolveApp) {
    if active(app) {
        app.ui.tab = "MODEL".into();
    }
    app.drawing_workspace = State::default();
}

pub fn can_open_options(id: &str) -> bool {
    matches!(id, "drawing.view.base" | "drawing.view.projected" | "drawing.dimension" | "drawing.note" | "drawing.export")
}

/// Dismiss the current Drawing inspector without touching source or drawing data.
pub fn close_panel(app: &mut SolveApp) -> bool {
    let open = app.drawing_workspace.settings_open || app.drawing_workspace.options.is_some();
    app.drawing_workspace.settings_open = false;
    app.drawing_workspace.options = None;
    app.drawing_workspace.settings = None;
    app.drawing_workspace.form = None;
    open
}

/// Toolbar clicks: Update Drawing runs; Base View and Export open their panels; planned tools
/// show their agreed options. This never executes a missing CAD command.
pub fn start(app: &mut SolveApp, id: &str) -> bool {
    if !active(app) {
        return false;
    }
    if id == "drawing.update" {
        if let Err(e) = app.run("drawing.update", json!({})) {
            app.set_status(e, true);
        }
        return true;
    }
    if !can_open_options(id) {
        return false;
    }
    app.drawing_workspace.options = match id {
        "drawing.view.base" => Some("Base View"),
        "drawing.view.projected" => Some("Projected View"),
        "drawing.dimension" => Some("Dimension"),
        "drawing.note" => Some("Note"),
        "drawing.export" => Some("Export Drawing"),
        _ => None,
    };
    app.drawing_workspace.form = Some(Form::of(app));
    app.drawing_workspace.settings_open = false;
    true
}

pub(crate) fn covered_rects(app: &SolveApp) -> Vec<Rect> {
    if !active(app) {
        return Vec::new();
    }
    [app.drawing_workspace.strip, app.drawing_workspace.settings].into_iter().flatten().collect()
}

pub(crate) fn settings_rect(app: &SolveApp) -> Option<Rect> {
    active(app).then_some(app.drawing_workspace.settings).flatten()
}

fn panel_width(app: &SolveApp) -> f32 {
    if app.preferences.drawing_panel_width.is_finite() { app.preferences.drawing_panel_width.clamp(240.0, 560.0) } else { 320.0 }
}

pub(crate) fn left_inset(app: &SolveApp) -> f32 {
    if active(app)
        && Container::for_layout(app.ui.layout, Surface::Panel).floating()
        && (app.drawing_workspace.settings_open || app.drawing_workspace.options.is_some())
        && app.preferences.drawing_panel_side == "left"
    {
        panel_width(app) + crate::shell::SHELL_GAP
    } else {
        0.0
    }
}

/// The view's name as people read it.
fn view_label(name: &str) -> &'static str {
    match name {
        "front" => "Front view",
        "top" => "Top view",
        "left" => "Left view",
        "right" => "Right view",
        _ => "Isometric view",
    }
}

/// Reuse Browser's existing content-hugging host and resize seam, with Drawing contents: the
/// sheet, its views and its dimensions, then the source design.
pub(crate) fn browser_contents(app: &mut SolveApp, ui: &mut egui::Ui) {
    match crate::drawing_window::sheet(app) {
        Some(sh) => {
            ui_kit::text(ui, &sheet_label(app), ui_kit::Text::Heading("drawing_sheet"));
            for v in &sh.views {
                ui.label(crate::i18n::tr_label(view_label(&v.name)));
            }
            let dims = sh.dims.len().to_string();
            ui_kit::text(ui, &crate::i18n::format("ui.drawing_dimension_count", &[("count", &dims)]), ui_kit::Text::Note("drawing_dims"));
            for w in &sh.warnings {
                ui.label(egui::RichText::new(w).color(Tokens::get().error));
            }
        }
        None => {
            ui_kit::text(ui, crate::i18n::tr_label("No drawing objects"), ui_kit::Text::Note("drawing_objects"));
        }
    }
    ui.add_space(Tokens::get().space_8);
    ui_kit::text(ui, crate::i18n::tr_label("Source design"), ui_kit::Text::Heading("drawing_source"));
    ui.label(&app.session.doc.name);
    if ui_kit::button(ui, crate::i18n::tr_label("Source properties"), ui_kit::Button::Normal).clicked() {
        app.start("document.metadata");
    }
}

/// "Sheet 1 · A3": the sheet strip's and Browser's name for the drawing on show.
fn sheet_label(app: &SolveApp) -> String {
    match crate::drawing_window::sheet(app) {
        Some(sh) => crate::i18n::format("ui.drawing_sheet_label", &[("format", &sh.format), ("scale", &sh.scale_text)]),
        None => crate::i18n::tr_label("Sheet unavailable").to_string(),
    }
}

/// The canvas: the sheet as it prints, as large as the space between the panels allows; before
/// the first drawing, what to do to make one.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::drawing_window::follow(app);
    let rect = ui.max_rect();
    app.viewport.rect = Some(rect);
    let t = Tokens::get();
    ui.painter().rect_filled(rect, 0.0, t.viewport_top);
    let mut bounds = crate::shell::workspace_content(app, rect).shrink(t.space_24);
    // Clear of the sheet strip and the docked panels.
    for covered in covered_rects(app) {
        if covered.intersects(bounds) {
            if covered.center().y < bounds.center().y && covered.width() < bounds.width() * 0.9 {
                bounds.min.y = bounds.min.y.max(covered.bottom() + t.space_8);
            } else if covered.center().x > bounds.center().x {
                bounds.max.x = bounds.max.x.min(covered.left() - t.space_8);
            } else {
                bounds.min.x = bounds.min.x.max(covered.right() + t.space_8);
            }
        }
    }
    if let Some(sh) = crate::drawing_window::sheet(app)
        && bounds.is_positive()
    {
        let paper = crate::drawing_window::paint_fit(&ui.painter_at(rect), bounds, sh);
        crate::scenario::publish_rect("drawing:paper", paper);
        return;
    }
    let width = (bounds.width() - t.space_24 * 2.0).clamp(0.0, 420.0);
    let centre = Rect::from_center_size(bounds.center(), vec2(width, 160.0));
    let mut make = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(centre), |ui| {
        ui.vertical_centered(|ui| {
            ui.heading(crate::i18n::tr_label("Drawing"));
            ui.label(crate::i18n::tr_label("No drawing of this design yet."));
            ui_kit::text(
                ui,
                crate::i18n::tr_label("Base View lays out the design's views on a sheet, with its dimensions and title block."),
                ui_kit::Text::Note("drawing_waiting"),
            );
            make = ui_kit::button(ui, crate::i18n::tr_label("Make the drawing"), ui_kit::Button::Primary).clicked();
        });
    });
    if make && let Err(e) = app.run("drawing.view.base", json!({})) {
        app.set_status(e, true);
    }
}

fn dock(app: &SolveApp) -> &str {
    match app.preferences.drawing_sheet_dock.as_str() {
        "bottom" => "bottom",
        "under_browser" if app.ui.show_browser => "under_browser",
        _ => "top",
    }
}

fn strip_size(app: &SolveApp, ui: &egui::Ui, destination: &str) -> egui::Vec2 {
    let t = Tokens::get();
    let label = ui_kit::ContentWidth::button(ui, &sheet_label(app));
    let spacing = ui.spacing().item_spacing;
    let height = ui_kit::row_height().max(t.space_24);
    let measured = if destination == "under_browser" {
        vec2(label.max(t.space_10 + 3.0 * t.space_24 + 3.0 * spacing.x) + 10.0, height * 2.0 + spacing.y + 10.0)
    } else {
        vec2(label + t.space_10 + 3.0 * t.space_24 + 4.0 * spacing.x + 10.0, height + 10.0)
    };
    // Retain measured card dimensions; the font/control metrics cover a first frame or scale change.
    app.drawing_workspace.strip_sizes.get(usize::from(destination == "under_browser")).copied().flatten().map_or(measured, |size| size.max(measured))
}

fn needs_vertical(app: &SolveApp, available: Rect, width: f32) -> bool {
    app.ui.show_browser && app.shell.browser.is_some_and(|b| available.right() - b.right() - Tokens::get().space_8 < width)
}

fn strip_position(app: &SolveApp, available: Rect, destination: &str, size: egui::Vec2) -> egui::Pos2 {
    let gap = Tokens::get().space_8;
    let beside = app.shell.browser.map_or(available.left(), |b| b.right() + gap);
    let x = beside.clamp(available.left(), (available.right() - size.x).max(available.left()));
    match destination {
        "bottom" => pos2(x, (available.bottom() - size.y).max(available.top())),
        "under_browser" => app.shell.browser.map_or(available.min, |b| pos2(b.left(), b.bottom() + gap)),
        _ => pos2(x, available.top()),
    }
}

fn strip_target(app: &SolveApp, available: Rect, at: egui::Pos2) -> Option<&'static str> {
    if !available.contains(at) {
        return None;
    }
    let edge = Tokens::get().space_24 * 2.0;
    if at.y <= available.top() + edge {
        Some("top")
    } else if at.y >= available.bottom() - edge {
        Some("bottom")
    } else if app.ui.show_browser && app.shell.browser.is_some_and(|b| at.x <= b.right() && at.y >= b.top()) {
        Some("under_browser")
    } else {
        None
    }
}

/// Same grip and highlighted docking vocabulary as History; no document state changes.
fn strip_drag(app: &mut SolveApp, ui: &mut egui::Ui, available: Rect) {
    let grip = ui_kit::panel_grip(ui);
    let drag = ui.interact(grip.rect, ui.id().with("drawing_sheet_grip"), Sense::drag()).on_hover_text(crate::i18n::tr_label("Dock sheet tabs"));
    crate::scenario::publish_target("drawing:sheets:grip", drag.rect);
    if drag.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let target = ui.ctx().pointer_latest_pos().and_then(|p| strip_target(app, available, p));
    if drag.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(target) = target {
            let horizontal = strip_size(app, ui, "top");
            let target = if target != "under_browser" && needs_vertical(app, available, horizontal.x) { "under_browser" } else { target };
            let size = strip_size(app, ui, target);
            let zone = Rect::from_min_size(strip_position(app, available, target, size), size);
            let t = Tokens::get();
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("drawing_dock_preview")));
            painter.rect(zone, t.radius_lg, t.accent.gamma_multiply(0.18), Stroke::new(t.space_2, t.accent), egui::StrokeKind::Inside);
            crate::scenario::publish_rect("drawing:sheets:dock_preview", zone);
        }
    }
    if drag.drag_stopped()
        && let Some(target) = target
    {
        app.preferences.drawing_sheet_dock = target.into();
    }
    // Keyboard-accessible alternative to dragging, like History's layout menu.
    ui_kit::Menu::context(&drag, |ui| dock_menu(app, ui));
}

fn dock_menu(app: &mut SolveApp, ui: &mut egui::Ui) {
    for (value, label) in [("top", "Dock at top"), ("bottom", "Dock at bottom"), ("under_browser", "Dock beneath Browser")] {
        if ui_kit::button(ui, crate::i18n::tr_label(label), ui_kit::Button::Enabled(value != "under_browser" || app.ui.show_browser)).clicked() {
            app.preferences.drawing_sheet_dock = value.into();
            ui.close();
        }
    }
}

fn icon_action(ui: &mut egui::Ui, id: &str, icon: &str, label: &str, enabled: bool) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(Tokens::get().space_24, Tokens::get().space_24), Sense::hover());
    let r = ui.add_enabled_ui(enabled, |ui| ui_kit::icon_button_at(ui, rect, icon, label)).inner;
    crate::scenario::publish_target(id, rect);
    r
}

/// Draw after Browser, so the strip can hug its measured right or bottom edge.
pub fn chrome(app: &mut SolveApp, ui: &mut egui::Ui) {
    app.drawing_workspace.strip = None;
    if !active(app) {
        return;
    }
    let mut available = crate::agent_chat::tool_space(app, ui.available_rect_before_wrap());
    if Container::for_layout(app.ui.layout, Surface::Panel).floating()
        && app.preferences.drawing_panel_side != "left"
        && let Some(settings) = settings_rect(app)
    {
        available.max.x = available.right().min(settings.left() - Tokens::get().space_8).max(available.left());
    }
    if let Some(bottom) = app.shell.cards.iter().map(Rect::bottom).max_by(f32::total_cmp) {
        available.min.y = available.min.y.max(bottom + Tokens::get().space_8);
    }
    if !available.is_positive() {
        return;
    }
    let destination =
        if dock(app) != "under_browser" && needs_vertical(app, available, strip_size(app, ui, "top").x) { "under_browser" } else { dock(app) }
            .to_string();
    let position = strip_position(app, available, &destination, strip_size(app, ui, &destination));
    let (label, has_sheet) = (sheet_label(app), crate::drawing_window::sheet(app).is_some());
    let container = Container::new(ui.ctx(), Surface::Panel);
    let frame = container.frame(ui.ctx(), Tokens::get().panel, crate::frame::Side::Left).inner_margin(4);
    let card = container.area(egui::Id::new("sc_drawing_sheets")).fixed_pos(position).show(ui.ctx(), |ui| {
        container.show_frame(ui, frame, available, |ui| {
            let buttons = |app: &mut SolveApp, ui: &mut egui::Ui| {
                icon_action(ui, "drawing:sheets:add", "plus", "Add sheet (not implemented)", false);
                let management = icon_action(ui, "drawing:sheets:manage", "dots", "Manage sheets and docking", true);
                Container::new(ui.ctx(), Surface::Popup).menu(&management).frame(ui_kit::Menu::frame()).show(|ui| {
                    ui_kit::button(ui, crate::i18n::tr_label("Manage sheets (not implemented)"), ui_kit::Button::Enabled(false));
                    ui.separator();
                    dock_menu(app, ui);
                });
                if icon_action(ui, "drawing:sheets:settings", "settings", "Sheet Settings", true).clicked() {
                    app.drawing_workspace.settings_open = !app.drawing_workspace.settings_open;
                    app.drawing_workspace.options = None;
                    app.drawing_workspace.form = Some(Form::of(app));
                }
            };
            if destination == "under_browser" {
                ui.horizontal(|ui| {
                    strip_drag(app, ui, available);
                    buttons(app, ui);
                });
                ui_kit::button(ui, &label, ui_kit::Button::Enabled(has_sheet));
            } else {
                ui.horizontal(|ui| {
                    strip_drag(app, ui, available);
                    ui_kit::button(ui, &label, ui_kit::Button::Enabled(has_sheet));
                    buttons(app, ui);
                });
            }
        });
    });
    app.drawing_workspace.strip = Some(card.response.rect);
    if let Some(size) = app.drawing_workspace.strip_sizes.get_mut(usize::from(destination == "under_browser")) {
        *size = Some(card.response.rect.size());
    }
    crate::scenario::publish_rect("drawing:sheets", card.response.rect);
}

pub fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    app.drawing_workspace.settings = None;
    if !active(app) || (!app.drawing_workspace.settings_open && app.drawing_workspace.options.is_none()) {
        return;
    }
    let left = app.preferences.drawing_panel_side == "left";
    let width = panel_width(app);
    let container = Container::new(ui.ctx(), Surface::Panel);
    if container.floating() {
        floating_panel(app, ui, left, width);
        return;
    }
    let r = container
        .edge(if left { "sc_drawing_settings_left" } else { "sc_drawing_settings_right" }, if left { Edge::Left } else { Edge::Right })
        .default_size(width)
        .size_range(240.0..=560.0)
        .resizable(true)
        .frame(container.edge_frame(Tokens::get().dialog_body).inner_margin(10))
        .show(ui, |ui| {
            ui_kit::scope(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| panel_body(app, ui));
            });
        });
    app.drawing_workspace.settings = Some(r.response.rect);
    app.preferences.drawing_panel_width = r.response.rect.width().clamp(240.0, 560.0);
    crate::scenario::publish_rect("drawing:settings", r.response.rect);
    let rect = r.response.rect;
    let rail = Rect::from_center_size(if left { rect.right_center() } else { rect.left_center() }, vec2(Tokens::get().space_4, rect.height()));
    crate::scenario::publish_target("drawing:settings:resize", rail);
}

/// Assistant's floating panel host and bounded resize rail; no independent window chrome.
fn floating_panel(app: &mut SolveApp, ui: &mut egui::Ui, left: bool, width: f32) {
    let ctx = ui.ctx().clone();
    let t = Tokens::get();
    let available = crate::agent_chat::tool_space(app, ui.available_rect_before_wrap());
    let mut bounds = available.shrink(crate::shell::SHELL_GAP);
    bounds.min.y = available.top();
    let width = width.min(bounds.width().max(0.0));
    let position = pos2(if left { bounds.left() } else { bounds.right() - width }, bounds.top());
    bounds = Rect::from_min_size(position, vec2(width, bounds.height().max(0.0)));
    if !bounds.is_positive() {
        return;
    }
    let side = if left { crate::frame::Side::Left } else { crate::frame::Side::Right };
    let frame = ui_kit::Panel::feature_frame(&ctx, app.ui.layout, side);
    let inner = (bounds.size() - frame.total_margin().sum()).max(egui::Vec2::ZERO);
    let anchor = if left { bounds.left_top() } else { bounds.right_top() };
    let panel =
        ui_kit::Panel::new(crate::i18n::tr_label("Sheet Settings"), egui::Id::new("sc_drawing_settings_floating"), anchor, available, side, frame)
            .width(width);
    if let Some(card) = panel.window(&ctx).fixed_size(inner).show(&ctx, |ui| {
        ui.set_min_size(inner);
        ui.set_max_size(inner);
        ui.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_size(ui.max_rect().min, inner)));
        ui_kit::scope(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| panel_body(app, ui));
        });
        let rect = ui.max_rect();
        let rail = if left {
            Rect::from_x_y_ranges(rect.right() - t.space_4..=rect.right(), rect.y_range())
        } else {
            Rect::from_x_y_ranges(rect.left()..=rect.left() + t.space_4, rect.y_range())
        };
        let response =
            ui.interact(rail, egui::Id::new("sc_drawing_settings_resize"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        crate::scenario::publish_target("drawing:settings:resize", rail);
        if response.dragged() {
            let delta = ui.input(|i| i.pointer.delta().x) * if left { 1.0 } else { -1.0 };
            if delta.is_finite() {
                app.preferences.drawing_panel_width = (width + delta).clamp(240.0, 560.0);
            }
        }
    }) {
        app.drawing_workspace.settings = Some(card.response.rect);
        crate::scenario::publish_rect("drawing:settings", card.response.rect);
    }
}

fn unavailable_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui_kit::row(ui, crate::i18n::tr_label(label), |ui| {
        ui.add_enabled_ui(false, |ui| {
            if matches!(
                label,
                "Source"
                    | "Orientation"
                    | "Direction"
                    | "Parent view"
                    | "Text style"
                    | "Profile & units"
                    | "Size & orientation"
                    | "Border & template"
            ) {
                ui_kit::dropdown_menu(ui, label, crate::i18n::tr_label(value), |_| {});
            } else {
                let mut text = crate::i18n::tr_label(value).to_string();
                ui_kit::cell_editor(ui, ui.id().with(label), &mut text, "", false);
            }
        });
    });
}

fn panel_body(app: &mut SolveApp, ui: &mut egui::Ui) {
    let options = app.drawing_workspace.options;
    ui.horizontal(|ui| {
        ui.heading(crate::i18n::tr_label(options.unwrap_or("Sheet Settings")));
        if icon_action(ui, "drawing:settings:close", "close", "Close drawing panel", true).clicked() {
            app.drawing_workspace.settings_open = false;
            app.drawing_workspace.options = None;
        }
    });
    ui.horizontal(|ui| {
        for (side, label) in [("left", "Dock left"), ("right", "Dock right")] {
            if ui_kit::button(ui, crate::i18n::tr_label(label), ui_kit::Button::Normal).clicked() {
                app.preferences.drawing_panel_side = side.into();
            }
        }
    });
    ui.separator();
    match options {
        None | Some("Base View") => {
            let mut form = app.drawing_workspace.form.take().unwrap_or_else(|| Form::of(app));
            if options.is_some() {
                views_rows(ui, &mut form);
            } else {
                sheet_rows(ui, &mut form);
            }
            ui.separator();
            let apply = ui_kit::button(ui, crate::i18n::tr_label("Apply"), ui_kit::Button::Primary).clicked();
            crate::scenario::publish_target("drawing:settings:apply", ui.min_rect());
            if apply {
                match app.run("drawing.view.base", form.params()) {
                    Ok(_) => crate::drawing_window::follow(app),
                    Err(e) => app.set_status(e, true),
                }
            }
            app.drawing_workspace.form = Some(form);
        }
        Some("Export Drawing") => export_rows(app, ui),
        Some(kind) => {
            ui_kit::text(
                ui,
                crate::i18n::tr_label("Not implemented. These options describe the planned interface; no drawing data is changed."),
                ui_kit::Text::Note("drawing_options_pending"),
            );
            match kind {
                "Projected View" => {
                    unavailable_row(ui, "Parent view", "No drawing view available");
                    unavailable_row(ui, "Direction", "Not configured");
                    unavailable_row(ui, "Scale", "Inherited from parent view");
                }
                "Dimension" => {
                    unavailable_row(ui, "References", "No drawing geometry available");
                    unavailable_row(ui, "Units & precision", "Drawing defaults (not available)");
                }
                "Note" => {
                    unavailable_row(ui, "Text", "Not configured");
                    unavailable_row(ui, "Text style", "Drawing defaults (not available)");
                }
                _ => {}
            }
            ui.separator();
            ui_kit::button(ui, crate::i18n::tr_label("Apply (not implemented)"), ui_kit::Button::Enabled(false));
        }
    }
}

/// A dropdown row over `options`, by index.
fn choice_row(ui: &mut egui::Ui, label: &str, selected: &mut usize, options: &[&str]) {
    ui_kit::row(ui, crate::i18n::tr_label(label), |ui| {
        ui_kit::dropdown(ui, &format!("drawing_{label}"), selected, options);
    });
}

/// Sheet Settings: the title block, the paper and the drafting defaults of the drawing.
fn sheet_rows(ui: &mut egui::Ui, form: &mut Form) {
    ui.heading(crate::i18n::tr_label("Drawing & title block"));
    for ((key, label), value) in TITLE_FIELDS.iter().zip(form.title.iter_mut()) {
        ui_kit::row(ui, crate::i18n::tr_label(label), |ui| {
            ui_kit::cell_editor(ui, egui::Id::new(("drawing_title", *key)), value, "", false);
        });
    }
    ui_kit::text(ui, crate::i18n::tr_label("Material and mass come from the design when left empty."), ui_kit::Text::Note("drawing_title_note"));
    ui.heading(crate::i18n::tr_label("Paper & layout"));
    choice_row(ui, "Paper size", &mut form.format, &FORMATS);
    let mut orientation = usize::from(form.portrait);
    choice_row(ui, "Orientation", &mut orientation, &["Landscape", "Portrait"]);
    form.portrait = orientation == 1;
    choice_row(ui, "Scale", &mut form.scale, &SCALES);
    ui.heading(crate::i18n::tr_label("Drafting defaults"));
    let mut projection = usize::from(form.third_angle);
    choice_row(ui, "Projection", &mut projection, &["First angle (ISO E)", "Third angle (ISO A)"]);
    form.third_angle = projection == 1;
    ui_kit::checkbox(ui, crate::i18n::tr_label("Hidden lines, dashed"), &mut form.hidden);
}

/// Base View: which standard views the sheet shows, their projection and scale.
fn views_rows(ui: &mut egui::Ui, form: &mut Form) {
    ui_kit::row(ui, crate::i18n::tr_label("Source"), |ui| {
        ui.label(crate::i18n::tr_label("Source design"));
    });
    let side = if form.third_angle { "Right view" } else { "Left view" };
    for (on, label) in form.views.iter_mut().zip(["Front view", "Top view", side, "Isometric view"]) {
        ui_kit::checkbox(ui, crate::i18n::tr_label(label), on);
    }
    let mut projection = usize::from(form.third_angle);
    choice_row(ui, "Projection", &mut projection, &["First angle (ISO E)", "Third angle (ISO A)"]);
    form.third_angle = projection == 1;
    choice_row(ui, "Scale", &mut form.scale, &SCALES);
    ui_kit::text(
        ui,
        crate::i18n::tr_label("Overall sizes and hole callouts are dimensioned with the views."),
        ui_kit::Text::Note("drawing_views_note"),
    );
}

/// Export: the sheet as PDF (to print), SVG or DXF (layered, for a CAD or a cutting machine).
fn export_rows(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ready = crate::drawing_window::sheet(app).is_some();
    if !ready {
        ui_kit::text(ui, crate::i18n::tr_label("Make the drawing first (Layout, Base View)."), ui_kit::Text::Note("drawing_export_none"));
    }
    for (ext, label, note) in [
        ("pdf", "Export PDF", "To print or send: one page, at the sheet's size."),
        ("svg", "Export SVG", "For documents and the web, at true size."),
        ("dxf", "Export DXF", "For a CAD program: views, hidden lines, axes, dimensions and title block on their own layers."),
    ] {
        if ui_kit::button(ui, crate::i18n::tr_label(label), ui_kit::Button::Enabled(ready)).clicked() {
            crate::drawing_window::save(app, ext);
        }
        ui_kit::text(ui, crate::i18n::tr_label(note), ui_kit::Text::Note(ext));
    }
    if let Some((m, error)) = crate::drawing_window::message(app) {
        let t = Tokens::get();
        ui.label(egui::RichText::new(m).color(if *error { t.error } else { t.text_dim }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    #[test]
    fn workspace_and_options_leave_source_and_camera_unchanged() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length":40,"width":20,"height":3})).unwrap();
        h.app.ui.tab = "SURFACE".into();
        let doc = h.app.session.doc.clone();
        let revision = h.app.session.revision;
        let undo = h.app.session.undo.len();
        let cam = h.app.cam;
        enter(&mut h.app, true).unwrap();
        assert!(start(&mut h.app, "drawing.view.base"));
        h.frames(2);
        assert!(h.widget("Apply").is_some());
        assert!(crate::scenario::rect_of("drawing:paper").is_none(), "no sheet before Apply");
        // Apply makes the sheet: shown on the canvas, the design and its history untouched.
        h.step(&serde_json::json!({"widget":"Apply"})).unwrap();
        h.frames(3);
        assert!(h.app.session.drawing.is_some());
        assert!(crate::scenario::rect_of("drawing:paper").is_some_and(|r| r.width() > 200.0), "the sheet fills the canvas");
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.cam, cam);
        let _ = revision;
        enter(&mut h.app, false).unwrap();
        assert_eq!(h.app.ui.tab, "SURFACE");
    }

    #[test]
    fn sheet_docking_and_settings_only_change_ui_preferences() {
        for layout in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.ui.layout = layout;
            h.call("ui.resize", serde_json::json!({"width":1400,"height":1000}));
            enter(&mut h.app, true).unwrap();
            let revision = h.app.session.revision;
            let doc = h.app.session.doc.clone();
            let cam = h.app.cam;
            h.frames(4);
            let strip = crate::scenario::rect_of("drawing:sheets").unwrap();
            let browser = crate::scenario::rect_of("browser:card").unwrap();
            assert!(strip.width() < 340.0 && strip.height() < 50.0, "single sheet strip should hug its contents: {strip:?}");
            assert!(strip.left() >= browser.right(), "sheet strip belongs beside Browser");
            for (destination, at) in [
                ("bottom", pos2(700.0, 980.0)),
                ("under_browser", pos2(browser.center().x, browser.bottom() + 80.0)),
                ("top", pos2(700.0, strip.center().y)),
            ] {
                let grip = crate::scenario::handle_at("drawing:sheets:grip").unwrap();
                h.app.synthetic.push(egui::Event::PointerMoved(grip));
                h.frame();
                h.app.synthetic.push(egui::Event::PointerButton {
                    pos: grip,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
                h.frame();
                h.app.synthetic.push(egui::Event::PointerMoved(at));
                h.frames(2);
                assert!(crate::scenario::rect_of("drawing:sheets:dock_preview").is_some(), "{layout:?} {destination}: highlighted docking preview");
                h.app.synthetic.push(egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
                h.frames(4);
                assert_eq!(h.app.preferences.drawing_sheet_dock, destination);
            }
            h.step(&serde_json::json!({"click":{"handle":"drawing:sheets:settings"}})).unwrap();
            h.frames(3);
            for (side, label) in [("left", "Dock left"), ("right", "Dock right")] {
                h.step(&serde_json::json!({"widget":label})).unwrap();
                h.frames(3);
                assert_eq!(h.app.preferences.drawing_panel_side, side);
                let panel = crate::scenario::rect_of("drawing:settings").unwrap();
                assert_eq!(panel.center().x < 700.0, side == "left", "{layout:?} {side}: {panel:?}");
                let rail = crate::scenario::handle_at("drawing:settings:resize").unwrap();
                h.call(
                    "ui.drag",
                    serde_json::json!({"x0":rail.x,"y0":rail.y,"x1":rail.x + if side == "left" {50.0} else {-50.0},"y1":rail.y,"steps":6}),
                );
                h.frames(3);
                assert!(app_width(&h) > panel.width() + 20.0, "{layout:?} {side}: properties resize");
            }
            assert_eq!(h.app.session.revision, revision);
            assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
            assert_eq!(h.app.cam, cam);
        }
        assert!(can_open_options("drawing.export"), "Export opens its formats");
        assert!(!can_open_options("drawing.update"), "Update runs at once");
    }

    fn app_width(h: &Harness) -> f32 {
        h.app.preferences.drawing_panel_width
    }

    #[test]
    fn crowded_sheet_strip_moves_beneath_browser_without_changing_its_dock_preference() {
        for layout in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.ui.layout = layout;
            h.call("ui.resize", serde_json::json!({"width":960,"height":600}));
            h.app.preferences.drawing_panel_width = 560.0;
            enter(&mut h.app, true).unwrap();
            h.app.drawing_workspace.settings_open = true;
            h.frames(4);
            let browser = crate::scenario::rect_of("browser:card").unwrap();
            let strip = crate::scenario::rect_of("drawing:sheets").unwrap();
            assert!(strip.top() >= browser.bottom(), "{layout:?}: crowded sheet strip must not cover Browser: {strip:?} {browser:?}");
            assert_eq!(h.app.preferences.drawing_sheet_dock, "top");
            h.step(&serde_json::json!({"click":{"handle":"drawing:settings:close"}})).unwrap();
            h.frames(4);
            let browser = crate::scenario::rect_of("browser:card").unwrap();
            let strip = crate::scenario::rect_of("drawing:sheets").unwrap();
            assert!(
                strip.left() >= browser.right() && strip.height() < 50.0,
                "{layout:?}: preferred horizontal strip returns when room is available"
            );
            assert_eq!(h.app.preferences.drawing_sheet_dock, "top");
        }
    }
}
