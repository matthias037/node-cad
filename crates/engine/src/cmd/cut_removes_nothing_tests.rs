use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["bodies"][0]["volume_mm3"].as_f64().unwrap()
}

/// A pocket sketched on a plate's top face and cut upwards removes nothing: kept, with a warning
/// giving the direction to take. Cut downwards, it is a pocket, without the warning.
#[test]
fn a_cut_that_removes_nothing_says_so_with_the_direction() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 20, "body_name": "Plaque"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 20}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 15], "p1": [80, 45]}));
    run(&mut s, "sketch.finish", json!({}));
    let r = run(&mut s, "solid.extrude", json!({"distance": 8, "operation": "cut"}));
    let w = r["warning"].as_str().unwrap_or_default();
    assert!(w.contains("removes nothing") && w.contains("negative distance"), "{r}");
    assert_eq!(volume(&mut s), 120_000.0);
    run(&mut s, "edit.undo", json!({}));
    let r = run(&mut s, "solid.extrude", json!({"distance": -8, "operation": "cut"}));
    assert!(r["warning"].is_null(), "{r}");
    assert!((volume(&mut s) - (120_000.0 - 60.0 * 30.0 * 8.0)).abs() < 1e-3);
}
