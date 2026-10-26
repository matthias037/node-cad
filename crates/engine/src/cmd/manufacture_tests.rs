use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn plate(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0.0, 0.0], "p1": [40.0, 30.0]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Holes"}));
    run(s, "sketch.circle.center", json!({"center": [10.0, 10.0], "diameter": 6.0}));
    run(s, "sketch.circle.center", json!({"center": [30.0, 20.0], "diameter": 6.0}));
    run(s, "sketch.finish", json!({}));
}

fn setup_job(s: &mut Session) {
    run(s, "manufacture.tool.set", json!({"id": 1, "diameter": 6.0}));
    run(s, "manufacture.tool.set", json!({"id": 2, "kind": "drill", "diameter": 6.0}));
    run(s, "manufacture.setup.set", json!({"id": 1, "plane": "XY", "stock": {"min": [-5.0, -5.0, -3.0], "max": [45.0, 35.0, 0.0]}}));
}

#[test]
fn job_round_trip_with_undo() {
    let mut s = Session::default();
    plate(&mut s);
    setup_job(&mut s);
    run(
        &mut s,
        "manufacture.operation.set",
        json!({"setup": 1, "tool": 1, "kind": {"type": "contour", "source": {"sketch": "Outline"}, "side": "outside"}, "top": 0.0, "bottom": -3.0, "feed": 600.0, "spindle_rpm": 8000.0}),
    );
    let job = run(&mut s, "manufacture.job", json!({}));
    assert_eq!(job["operations"].as_array().map(Vec::len), Some(1));
    assert_eq!(job["tools"].as_array().map(Vec::len), Some(2));
    run(&mut s, "edit.undo", json!({}));
    let job = run(&mut s, "manufacture.job", json!({}));
    assert_eq!(job["operations"].as_array().map(Vec::len), Some(0));
}

#[test]
fn toolpath_and_post_all_dialects() {
    let mut s = Session::default();
    plate(&mut s);
    setup_job(&mut s);
    run(
        &mut s,
        "manufacture.operation.set",
        json!({"setup": 1, "tool": 1, "kind": {"type": "contour", "source": {"sketch": "Outline"}, "side": "outside"}, "top": 0.0, "bottom": -3.0, "feed": 600.0, "spindle_rpm": 8000.0}),
    );
    run(
        &mut s,
        "manufacture.operation.set",
        json!({"setup": 1, "tool": 2, "kind": {"type": "drill", "points": {"from": "sketch", "sketch": "Holes"}}, "top": 0.0, "bottom": -3.0, "feed": 200.0, "spindle_rpm": 3000.0}),
    );
    let t = run(&mut s, "manufacture.toolpath", json!({"operation": 2, "moves": true}));
    assert!(t["moves"].as_u64().unwrap() >= 2);
    for d in ["grbl", "fanuc", "haas"] {
        let r = run(&mut s, "manufacture.post", json!({"dialect": d}));
        assert!(r["gcode"].as_str().unwrap().contains("M30") || r["gcode"].as_str().unwrap().contains("M5"), "{d}");
    }
}

#[test]
fn hostile_params_are_errors() {
    let mut s = Session::default();
    plate(&mut s);
    setup_job(&mut s);
    for (id, p) in [
        ("manufacture.tool.set", json!({"diameter": -1.0})),
        ("manufacture.tool.set", json!({"diameter": 1e300})),
        ("manufacture.tool.set", json!({"id": 0, "diameter": 3.0})),
        ("manufacture.tool.set", json!({"diameter": 3.0, "bogus": 1})),
        ("manufacture.tool.delete", json!({"id": 99})),
        ("manufacture.setup.set", json!({"plane": "QQ"})),
        ("manufacture.setup.set", json!({"clearance": 1e308})),
        (
            "manufacture.operation.set",
            json!({"setup": 1, "tool": 1, "kind": {"type": "face"}, "top": 0.0, "bottom": 1.0, "feed": 1.0, "spindle_rpm": 1.0}),
        ),
        ("manufacture.operation.set", json!({"setup": 9, "tool": 1})),
        ("manufacture.toolpath", json!({"operation": 99})),
        ("manufacture.post", json!({"dialect": "mach3"})),
        ("manufacture.post", json!({"dialect": "grbl"})),
        ("manufacture.post", json!([])),
    ] {
        assert!(s.execute(id, &p).is_err(), "{id} {p}");
    }
}

#[test]
fn delete_refused_while_used() {
    let mut s = Session::default();
    plate(&mut s);
    setup_job(&mut s);
    run(
        &mut s,
        "manufacture.operation.set",
        json!({"setup": 1, "tool": 1, "kind": {"type": "face"}, "top": 0.0, "bottom": -1.0, "feed": 600.0, "spindle_rpm": 8000.0}),
    );
    assert!(s.execute("manufacture.tool.delete", &json!({"id": 1})).is_err());
    assert!(s.execute("manufacture.setup.delete", &json!({"id": 1})).is_err());
    run(&mut s, "manufacture.operation.delete", json!({"id": 1}));
    run(&mut s, "manufacture.tool.delete", json!({"id": 1}));
}

fn block_with_pocket_job(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Plate"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0.0, 0.0], "p1": [40.0, 30.0]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 10.0}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Pocket"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [10.0, 8.0], "p1": [30.0, 22.0]}));
    run(s, "sketch.finish", json!({}));
    run(s, "manufacture.tool.set", json!({"id": 1, "diameter": 4.0}));
    // Work Z = 0 at the part's top (model Z = 10).
    run(
        s,
        "manufacture.setup.set",
        json!({"id": 1, "plane": "XY", "origin": [0.0, 0.0, 10.0], "stock": {"min": [-5.0, -5.0, -10.0], "max": [45.0, 35.0, 3.0]}}),
    );
    run(
        s,
        "manufacture.operation.set",
        json!({"setup": 1, "tool": 1, "kind": {"type": "pocket", "source": {"sketch": "Pocket"}, "stepover": 0.5}, "top": 0.0, "bottom": -4.0, "feed": 500.0, "spindle_rpm": 8000.0}),
    );
}

#[test]
fn simulate_reports_gouges_into_the_model_and_volumes() {
    let mut s = Session::default();
    block_with_pocket_job(&mut s);
    let r = run(&mut s, "manufacture.simulate", json!({"resolution": 0.5, "surface": 400}));
    // The pocket is cut out of a solid block: that is a gouge about 4 mm deep.
    assert!(r["counts"]["gouge"].as_u64().unwrap() >= 1, "{r}");
    let gouged = r["volumes"]["gouged"].as_f64().unwrap();
    assert!((gouged - 20.0 * 14.0 * 4.0).abs() < 20.0 * 14.0 * 4.0 * 0.15, "{gouged}");
    assert!(r["collisions"].as_array().unwrap().iter().any(|c| c["kind"] == "gouge" && c["operation"] == 1));
    assert_eq!(r["steps"], r["total_steps"]);
    let surface = &r["surface"];
    assert_eq!(surface["z"].as_array().unwrap().len(), surface["nx"].as_u64().unwrap() as usize * surface["ny"].as_u64().unwrap() as usize);
    // Stock alone: the same cut, no collisions, and a stop part way removes less.
    let free = run(&mut s, "manufacture.simulate", json!({"part": false}));
    assert_eq!(free["counts"]["gouge"], 0);
    let removed = free["volumes"]["removed"].as_f64().unwrap();
    // The pocket, plus the 3 mm of stock standing above the part's top.
    assert!((removed - 20.0 * 14.0 * 7.0).abs() < 20.0 * 14.0 * 7.0 * 0.1, "{removed}");
    let early = run(&mut s, "manufacture.simulate", json!({"part": false, "until": 40}));
    assert!(early["volumes"]["removed"].as_f64().unwrap() < removed && early["steps"] == 40);
    // It does not change the design.
    let before = s.revision;
    run(&mut s, "manufacture.simulate", json!({}));
    assert_eq!(s.revision, before);
}

#[test]
fn simulate_refuses_bad_requests() {
    let mut s = Session::default();
    block_with_pocket_job(&mut s);
    for p in [
        json!({"resolution": 0.0}),
        json!({"resolution": -1.0}),
        json!({"resolution": 1e9}),
        json!({"tolerance": "deep"}),
        json!({"tolerance": 1e9}),
        json!({"until": -1}),
        json!({"setup": 9}),
        json!({"operations": "all"}),
        json!({"operations": [99]}),
        json!({"bodies": 3}),
        json!({"tolerance_chord": 0.0}),
        json!([]),
    ] {
        assert!(s.execute("manufacture.simulate", &p).is_err(), "{p}");
    }
    // A setup without stock cannot be simulated.
    run(&mut s, "manufacture.setup.set", json!({"id": 2, "plane": "XY"}));
    assert!(s.execute("manufacture.simulate", &json!({"setup": 2})).is_err());
}
