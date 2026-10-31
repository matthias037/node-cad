//! Arc conditions: a distance to a circle or arc measured from its centre, its edge nearest the
//! other entity (min) or its edge farthest from it (max), chosen when dimensioning
//! (`arc_condition`) or afterwards (`sketch.dimension_condition`), for a point, a line or another
//! circle.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn sketch(s: &Session) -> solidcraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

fn point(s: &Session, id: &str) -> Vec2 {
    let sk = sketch(s);
    sk.resolve_point(id).and_then(|p| sk.point(p)).unwrap()
}

/// Two circles side by side: `c1` of radius 5 fixed at the origin, `c2` of radius 3 at (20, 0)
/// on the x axis, both radii dimensioned.
fn two_circles() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 0], "radius": 3}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["c1.center"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["c1"], "type": "radius", "value": 5}));
    run(&mut s, "sketch.dimension", json!({"entities": ["c2"], "type": "radius", "value": 3}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["c1.center", "c2.center"]}));
    s
}

#[test]
fn two_circles_dimension_centre_to_centre_by_default_or_edge_to_edge() {
    let mut s = two_circles();
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"]}));
    assert_eq!(r["measured"], 20.0, "{r}");
    let mut s = two_circles();
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"], "arc_condition": "min"}));
    assert_eq!(r["measured"], 12.0, "the gap between the edges: {r}");
    let param = r["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": param, "expression": "4 mm"}));
    assert!(point(&s, "c2.center").dist(Vec2::new(12.0, 0.0)) < 1e-7, "{:?}", point(&s, "c2.center"));
    let mut s = two_circles();
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"], "arc_condition": "max", "value": 30}));
    assert_eq!(r["measured"], 30.0, "{r}");
    assert!(point(&s, "c2.center").dist(Vec2::new(22.0, 0.0)) < 1e-7, "over both: 30 - 5 - 3: {:?}", point(&s, "c2.center"));
    // One condition per end: from the first centre to the far edge of the second.
    let mut s = two_circles();
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"], "arc_condition": ["center", "max"]}));
    assert_eq!(r["measured"], 23.0, "{r}");
}

#[test]
fn a_point_and_a_circle_dimension_to_the_centre_or_either_edge() {
    let mut s = two_circles();
    run(&mut s, "sketch.point", json!({"point": [0, 12]}));
    let pt = sketch(&s).points.last().map(|p| p.id.clone()).unwrap();
    let r = run(&mut s, "sketch.dimension", json!({"entities": [pt, "c1"]}));
    assert_eq!(r["measured"], 12.0, "{r}");
    run(&mut s, "edit.undo", json!({}));
    let r = run(&mut s, "sketch.dimension", json!({"entities": [pt, "c1"], "arc_condition": "min"}));
    assert_eq!(r["measured"], 7.0, "{r}");
    run(&mut s, "edit.undo", json!({}));
    let r = run(&mut s, "sketch.dimension", json!({"entities": [pt, "c1"], "arc_condition": "max"}));
    assert_eq!(r["measured"], 17.0, "{r}");
}

#[test]
fn a_line_and_a_circle_dimension_to_the_far_edge_or_the_centre() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1"]}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5}));
    run(&mut s, "sketch.dimension", json!({"entities": ["c1"], "type": "radius", "value": 5}));
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["l1", "c1"], "arc_condition": "max"}));
    assert_eq!(r["measured"], 20.0, "{r}");
    let param = r["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": param, "expression": "25"}));
    assert!((point(&s, "c1.center").y - 20.0).abs() < 1e-7, "{:?}", point(&s, "c1.center"));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["l1", "c1"], "arc_condition": "center"}));
    assert_eq!(r["measured"], 15.0, "{r}");
}

#[test]
fn the_condition_changes_afterwards_and_the_sketch_stays_put() {
    let mut s = two_circles();
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"]}));
    let param = r["param"].as_str().unwrap().to_string();
    let at = point(&s, "c2.center");
    let r = run(&mut s, "sketch.dimension_condition", json!({"dimension": param, "arc_condition": "min"}));
    assert_eq!(r["measured"], 12.0, "{r}");
    assert!(point(&s, "c2.center").dist(at) < 1e-7, "nothing moves");
    assert_eq!(s.doc.param(&param).unwrap().expr, "12 mm");
    let r = run(&mut s, "sketch.dimension_condition", json!({"dimension": param, "arc_condition": "max"}));
    assert_eq!(r["measured"], 28.0, "{r}");
    let r = run(&mut s, "sketch.dimension_condition", json!({"dimension": param, "arc_condition": "center"}));
    assert_eq!(r["measured"], 20.0, "{r}");
    let sk = sketch(&s);
    let k = &sk.constraints.iter().find(|c| c.param.as_deref() == Some(param.as_str())).unwrap().kind;
    assert!(matches!(k, solidcraft_sketch::ConstraintKind::Distance { .. }), "{k:?}");
    assert_eq!(crate::cmd::arc_conditions_of(&sk, k), Some([Some("center"), Some("center")]));
    // The value still drives it.
    run(&mut s, "sketch.dimension_condition", json!({"dimension": param, "arc_condition": "min"}));
    run(&mut s, "parameters.change", json!({"name": param, "expression": "2 mm"}));
    assert!(point(&s, "c2.center").dist(Vec2::new(10.0, 0.0)) < 1e-7, "{:?}", point(&s, "c2.center"));
    // A plain distance between two points has nothing to choose.
    run(&mut s, "sketch.line", json!({"points": [[0, 30], [10, 30]]}));
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["l1"]}));
    let err = s.execute("sketch.dimension_condition", &json!({"dimension": r["param"], "arc_condition": "min"})).unwrap_err().to_string();
    assert!(err.contains("circle or arc"), "{err}");
}

#[test]
fn the_condition_saves_reopens_and_undoes() {
    let mut s = two_circles();
    let before = sketch(&s).constraints.len();
    run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"], "arc_condition": ["min", "max"], "value": 20}));
    let saved = s.doc.to_json();
    assert!(saved.contains("\"round_distance\""), "not saved");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    run(&mut reopened, "sketch.edit", json!({"sketch": "Sketch1"}));
    // 20 = d - 5 + 3: the centres lie 22 apart.
    assert!(point(&reopened, "c2.center").dist(Vec2::new(22.0, 0.0)) < 1e-7, "{:?}", point(&reopened, "c2.center"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(sketch(&s).constraints.len(), before);
}

/// A ring: the least distance between its two circles is its wall, centred or not.
#[test]
fn the_min_of_a_ring_is_its_wall() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 20}));
    run(&mut s, "sketch.circle.center", json!({"center": [3, 0], "radius": 15}));
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"], "arc_condition": "min"}));
    assert_eq!(r["measured"], 2.0, "20 - 3 - 15: {r}");
    let sk = sketch(&s);
    let k = &sk.constraints.last().unwrap().kind;
    assert_eq!(crate::cmd::arc_conditions_of(&sk, k), Some([Some("min"), Some("min")]));
    let r = run(&mut s, "sketch.dimension_condition", json!({"dimension": r["param"], "arc_condition": "max"}));
    assert_eq!(r["measured"], 38.0, "3 + 20 + 15: {r}");
}
