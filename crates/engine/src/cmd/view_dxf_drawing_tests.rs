use serde_json::{Value, json};

use super::*;

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 120, "width": 80, "height": 10, "body_name": "Platine"}));
    for (x, y) in [(10, 10), (110, 10), (110, 70), (10, 70)] {
        run(&mut s, "solid.hole", json!({"position": [x, y, 10], "clearance": "M6"}));
    }
    s
}

/// A plate with four holes: four views on an A3 at 1:1, its overall sizes and one callout
/// for the four holes, seen end-on in the top view with their centre crosses.
#[test]
fn a_plate_gets_its_views_sizes_and_a_hole_callout() {
    let mut s = plate();
    let r = run(&mut s, "drawing.create", json!({"title": {"titre": "Platine", "matiere": "EN AW-2017"}}));
    assert_eq!(r["format"], "A3", "{r}");
    assert_eq!(r["scale"], "1:1", "{r}");
    assert_eq!(r["projection"], "first_angle");
    let views: Vec<&str> = r["views"].as_array().unwrap().iter().map(|v| v["view"].as_str().unwrap()).collect();
    assert_eq!(views, ["front", "top", "left", "iso"]);
    let dims: Vec<&str> = r["dimensions"].as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
    for want in ["120", "10", "80", "4× Ø6,6"] {
        assert!(dims.contains(&want), "{want} in {dims:?}");
    }
    let top = r["views"].as_array().unwrap().iter().find(|v| v["view"] == "top").unwrap();
    assert_eq!(top["axes"].as_u64().unwrap(), 4, "four centre crosses: {top}");
    assert!(top["hidden"].as_u64().unwrap() < 20, "the holes' far edges lie under their visible circles: not drawn twice ({top})");
    // The holes run through: the front view shows them dashed.
    let front = r["views"].as_array().unwrap().iter().find(|v| v["view"] == "front").unwrap();
    assert!(front["hidden"].as_u64().unwrap() > 0, "{front}");
    let iso = r["views"].as_array().unwrap().iter().find(|v| v["view"] == "iso").unwrap();
    assert_eq!(iso["hidden"].as_u64().unwrap(), 0, "no hidden lines in the pictorial view: {iso}");
    assert!(r["warnings"].as_array().unwrap().is_empty(), "{r}");
}

/// The title block takes the material and the mass from the model unless it is told them.
#[test]
fn the_title_block_takes_material_and_mass_from_the_model() {
    let mut s = plate();
    run(&mut s, "material.assign", json!({"bodies": ["Platine"], "material": "Aluminum"}));
    run(&mut s, "drawing.create", json!({}));
    let t = &s.drawing.clone().unwrap()["title"];
    assert_eq!(t["matiere"], "Aluminium");
    // 94 632 mm³ of aluminium at 2.7 g/cm³.
    assert_eq!(t["masse"], "256 g", "{t}");
    run(&mut s, "drawing.create", json!({"title": {"matiere": "EN AW-2017", "masse": "0,26 kg"}}));
    assert_eq!(s.drawing.clone().unwrap()["title"]["matiere"], "EN AW-2017");
}

/// The drawing writes as SVG, DXF and PDF, each with its frame, views, dimensions and title.
#[test]
fn the_drawing_exports_as_svg_dxf_and_pdf() {
    let mut s = plate();
    run(&mut s, "drawing.create", json!({"title": {"titre": "Platine", "numero": "NC-001"}}));
    let svg = run(&mut s, "drawing.export", json!({"format": "svg"}))["text"].as_str().unwrap().to_string();
    assert!(
        svg.starts_with("<svg") && svg.contains("width=\"420mm\"") && svg.contains("4× Ø6,6") && svg.contains("Platine") && svg.contains("NC-001")
    );
    assert!(svg.contains("class=\"hidden\"") && svg.contains("class=\"axis\""));
    let dxf = run(&mut s, "drawing.export", json!({"format": "dxf"}))["text"].as_str().unwrap().to_string();
    for layer in ["VISIBLE", "HIDDEN", "AXES", "COTES", "CARTOUCHE"] {
        assert!(dxf.contains(layer), "{layer}");
    }
    assert!(dxf.contains("%%c6,6"), "Ø as AutoCAD's diameter sign");
    let pdf = pdf(&sheet(&s, "test").unwrap());
    assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/MediaBox [0 0 1190.55 841.89]"), "A3 in points");
    assert!(text.contains("(4\\327 \\3306,6) Tj"), "× and Ø in WinAnsi");
    let b64 = run(&mut s, "drawing.export", json!({"format": "pdf"}));
    assert!(b64["base64"].as_str().unwrap().starts_with("JVBERi0xLjQ"));
}

/// A long part goes to a smaller standard scale, third angle swaps the views, and nothing to
/// draw or a wrong parameter is refused with why.
#[test]
fn scale_projection_and_refusals() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 900, "width": 100, "height": 50}));
    let r = run(&mut s, "drawing.create", json!({"projection": "third_angle", "views": ["front", "top", "right"]}));
    assert_eq!(r["format"], "A3");
    assert_eq!(r["scale"], "1:5", "{r}");
    let d = s.drawing.clone().unwrap();
    let centre = |n: &str| d["views"].as_array().unwrap().iter().find(|v| v["name"] == n).unwrap()["center"][1].as_f64().unwrap();
    assert!(centre("top") > centre("front"), "third angle: the top view above the front view");
    let r = run(&mut s, "drawing.create", json!({"scale": "1:10", "format": "A4"}));
    assert_eq!(r["scale"], "1:10");
    assert!(s.execute("drawing.create", &json!({"views": ["dessus"]})).is_err());
    assert!(s.execute("drawing.create", &json!({"scale": "moitié"})).is_err());
    let mut empty = Session::default();
    assert!(empty.execute("drawing.create", &json!({})).is_err());
    assert!(empty.execute("drawing.export", &json!({"format": "svg"})).is_err());
}

#[test]
fn numbers_are_written_the_french_way() {
    assert_eq!(mm(6.6), "6,6");
    assert_eq!(mm(120.0), "120");
    assert_eq!(mm(12.345), "12,35");
    assert_eq!(scale_text(0.5), "1:2");
    assert_eq!(scale_text(2.0), "2:1");
}

/// A pocket's rounded corners are internal cylinders a quarter turn each: not holes, so no
/// « 4× Ø10 » callout for a pocket with R5 corners.
#[test]
fn rounded_pocket_corners_are_not_holes() {
    let mut s = plate();
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10}));
    run(&mut s, "sketch.rectangle.rounded", json!({"p0": [30, 20], "p1": [90, 60], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": -5, "operation": "cut"}));
    let r = run(&mut s, "drawing.create", json!({}));
    let dims: Vec<&str> = r["dimensions"].as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
    assert!(dims.contains(&"4× Ø6,6") && !dims.iter().any(|d| d.contains("Ø10")), "{dims:?}");
}
