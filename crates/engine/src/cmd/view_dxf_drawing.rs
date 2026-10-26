//! Mise en plan (a technical drawing of the design): an ISO 5457 sheet (A4 to A0, its frame
//! and centring marks) with an ISO 7200-style title block, the part's standard views projected
//! by the same hidden-line pipeline as `file.export_view_dxf` (visible edges, hidden edges
//! dashed), arranged in first-angle projection (ISO E, the European default) or third-angle
//! projection at a standard scale, with the holes' centre lines, the overall dimensions and
//! hole callouts ("4× Ø6,6") placed automatically. `drawing.create` makes it and keeps it in
//! the session; `drawing.export` writes it as SVG, DXF or PDF.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solidcraft_geom::{Aabb3, Vec2, Vec3};
use solidcraft_io::{DxfEntity, DxfLayer, DxfLinetype, DxfShape};
use solidcraft_render::{Camera, StandardView};

use super::super::CommandSpec;
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.create", "Create Drawing", create).icon("export").noundo().params(
        "format?: A4|A3|A2|A1|A0 (default: the smallest that fits at 1:1, A3 at most unless asked); orientation?: landscape|portrait \
         (default landscape); projection?: first_angle|third_angle (default first_angle, ISO E); views?: [front, top, left|right, iso] \
         (default all four); scale?: \"1:2\" (default: the largest standard scale that fits); hidden?: dashed|omit (default dashed); \
         dimensions?: true (overall sizes and hole callouts); title?: {titre, numero, matiere, masse, auteur, date, tolerance} \
         (the title block; tolerance default \"ISO 2768-m\"). Projects the visible bodies, keeps the drawing in the session \
         (drawing.export writes it) and returns the sheet: format, scale, views with their line counts, dimensions and warnings",
    ),
    CommandSpec::new("drawing.export", "Export Drawing", export).icon("export").noundo().params(
        "format: svg|dxf|pdf; path?: write it there (else the text is returned: svg and dxf as text, pdf as base64). Exports the \
         drawing drawing.create made",
    ),
    CommandSpec::new("drawing.get", "Show Drawing", get)
        .icon("export")
        .noundo()
        .params("No parameters. The drawing drawing.create made (the sheet with its views, dimensions and title block), or null"),
];

/// A drawing sheet in millimetres, origin at the bottom left, Y up (as on paper and in DXF).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Sheet {
    pub format: String,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub scale_text: String,
    pub projection: String,
    pub views: Vec<View>,
    pub dims: Vec<Dim>,
    pub title: Title,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct View {
    pub name: String,
    pub center: [f64; 2],
    pub visible: Vec<[f64; 4]>,
    pub hidden: Vec<[f64; 4]>,
    pub axes: Vec<[f64; 4]>,
}

/// A dimension: its dimension line (with arrows at both ends), extension lines and text.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Dim {
    pub kind: String,
    pub line: [f64; 4],
    pub ext: Vec<[f64; 4]>,
    /// Whether the dimension line ends in arrows at both ends (a leader has one).
    pub arrows: u8,
    pub text: String,
    pub text_at: [f64; 2],
    pub text_angle: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Title {
    pub titre: String,
    pub numero: String,
    pub matiere: String,
    pub masse: String,
    pub auteur: String,
    pub date: String,
    pub tolerance: String,
}

/// Paper sizes (landscape), ISO 216.
const FORMATS: [(&str, f64, f64); 5] =
    [("A4", 297.0, 210.0), ("A3", 420.0, 297.0), ("A2", 594.0, 420.0), ("A1", 841.0, 594.0), ("A0", 1189.0, 841.0)];
/// ISO 5457: a 20 mm filing margin on the left, 10 mm elsewhere.
const MARGIN_LEFT: f64 = 20.0;
const MARGIN: f64 = 10.0;
/// The title block, ISO 7200: 180 mm wide at the bottom right of the frame.
const TITLE_W: f64 = 180.0;
const TITLE_H: f64 = 40.0;
/// Room around each view for its dimensions.
const GAP: f64 = 22.0;
/// Standard scales, ISO 5455, largest first.
const SCALES: [f64; 10] = [10.0, 5.0, 2.0, 1.0, 0.5, 0.2, 0.1, 0.05, 0.02, 0.01];
/// Text height for dimensions and the title block (ISO 3098: 3.5 mm).
const TEXT: f64 = 3.5;

/// A number as a French drawing writes it: a decimal comma, no trailing zeros.
pub fn mm(x: f64) -> String {
    let s = format!("{:.2}", (x * 100.0).round() / 100.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.replace('.', ",")
}

fn scale_text(s: f64) -> String {
    if s >= 1.0 { format!("{}:1", mm(s)) } else { format!("1:{}", mm(1.0 / s)) }
}

fn parse_scale(t: &str) -> Option<f64> {
    let (a, b) = t.split_once(':')?;
    let (a, b): (f64, f64) = (a.trim().replace(',', ".").parse().ok()?, b.trim().replace(',', ".").parse().ok()?);
    (a > 0.0 && b > 0.0 && (a / b).is_finite()).then_some(a / b)
}

/// The camera that draws a standard view, orthographic and centred on the part.
fn camera(view: StandardView, center: Vec3) -> Camera {
    let mut cam = Camera { fov: 0.0, ..Camera::default() };
    cam.set_view(view);
    cam.target = center;
    cam.distance = 1000.0;
    cam
}

struct Projected {
    view: StandardView,
    name: &'static str,
    lines: Vec<([f64; 2], [f64; 2], bool)>,
    lo: [f64; 2],
    hi: [f64; 2],
}

fn project(meshes: &super::meshes::Scene, view: StandardView, name: &'static str, center: Vec3) -> std::result::Result<Projected, &'static str> {
    let drawing = super::projection::drawing(meshes, &[], &camera(view, center))?;
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let lines: Vec<_> = drawing
        .lines
        .into_iter()
        .map(|l| {
            for p in [l.a, l.b] {
                lo = [lo[0].min(p.x), lo[1].min(p.y)];
                hi = [hi[0].max(p.x), hi[1].max(p.y)];
            }
            ([l.a.x, l.a.y], [l.b.x, l.b.y], l.hidden)
        })
        .collect();
    if lines.is_empty() {
        return Err("nothing to draw");
    }
    Ok(Projected { view, name, lines, lo, hi })
}

/// A hole: a round internal face whose axis runs along a view (seen as a circle there).
struct Hole {
    axis: Vec3,
    center: Vec3,
    diameter: f64,
    /// Where its faces' points sit round the axis (radians), to tell a hole from a rounded
    /// corner: a pocket's R5 corners are internal cylinders too, a quarter turn each.
    angles: Vec<f64>,
}

impl Hole {
    /// Whether the faces go all the way round: no gap wider than 45° between their points.
    fn round(&self) -> bool {
        let mut a = self.angles.clone();
        if a.len() < 3 {
            return false;
        }
        a.sort_by(f64::total_cmp);
        let wrap = a[0] + std::f64::consts::TAU - a[a.len() - 1];
        let gap = a.windows(2).map(|w| w[1] - w[0]).fold(wrap, f64::max);
        gap < std::f64::consts::FRAC_PI_4
    }
}

fn holes(s: &Session) -> Vec<Hole> {
    let st = s.world_state();
    let mut out: Vec<Hole> = Vec::new();
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh()).take(500) {
        let mesh = b.mesh();
        for i in 0..b.body.face_count().min(20_000) {
            let Some(c) = solidcraft_kernel::cylinder_face(&b.body, &mesh, i).filter(|c| c.internal) else { continue };
            let axis = *c.axis;
            let center = c.axis_point;
            let diameter = 2.0 * c.radius;
            // Two directions across the axis, to read each point's angle round it.
            let across = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
            let (Some(u), Some(v)) = (axis.cross(across).normalized(), axis.cross(axis.cross(across)).normalized()) else { continue };
            let face = u32::try_from(i).unwrap_or(u32::MAX);
            let angles: Vec<f64> = mesh
                .triangles
                .iter()
                .zip(&mesh.tri_face)
                .filter(|(_, f)| **f == face)
                .flat_map(|(t, _)| t.iter())
                .filter_map(|k| mesh.positions.get(*k as usize))
                .map(|q| {
                    let d = *q - center;
                    d.dot(v).atan2(d.dot(u))
                })
                .collect();
            // A hole's two half faces, and a counterbore's seat, are one hole each.
            let same = out.iter_mut().find(|h| {
                let d = center - h.center;
                h.axis.dot(axis).abs() > 0.999 && (d - h.axis * d.dot(h.axis)).len() < 1e-3 && (h.diameter - diameter).abs() < 1e-3
            });
            match same {
                Some(h) => h.angles.extend(angles),
                None => out.push(Hole { axis, center, diameter, angles }),
            }
        }
    }
    out.retain(Hole::round);
    out
}

fn line(a: [f64; 2], b: [f64; 2]) -> [f64; 4] {
    [a[0], a[1], b[0], b[1]]
}

/// A horizontal or vertical dimension between two points of a view, `off` mm off the view.
fn linear(a: [f64; 2], b: [f64; 2], vertical: bool, at: f64, value: f64) -> Dim {
    if vertical {
        let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
        Dim {
            kind: "linear".into(),
            line: [at, y0, at, y1],
            ext: vec![line([a[0], a[1]], [at - 2.0, a[1]]), line([b[0], b[1]], [at - 2.0, b[1]])],
            arrows: 2,
            text: mm(value),
            text_at: [at - 1.5, (y0 + y1) / 2.0],
            text_angle: 90.0,
        }
    } else {
        let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
        Dim {
            kind: "linear".into(),
            line: [x0, at, x1, at],
            ext: vec![line([a[0], a[1]], [a[0], at - 2.0]), line([b[0], b[1]], [b[0], at - 2.0])],
            arrows: 2,
            text: mm(value),
            text_at: [(x0 + x1) / 2.0, at + 1.5],
            text_angle: 0.0,
        }
    }
}

/// A library material as a French title block writes it (a design's own names as they are).
fn french_material(name: &str) -> String {
    match name {
        "Steel" => "Acier",
        "Stainless Steel" => "Acier inoxydable",
        "Aluminum" => "Aluminium",
        "Brass" => "Laiton",
        "Copper" => "Cuivre",
        "Titanium" => "Titane",
        "Cast Iron" => "Fonte",
        "ABS Plastic" => "ABS",
        "Oak" => "Chêne",
        "Glass" => "Verre",
        other => other,
    }
    .to_string()
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.create";
    if !super::sketch::exportable(s) {
        return Err(bad(cmd, "no visible bodies to draw"));
    }
    let landscape = match str_(p, "orientation").unwrap_or("landscape") {
        "landscape" => true,
        "portrait" => false,
        _ => return Err(bad(cmd, "orientation must be landscape or portrait")),
    };
    let first_angle = match str_(p, "projection").unwrap_or("first_angle") {
        "first_angle" => true,
        "third_angle" => false,
        _ => return Err(bad(cmd, "projection must be first_angle or third_angle")),
    };
    let dashed = match str_(p, "hidden").unwrap_or("dashed") {
        "dashed" => true,
        "omit" => false,
        _ => return Err(bad(cmd, "hidden must be dashed or omit")),
    };
    let side = if first_angle { "left" } else { "right" };
    let wanted: Vec<String> = match p.get("views").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        None => vec!["front".into(), "top".into(), side.into(), "iso".into()],
    };
    for v in &wanted {
        if !matches!(v.as_str(), "front" | "top" | "left" | "right" | "iso") {
            return Err(bad(cmd, format!("unknown view `{v}` (front, top, left, right, iso)")));
        }
    }
    let meshes = super::meshes::prepare(s, 0.01)?;
    let mut bounds = Aabb3::EMPTY;
    for b in s.world_state().bodies.iter().filter(|b| !b.body.is_mesh()) {
        bounds = bounds.union(&b.mesh().bounds());
    }
    let center = bounds.center();
    let has = |n: &str| wanted.iter().any(|v| v == n);
    let mut proj = Vec::new();
    for (key, view, name) in [
        ("front", StandardView::Front, "front"),
        ("top", StandardView::Top, "top"),
        ("left", StandardView::Left, "left"),
        ("right", StandardView::Right, "right"),
        ("iso", StandardView::Iso, "iso"),
    ] {
        if has(key) {
            proj.push(project(&meshes, view, name, center).map_err(|e| bad(cmd, e))?);
        }
    }
    let size = |name: &str| proj.iter().find(|v| v.name == name).map(|v| [v.hi[0] - v.lo[0], v.hi[1] - v.lo[1]]).unwrap_or([0.0, 0.0]);
    let front = size("front");
    let top = size("top");
    let lateral = size(side).max_by_side(size(if first_angle { "right" } else { "left" }));
    let iso = size("iso");
    // The grid: front and its side view on one row, the top view and the iso view on the other.
    let col1 = front[0].max(top[0]);
    let col2 = lateral[0].max(iso[0]);
    let row_front = front[1].max(lateral[1]);
    let row_top = top[1].max(iso[1]);
    let cols = [col1, col2].iter().filter(|w| **w > 0.0).count().max(1) as f64;
    let rows = [row_front, row_top].iter().filter(|h| **h > 0.0).count().max(1) as f64;
    let need =
        |s: f64, w: f64, h: f64| (col1 + col2) * s + GAP * (cols + 1.0) <= w && (row_front + row_top) * s + GAP * (rows + 1.0) <= h - TITLE_H * 0.5;
    let fits = |s: f64, (w, h): (f64, f64)| need(s, w - MARGIN_LEFT - MARGIN, h - 2.0 * MARGIN);
    let asked_scale = match p.get("scale") {
        None => None,
        Some(Value::String(t)) => Some(parse_scale(t).ok_or_else(|| bad(cmd, "scale must read like \"1:2\" or \"2:1\""))?),
        Some(_) => return Err(bad(cmd, "scale must read like \"1:2\"")),
    };
    let paper = |f: &(&str, f64, f64)| if landscape { (f.1, f.2) } else { (f.2, f.1) };
    let (format, (w, h)) = match str_(p, "format") {
        Some(f) => {
            let found = FORMATS.iter().find(|x| x.0.eq_ignore_ascii_case(f)).ok_or_else(|| bad(cmd, "format must be A4, A3, A2, A1 or A0"))?;
            (found.0, paper(found))
        }
        // The smallest sheet that holds the views at 1:1 (or at the asked scale), A3 at most:
        // past that, a smaller scale on an A3 reads better than a huge sheet.
        None => {
            let s = asked_scale.unwrap_or(1.0);
            let found = FORMATS.iter().take(2).find(|x| fits(s, paper(x))).unwrap_or(&FORMATS[1]);
            (found.0, paper(found))
        }
    };
    let scale = match asked_scale {
        Some(s) => s,
        None => SCALES.iter().copied().find(|s| fits(*s, (w, h))).unwrap_or(0.01),
    };
    let mut warnings = Vec::new();
    if !fits(scale, (w, h)) {
        warnings.push(format!("the views do not fit on {format} at {}: choose a smaller scale or a larger format", scale_text(scale)));
    }
    // Cell centres, from the top left of the frame: in first angle the top view goes under
    // the front view and the view from the left on its right; third angle mirrors both.
    let (x0, y1) = (MARGIN_LEFT, h - MARGIN);
    let c1 = x0 + GAP + col1 * scale / 2.0;
    let c2 = x0 + GAP * 2.0 + col1 * scale + col2 * scale / 2.0;
    let r1 = y1 - GAP - row_front * scale / 2.0;
    let r2 = y1 - GAP * 2.0 - row_front * scale - row_top * scale / 2.0;
    let (r_front, r_top) = if first_angle { (r1, r2) } else { (r2, r1) };
    let place = |name: &str| -> Option<[f64; 2]> {
        match name {
            "front" => Some([c1, r_front]),
            "top" => Some([c1, r_top]),
            "left" | "right" => Some([c2, r_front]),
            "iso" => Some([c2, r_top]),
            _ => None,
        }
    };
    let mut views = Vec::new();
    let mut dims = Vec::new();
    let all_holes = if p.get("dimensions").and_then(Value::as_bool).unwrap_or(true) { holes(s) } else { Vec::new() };
    let show_dims = p.get("dimensions").and_then(Value::as_bool).unwrap_or(true);
    let mut called: Vec<usize> = Vec::new();
    for v in &proj {
        let Some(at) = place(v.name) else { continue };
        let mid = [(v.lo[0] + v.hi[0]) / 2.0, (v.lo[1] + v.hi[1]) / 2.0];
        let to_sheet = |q: [f64; 2]| [at[0] + (q[0] - mid[0]) * scale, at[1] + (q[1] - mid[1]) * scale];
        let mut out = View { name: v.name.into(), center: at, ..Default::default() };
        for (a, b, hidden) in &v.lines {
            let l = line(to_sheet(*a), to_sheet(*b));
            if *hidden {
                // A pictorial view shows its visible edges only (ISO 128-3).
                if dashed && v.name != "iso" {
                    out.hidden.push(l);
                }
            } else {
                out.visible.push(l);
            }
        }
        // A hidden edge under a visible one (a through hole's far rim under its near rim) is
        // not drawn twice: ISO 128 draws the visible line only.
        if out.hidden.len() <= 20_000 && out.visible.len() <= 20_000 {
            let near = |p: [f64; 2], l: &[f64; 4]| {
                let (dx, dy) = (l[2] - l[0], l[3] - l[1]);
                let len2 = (dx * dx + dy * dy).max(1e-12);
                let t = (((p[0] - l[0]) * dx + (p[1] - l[1]) * dy) / len2).clamp(0.0, 1.0);
                (p[0] - l[0] - t * dx).hypot(p[1] - l[1] - t * dy) < 0.05
            };
            let visible = out.visible.clone();
            out.hidden.retain(|h| {
                !visible.iter().any(|v| near([h[0], h[1]], v) && near([h[2], h[3]], v))
                    && !(visible.iter().any(|v| near([h[0], h[1]], v))
                        && visible.iter().any(|v| near([h[2], h[3]], v))
                        && visible.iter().any(|v| near([(h[0] + h[2]) / 2.0, (h[1] + h[3]) / 2.0], v)))
            });
        }
        let lo = to_sheet(v.lo);
        let hi = to_sheet(v.hi);
        if v.view != StandardView::Iso {
            let cam = camera(v.view, center);
            let (right, up, back) = cam.basis();
            let flat = |q: Vec3| {
                let d = q - center;
                to_sheet([d.dot(right), d.dot(up)])
            };
            // Hole axes seen end-on: a centre cross; seen from the side: an axis line.
            let mut groups: Vec<(f64, Vec<[f64; 2]>, usize)> = Vec::new();
            for (k, hole) in all_holes.iter().enumerate() {
                let c = flat(hole.center);
                let r = hole.diameter / 2.0 * scale;
                if hole.axis.dot(back).abs() > 0.999 {
                    out.axes.push([c[0] - r - 2.0, c[1], c[0] + r + 2.0, c[1]]);
                    out.axes.push([c[0], c[1] - r - 2.0, c[0], c[1] + r + 2.0]);
                    if !called.contains(&k) {
                        match groups.iter_mut().find(|g| (g.0 - hole.diameter).abs() < 1e-3) {
                            Some(g) => g.1.push(c),
                            None => groups.push((hole.diameter, vec![c], k)),
                        }
                        called.push(k);
                    }
                } else if hole.axis.dot(back).abs() < 1e-3 {
                    // The axis across the view, clipped to the part's outline.
                    let dir = [hole.axis.dot(right), hole.axis.dot(up)];
                    if dir[0].abs() > 0.999 {
                        out.axes.push([lo[0] - 2.0, c[1], hi[0] + 2.0, c[1]]);
                    } else if dir[1].abs() > 0.999 {
                        out.axes.push([c[0], lo[1] - 2.0, c[0], hi[1] + 2.0]);
                    }
                }
            }
            if show_dims {
                // Hole callouts: a leader from the first hole of each size, up and to the right.
                for (diameter, centres, _) in &groups {
                    let c = centres[0];
                    let r = diameter / 2.0 * scale;
                    let a = [c[0] + r * std::f64::consts::FRAC_1_SQRT_2, c[1] + r * std::f64::consts::FRAC_1_SQRT_2];
                    let knee = [a[0] + 6.0, a[1] + 6.0];
                    let text = if centres.len() > 1 { format!("{}× Ø{}", centres.len(), mm(*diameter)) } else { format!("Ø{}", mm(*diameter)) };
                    let end = [knee[0] + text.chars().count() as f64 * TEXT * 0.62, knee[1]];
                    dims.push(Dim {
                        kind: "diameter".into(),
                        line: line(a, knee),
                        ext: vec![line(knee, end)],
                        arrows: 1,
                        text,
                        text_at: [(knee[0] + end[0]) / 2.0, knee[1] + 1.0],
                        text_angle: 0.0,
                    });
                }
                // Overall sizes: the front view's width and height, the top view's depth.
                let model = [v.hi[0] - v.lo[0], v.hi[1] - v.lo[1]];
                match v.name {
                    "front" => {
                        dims.push(linear([lo[0], lo[1]], [hi[0], lo[1]], false, lo[1] - 9.0, model[0]));
                        dims.push(linear([lo[0], lo[1]], [lo[0], hi[1]], true, lo[0] - 9.0, model[1]));
                    }
                    "top" => dims.push(linear([lo[0], lo[1]], [lo[0], hi[1]], true, lo[0] - 9.0, model[1])),
                    _ => {}
                }
            }
        }
        views.push(out);
    }
    let t = p.get("title").cloned().unwrap_or(Value::Null);
    let field = |k: &str, d: &str| t.get(k).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| d.to_string());
    let name = s.doc.name.clone();
    // Material and mass from the model when its bodies have a material and the title does
    // not say otherwise.
    let (mut materials, mut grams) = (Vec::<String>::new(), 0.0);
    for b in s.world_state().bodies.iter().filter(|b| !b.body.is_mesh()) {
        if let Some(m) = s.doc.materials.get(&b.name) {
            let m = french_material(m);
            if !materials.contains(&m) {
                materials.push(m);
            }
            grams += solidcraft_kernel::measure(&b.body).map(|m| m.volume).unwrap_or(0.0) * s.doc.density(&b.name) / 1000.0;
        }
    }
    let mass_text = if grams <= 0.0 {
        String::new()
    } else if grams >= 1000.0 {
        format!("{} kg", mm(grams / 1000.0))
    } else {
        format!("{} g", mm(grams.round()))
    };
    let sheet = Sheet {
        format: format.into(),
        width: w,
        height: h,
        scale,
        scale_text: scale_text(scale),
        projection: if first_angle { "first_angle" } else { "third_angle" }.into(),
        views,
        dims,
        title: Title {
            titre: field("titre", if name.is_empty() || name == "Untitled" { "Pièce" } else { &name }),
            numero: field("numero", ""),
            matiere: field("matiere", &materials.join(", ")),
            masse: field("masse", &mass_text),
            auteur: field("auteur", ""),
            date: field("date", ""),
            tolerance: field("tolerance", "ISO 2768-m"),
        },
        warnings,
    };
    let summary = json!({
        "format": sheet.format, "size": [sheet.width, sheet.height], "scale": sheet.scale_text, "projection": sheet.projection,
        "views": sheet.views.iter().map(|v| json!({"view": v.name, "visible": v.visible.len(), "hidden": v.hidden.len(), "axes": v.axes.len() / 2})).collect::<Vec<_>>(),
        "dimensions": sheet.dims.iter().map(|d| d.text.clone()).collect::<Vec<_>>(),
        "warnings": sheet.warnings,
    });
    s.drawing = Some(serde_json::to_value(&sheet).map_err(|e| bad(cmd, e.to_string()))?);
    s.drawing_seq += 1;
    Ok(summary)
}

trait Widest {
    fn max_by_side(self, other: [f64; 2]) -> [f64; 2];
}
impl Widest for [f64; 2] {
    fn max_by_side(self, other: [f64; 2]) -> [f64; 2] {
        [self[0].max(other[0]), self[1].max(other[1])]
    }
}

fn sheet(s: &Session, cmd: &str) -> Result<Sheet> {
    let v = s.drawing.clone().ok_or_else(|| bad(cmd, "no drawing yet: run drawing.create first"))?;
    serde_json::from_value(v).map_err(|e| bad(cmd, e.to_string()))
}

fn get(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(s.drawing.clone().unwrap_or(Value::Null))
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.export";
    let sh = sheet(s, cmd)?;
    let format = str_(p, "format").unwrap_or("svg");
    let bytes: Vec<u8> = match format {
        "svg" => svg(&sh).into_bytes(),
        "dxf" => dxf(&sh).into_bytes(),
        "pdf" => pdf(&sh),
        _ => return Err(bad(cmd, "format must be svg, dxf or pdf")),
    };
    if let Some(path) = str_(p, "path").filter(|v| !v.trim().is_empty()) {
        solidcraft_io::vfs::check(path).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        solidcraft_io::write_atomic(std::path::Path::new(path), &bytes, false).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        return Ok(json!({"format": format, "path": path, "bytes": bytes.len()}));
    }
    Ok(match format {
        "pdf" => json!({"format": format, "bytes": bytes.len(), "base64": base64(&bytes)}),
        _ => json!({"format": format, "bytes": bytes.len(), "text": String::from_utf8_lossy(&bytes)}),
    })
}

/// What every output draws, in order: the frame, the title block, the views, the dimensions.
pub enum Mark {
    /// A line: its kind (`frame`, `thin`, `visible`, `hidden`, `axis`, `dim`).
    Line(&'static str, [f64; 4]),
    /// A filled arrowhead at `tip`, pointing along `dir`.
    Arrow([f64; 2], [f64; 2]),
    /// Text centred on `at` (its baseline), `size` mm high, turned `angle` degrees.
    Text([f64; 2], f64, f64, String),
}

pub fn marks(sh: &Sheet) -> Vec<Mark> {
    let mut m = Vec::new();
    let (w, h) = (sh.width, sh.height);
    let (x0, y0, x1, y1) = (MARGIN_LEFT, MARGIN, w - MARGIN, h - MARGIN);
    for l in [[x0, y0, x1, y0], [x1, y0, x1, y1], [x1, y1, x0, y1], [x0, y1, x0, y0]] {
        m.push(Mark::Line("frame", l));
    }
    // ISO 5457 centring marks.
    for l in [[w / 2.0, 0.0, w / 2.0, y0 + 5.0], [w / 2.0, h, w / 2.0, y1 - 5.0], [0.0, h / 2.0, x0 + 5.0, h / 2.0], [w, h / 2.0, x1 - 5.0, h / 2.0]]
    {
        m.push(Mark::Line("frame", l));
    }
    // The title block: three rows, the title across the top.
    let (tx0, ty1) = (x1 - TITLE_W, y0 + TITLE_H);
    for l in [[tx0, y0, tx0, ty1], [tx0, ty1, x1, ty1]] {
        m.push(Mark::Line("frame", l));
    }
    let row = TITLE_H / 3.0;
    for l in [[tx0, y0 + row, x1, y0 + row], [tx0, y0 + 2.0 * row, x1, y0 + 2.0 * row]] {
        m.push(Mark::Line("thin", l));
    }
    let cols = [tx0, tx0 + 45.0, tx0 + 90.0, tx0 + 135.0, x1];
    for x in &cols[1..4] {
        m.push(Mark::Line("thin", [*x, y0, *x, y0 + 2.0 * row]));
    }
    let t = &sh.title;
    let cell = |c: usize, r: usize, label: &str, value: &str, m: &mut Vec<Mark>| {
        let (cx0, cx1) = (cols[c], cols[c + 1]);
        let base = y0 + row * r as f64;
        m.push(Mark::Text([cx0 + 2.0 + label.chars().count() as f64 * 0.62, base + row - 3.0], 2.0, 0.0, label.to_string()));
        if !value.is_empty() {
            m.push(Mark::Text([(cx0 + cx1) / 2.0, base + 2.2], TEXT, 0.0, value.to_string()));
        }
    };
    cell(0, 1, "Matière", &t.matiere, &mut m);
    cell(1, 1, "Masse", &t.masse, &mut m);
    cell(2, 1, "Échelle", &sh.scale_text, &mut m);
    cell(3, 1, "Format", &sh.format, &mut m);
    cell(0, 0, "Dessiné par", &t.auteur, &mut m);
    cell(1, 0, "Date", &t.date, &mut m);
    cell(2, 0, "Tolérances", &t.tolerance, &mut m);
    cell(3, 0, "N° de plan", &t.numero, &mut m);
    m.push(Mark::Text([tx0 + TITLE_W / 2.0, y0 + 2.0 * row + 3.6], 5.0, 0.0, t.titre.clone()));
    m.push(Mark::Text([x1 - 14.0, y0 + 2.0 * row + 9.0], 2.0, 0.0, "NØDE CAD".into()));
    // The projection symbol (ISO 128-30): a cone's two views, side by side.
    let (px, py) = (tx0 + 10.0, y0 + 2.0 * row + 6.5);
    let first = sh.projection == "first_angle";
    let (cone_x, circles_x) = if first { (px, px + 11.0) } else { (px + 11.0, px) };
    for l in [
        [cone_x - 4.0, py - 2.0, cone_x + 4.0, py - 3.5],
        [cone_x - 4.0, py + 2.0, cone_x + 4.0, py + 3.5],
        [cone_x - 4.0, py - 2.0, cone_x - 4.0, py + 2.0],
        [cone_x + 4.0, py - 3.5, cone_x + 4.0, py + 3.5],
    ] {
        m.push(Mark::Line("thin", l));
    }
    for r in [2.0, 3.5] {
        let n = 24;
        for i in 0..n {
            let a0 = std::f64::consts::TAU * i as f64 / n as f64;
            let a1 = std::f64::consts::TAU * (i + 1) as f64 / n as f64;
            m.push(Mark::Line("thin", [circles_x + r * a0.cos(), py + r * a0.sin(), circles_x + r * a1.cos(), py + r * a1.sin()]));
        }
    }
    for v in &sh.views {
        m.extend(v.visible.iter().map(|l| Mark::Line("visible", *l)));
        m.extend(v.hidden.iter().map(|l| Mark::Line("hidden", *l)));
        m.extend(v.axes.iter().map(|l| Mark::Line("axis", *l)));
    }
    for d in &sh.dims {
        m.push(Mark::Line("dim", d.line));
        m.extend(d.ext.iter().map(|l| Mark::Line("dim", *l)));
        let [ax, ay, bx, by] = d.line;
        m.push(Mark::Arrow([ax, ay], [ax - bx, ay - by]));
        if d.arrows > 1 {
            m.push(Mark::Arrow([bx, by], [bx - ax, by - ay]));
        }
        m.push(Mark::Text(d.text_at, TEXT, d.text_angle, d.text.clone()));
    }
    m
}

fn arrow_points(tip: [f64; 2], dir: [f64; 2]) -> [[f64; 2]; 3] {
    let len = dir[0].hypot(dir[1]).max(1e-9);
    let (ux, uy) = (dir[0] / len, dir[1] / len);
    let (l, w) = (3.0, 0.5);
    [tip, [tip[0] - ux * l - uy * w, tip[1] - uy * l + ux * w], [tip[0] - ux * l + uy * w, tip[1] - uy * l - ux * w]]
}

fn escape_xml(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn svg(sh: &Sheet) -> String {
    let (w, h) = (sh.width, sh.height);
    let y = |v: f64| h - v;
    let mut paths: std::collections::BTreeMap<&str, String> = Default::default();
    let mut extra = String::new();
    for mark in marks(sh) {
        match mark {
            Mark::Line(kind, [ax, ay, bx, by]) => {
                use std::fmt::Write;
                let _ = write!(paths.entry(kind).or_default(), "M{ax:.3} {:.3}L{bx:.3} {:.3}", y(ay), y(by));
            }
            Mark::Arrow(tip, dir) => {
                let p = arrow_points(tip, dir);
                extra.push_str(&format!(
                    "<path d=\"M{:.3} {:.3}L{:.3} {:.3}L{:.3} {:.3}Z\" fill=\"#000\"/>",
                    p[0][0],
                    y(p[0][1]),
                    p[1][0],
                    y(p[1][1]),
                    p[2][0],
                    y(p[2][1])
                ));
            }
            Mark::Text(at, size, angle, text) => {
                let rot = if angle != 0.0 { format!(" transform=\"rotate({} {:.3} {:.3})\"", -angle, at[0], y(at[1])) } else { String::new() };
                extra.push_str(&format!(
                    "<text x=\"{:.3}\" y=\"{:.3}\" font-size=\"{size}\" text-anchor=\"middle\"{rot}>{}</text>",
                    at[0],
                    y(at[1]),
                    escape_xml(&text)
                ));
            }
        }
    }
    let style = |k: &str| match k {
        "frame" => "stroke-width=\"0.7\"",
        "visible" => "stroke-width=\"0.5\"",
        "hidden" => "stroke-width=\"0.25\" stroke-dasharray=\"2 1\"",
        "axis" => "stroke-width=\"0.18\" stroke-dasharray=\"6 1.5 1 1.5\"",
        _ => "stroke-width=\"0.18\"",
    };
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\
         <rect width=\"{w}\" height=\"{h}\" fill=\"#fff\"/><g fill=\"none\" stroke=\"#000\" stroke-linecap=\"round\">"
    );
    for (k, d) in &paths {
        out.push_str(&format!("<path class=\"{k}\" {} d=\"{d}\"/>", style(k)));
    }
    out.push_str("</g><g font-family=\"osifont, 'DejaVu Sans', Arial, sans-serif\" fill=\"#000\">");
    out.push_str(&extra);
    out.push_str("</g></svg>");
    out
}

pub fn dxf(sh: &Sheet) -> String {
    let mut entities = Vec::new();
    let layer = |k: &str| match k {
        "frame" | "thin" => "CARTOUCHE",
        "visible" => "VISIBLE",
        "hidden" => "HIDDEN",
        "axis" => "AXES",
        _ => "COTES",
    };
    for mark in marks(sh) {
        match mark {
            Mark::Line(k, [ax, ay, bx, by]) => {
                entities.push(DxfEntity { layer: layer(k).into(), shape: DxfShape::Line(Vec2::new(ax, ay), Vec2::new(bx, by)) })
            }
            Mark::Arrow(tip, dir) => {
                let p = arrow_points(tip, dir);
                entities.push(DxfEntity {
                    layer: "COTES".into(),
                    shape: DxfShape::Polyline { points: vec![Vec2::new(p[1][0], p[1][1]), Vec2::new(p[0][0], p[0][1]), Vec2::new(p[2][0], p[2][1])] },
                });
            }
            Mark::Text(at, size, angle, text) => entities.push(DxfEntity {
                layer: "COTES".into(),
                shape: DxfShape::Text { at: Vec2::new(at[0], at[1]), height: size, angle_deg: angle, text: text.replace('Ø', "%%c") },
            }),
        }
    }
    let layers = vec![
        DxfLayer::new("VISIBLE", 7),
        DxfLayer { linetype: DxfLinetype::Hidden, ..DxfLayer::new("HIDDEN", 8) },
        DxfLayer { linetype: DxfLinetype::Dashed, ..DxfLayer::new("AXES", 1) },
        DxfLayer::new("COTES", 3),
        DxfLayer::new("CARTOUCHE", 7),
    ];
    solidcraft_io::write_dxf_drawing(&layers, &entities)
}

/// A one-page vector PDF: lines in their weights and dashes, Helvetica text (WinAnsi, which has
/// Ø, ×, accents and °). Written here: no dependency, and it builds in the browser too.
pub fn pdf(sh: &Sheet) -> Vec<u8> {
    let k = 72.0 / 25.4;
    let mut c = String::new();
    use std::fmt::Write;
    let _ = write!(c, "1 J 1 j ");
    let mut last = "";
    for mark in marks(sh) {
        match mark {
            Mark::Line(kind, [ax, ay, bx, by]) => {
                if kind != last {
                    let (wd, dash) = match kind {
                        "frame" => (0.7, "[] 0 d"),
                        "visible" => (0.5, "[] 0 d"),
                        "hidden" => (0.25, "[5.67 2.83] 0 d"),
                        "axis" => (0.18, "[17 4.25 2.83 4.25] 0 d"),
                        _ => (0.18, "[] 0 d"),
                    };
                    let _ = write!(c, "{:.3} w {dash} ", wd * k);
                    last = kind;
                }
                let _ = write!(c, "{:.2} {:.2} m {:.2} {:.2} l S ", ax * k, ay * k, bx * k, by * k);
            }
            Mark::Arrow(tip, dir) => {
                let p = arrow_points(tip, dir);
                let _ = write!(
                    c,
                    "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l f ",
                    p[0][0] * k,
                    p[0][1] * k,
                    p[1][0] * k,
                    p[1][1] * k,
                    p[2][0] * k,
                    p[2][1] * k
                );
            }
            Mark::Text(at, size, angle, text) => {
                // Centred: Helvetica's average width is about 0.55 of its size.
                let width = text.chars().count() as f64 * size * 0.55;
                let (sa, ca) = (angle.to_radians().sin(), angle.to_radians().cos());
                let (x, y) = (at[0] - ca * width / 2.0, at[1] - sa * width / 2.0);
                let enc: String = text
                    .chars()
                    .map(|ch| match ch {
                        '(' | ')' | '\\' => format!("\\{ch}"),
                        c if (c as u32) < 128 => c.to_string(),
                        c => match winansi(c) {
                            Some(b) => format!("\\{b:03o}"),
                            None => "?".into(),
                        },
                    })
                    .collect();
                let _ = write!(c, "BT /F1 {:.2} Tf {ca:.4} {sa:.4} {:.4} {ca:.4} {:.2} {:.2} Tm ({enc}) Tj ET ", size * k * 1.4, -sa, x * k, y * k);
                last = "";
            }
        }
    }
    let (pw, ph) = (sh.width * k, sh.height * k);
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"),
        format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len() + 1),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

/// Standard base64 with padding (RFC 4648), for a PDF returned as text.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= chunk.len() { char::from(T[((n >> shift) & 63) as usize]) } else { '=' });
        }
    }
    out
}

/// A letter's code in WinAnsiEncoding, when it has one.
fn winansi(c: char) -> Option<u8> {
    let u = c as u32;
    if (0xA0..=0xFF).contains(&u) {
        return Some(u as u8);
    }
    Some(match c {
        '€' => 0x80,
        '–' => 0x96,
        '—' => 0x97,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '…' => 0x85,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "view_dxf_drawing_tests.rs"]
mod tests;
