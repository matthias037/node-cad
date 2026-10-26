//! The Mise en plan window: the sheet `drawing.create` made (by a click, a script or NØDE IA),
//! drawn as it prints, with its exports. It opens by itself when a new drawing is made.

use egui::{Color32, Pos2, Sense, Stroke, Vec2};
use serde_json::json;
use solidcraft_engine::cmd::{DrawingMark, DrawingSheet, drawing_dxf, drawing_marks, drawing_pdf, drawing_svg};

use crate::SolveApp;

#[derive(Default)]
pub struct State {
    pub open: bool,
    /// The drawing last shown (`Session::drawing_seq`).
    seen: u64,
    sheet: Option<DrawingSheet>,
    message: Option<(String, bool)>,
}

/// Paper and ink: a drawing is shown as it prints, black on white, in either theme.
const PAPER: Color32 = Color32::WHITE;
const INK: Color32 = Color32::BLACK;

/// Open the window on a new drawing.
fn follow(app: &mut SolveApp) {
    let seq = app.session.drawing_seq;
    if seq != app.drawing_window.seen {
        app.drawing_window.seen = seq;
        app.drawing_window.sheet = app.session.drawing.clone().and_then(|v| serde_json::from_value(v).ok());
        app.drawing_window.open = app.drawing_window.sheet.is_some();
        app.drawing_window.message = None;
    }
}

fn paint(ui: &mut egui::Ui, sh: &DrawingSheet) {
    // The whole sheet in view: as wide as the window, as tall as the screen leaves room for.
    let room = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 96.0).max(200.0);
    let width = ui.available_width().min(room * (sh.width / sh.height) as f32).clamp(320.0, 1400.0);
    let k = width / sh.width as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, sh.height as f32 * k), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, PAPER);
    let at = |x: f64, y: f64| Pos2::new(rect.left() + x as f32 * k, rect.bottom() - y as f32 * k);
    let px = |mm: f64| (mm as f32 * k).max(0.6);
    for mark in drawing_marks(sh) {
        match mark {
            DrawingMark::Line(kind, [ax, ay, bx, by]) => {
                let (a, b) = (at(ax, ay), at(bx, by));
                match kind {
                    "hidden" => {
                        painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(px(0.25), INK), px(2.0), px(1.0)));
                    }
                    "axis" => {
                        painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(px(0.18), Color32::from_gray(90)), px(6.0), px(1.5)));
                    }
                    _ => {
                        let w = match kind {
                            "frame" => 0.7,
                            "visible" => 0.5,
                            _ => 0.18,
                        };
                        painter.line_segment([a, b], Stroke::new(px(w), INK));
                    }
                }
            }
            DrawingMark::Arrow(tip, dir) => {
                let len = dir[0].hypot(dir[1]).max(1e-9);
                let (ux, uy) = (dir[0] / len, dir[1] / len);
                let p = |l: f64, w: f64| at(tip[0] - ux * l - uy * w, tip[1] - uy * l + ux * w);
                painter.add(egui::Shape::convex_polygon(vec![at(tip[0], tip[1]), p(3.0, 0.5), p(3.0, -0.5)], INK, Stroke::NONE));
            }
            DrawingMark::Text(pos, size, angle, text) => {
                let galley = painter.layout_no_wrap(text, egui::FontId::proportional((size as f32 * k).max(4.0)), INK);
                let a = at(pos[0], pos[1]);
                let r = -(angle as f32).to_radians();
                // Centred on its baseline point, turned with it.
                let half = galley.size() * 0.5;
                let (s, c) = r.sin_cos();
                let off = Vec2::new(-half.x * c + galley.size().y * s, -half.x * s - galley.size().y * c);
                painter.add(egui::epaint::TextShape::new(a + off, galley, INK).with_angle(r));
            }
        }
    }
}

/// Save the drawing in a format: a download in the browser, a file the user picks elsewhere.
fn save(app: &mut SolveApp, ext: &'static str) {
    let Some(sh) = app.drawing_window.sheet.clone() else { return };
    let bytes = match ext {
        "pdf" => drawing_pdf(&sh),
        "svg" => drawing_svg(&sh).into_bytes(),
        _ => drawing_dxf(&sh).into_bytes(),
    };
    let base = if sh.title.titre.trim().is_empty() { "plan".to_string() } else { sh.title.titre.trim().replace(['/', '\\', ':'], "-") };
    let name = format!("{base}.{ext}");
    if let Some(download) = &app.services.download_design {
        app.drawing_window.message = Some(match download(&name, &bytes) {
            Ok(()) => (crate::i18n::format("ui.drawing_saved", &[("name", &name)]), false),
            Err(e) => (e, true),
        });
        return;
    }
    app.pick_save(&name, &[ext], move |app, path| {
        if let Some(path) = path {
            app.drawing_window.message = Some(match app.run("drawing.export", json!({"format": ext, "path": path})) {
                Ok(_) => (crate::i18n::format("ui.drawing_saved", &[("name", &path)]), false),
                Err(e) => (e, true),
            });
        }
    });
}

/// The File menu's Mise en plan: the drawing already made, or a new one of the design.
pub fn make(app: &mut SolveApp) {
    if app.drawing_window.sheet.is_some() && app.session.drawing.is_some() {
        app.drawing_window.open = true;
        return;
    }
    if let Err(e) = app.run("drawing.create", json!({})) {
        app.drawing_window.message = Some((e, true));
    }
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    follow(app);
    if !app.drawing_window.open {
        return;
    }
    let Some(sh) = app.drawing_window.sheet.clone() else { return };
    let mut export: Option<&'static str> = None;
    let mut redo = false;
    let title = crate::i18n::tr_label("Drawing");
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_drawing"), "export", title).cards(4.0).show_body(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::format(
                "ui.drawing_summary",
                &[("format", &sh.format), ("scale", &sh.scale_text), ("views", &sh.views.len().to_string())],
            ));
        });
        for w in &sh.warnings {
            ui.label(egui::RichText::new(w).color(crate::theme::Tokens::get().error));
        }
        ui.horizontal(|ui| {
            for (ext, label) in [("pdf", "Export PDF"), ("svg", "Export SVG"), ("dxf", "Export DXF")] {
                if crate::ui_kit::button(ui, crate::i18n::tr_label(label), crate::ui_kit::Button::Normal).clicked() {
                    export = Some(ext);
                }
            }
            if crate::ui_kit::button(ui, crate::i18n::tr_label("Update drawing"), crate::ui_kit::Button::Normal).clicked() {
                redo = true;
            }
        });
        if let Some((m, error)) = &app.drawing_window.message {
            let t = crate::theme::Tokens::get();
            ui.label(egui::RichText::new(m).color(if *error { t.error } else { t.text_dim }));
        }
        paint(ui, &sh);
    });
    if closed {
        app.drawing_window.open = false;
    }
    if let Some(ext) = export {
        save(app, ext);
    }
    if redo {
        // The same sheet again, from the design as it is now.
        let p = json!({"format": sh.format, "projection": sh.projection, "title": sh.title});
        if let Err(e) = app.run("drawing.create", p) {
            app.drawing_window.message = Some((e, true));
        }
    }
}
