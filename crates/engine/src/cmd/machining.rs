//! Machinability (`inspect.machining`): what a milling and drilling shop cannot make as the
//! model draws it, for a tool along one axis. Sharp internal corners along the tool (an end mill
//! leaves its radius there), internal rounds along the tool smaller than the tool's radius, and
//! holes deeper than a drill reaches. Read from each body's display mesh: an edge is concave
//! where the faces on its two sides fold towards each other, and a round is a cylindrical face
//! seen from inside.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};
use solidcraft_geom::{Mesh, Vec3};
use solidcraft_kernel::FaceSurface;

use super::CommandSpec;
use crate::params::{bad, num, string_list};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("inspect.machining", "Machinability", machining).icon("inspect").noundo().params(
    "bodies?: [names] (default: every solid body); axis?: X|Y|Z|[x,y,z] the tool's axis (default Z: milled from above or below); \
     tool_diameter?: mm, the smallest end mill to be used (default 6); hole_depth_ratio?: the deepest hole a drill makes, in diameters \
     (default 10) → bodies: [{body, sharp_corners: [{at, length}] (internal edges along the tool between two flat faces: an end mill \
     cannot cut them sharp), small_radii: [{at, radius}] (internal rounds along the tool below the tool's radius), deep_holes: [{at, \
     diameter, depth}]}], ok (nothing found), advice: what to change. A rough check of the model, not of a machine's reach",
)];

/// Most bodies one call looks at.
const MAX_BODIES: usize = 2000;
/// Most findings of one kind listed per body (`more` counts the rest).
const MAX_LISTED: usize = 50;

fn machining(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.machining";
    let axis = super::features::axis_dir_param(p, cmd)?;
    let tool = num(p, "tool_diameter").unwrap_or(6.0);
    if !(tool.is_finite() && tool > 0.0 && tool <= 1000.0) {
        return Err(bad(cmd, "`tool_diameter` must be a diameter in mm (0 to 1000)"));
    }
    let ratio = num(p, "hole_depth_ratio").unwrap_or(10.0);
    if !(ratio.is_finite() && ratio > 0.0) {
        return Err(bad(cmd, "`hole_depth_ratio` must be a positive number of diameters"));
    }
    let want = string_list(p, "bodies");
    let st = s.world_state();
    for name in &want {
        if !st.bodies.iter().any(|b| &b.name == name) {
            return Err(bad(cmd, format!("no body `{name}`")));
        }
    }
    let bodies: Vec<_> = st.bodies.iter().filter(|b| !b.body.is_mesh() && (want.is_empty() || want.contains(&b.name))).take(MAX_BODIES).collect();
    let (mut out, mut corners, mut radii, mut holes) = (Vec::new(), 0usize, 0usize, 0usize);
    for b in bodies {
        let mesh = b.mesh();
        let found = body_findings(&b.body, &mesh, axis, tool / 2.0, ratio);
        corners += found.sharp.len();
        radii += found.small.len();
        holes += found.deep.len();
        let list = |v: Vec<Value>| -> (Vec<Value>, usize) {
            let more = v.len().saturating_sub(MAX_LISTED);
            (v.into_iter().take(MAX_LISTED).collect(), more)
        };
        let (sharp, more_sharp) = list(found.sharp);
        let (small, more_small) = list(found.small);
        let (deep, more_deep) = list(found.deep);
        let mut row = json!({"body": b.name, "sharp_corners": sharp, "small_radii": small, "deep_holes": deep});
        for (k, n) in [("more_sharp_corners", more_sharp), ("more_small_radii", more_small), ("more_deep_holes", more_deep)] {
            if n > 0 {
                row[k] = json!(n);
            }
        }
        out.push(row);
    }
    let r = (tool / 2.0 * 10.0).round() / 10.0;
    let mut advice = Vec::new();
    if corners > 0 {
        advice.push(format!(
            "sharp internal corners along the tool: round them to at least {r} mm (solid.fillet on these edges, or sketch.fillet in the pocket's sketch), or plan them for EDM"
        ));
    }
    if radii > 0 {
        advice.push(format!("internal rounds below {r} mm: a Ø{tool} mm end mill cannot follow them; enlarge them or plan a smaller tool"));
    }
    if holes > 0 {
        advice.push(format!("holes deeper than {ratio} diameters: drill them from both sides, enlarge them, or plan deep-hole drilling"));
    }
    Ok(json!({
        "axis": [axis.x, axis.y, axis.z],
        "tool_diameter": tool,
        "bodies": out,
        "ok": corners + radii + holes == 0,
        "advice": advice,
    }))
}

#[derive(Default)]
struct Findings {
    sharp: Vec<Value>,
    small: Vec<Value>,
    deep: Vec<Value>,
}

fn round3(p: Vec3) -> Value {
    let r = |x: f64| (x * 1000.0).round() / 1000.0 + 0.0;
    json!([r(p.x), r(p.y), r(p.z)])
}

fn body_findings(body: &solidcraft_kernel::Body, mesh: &Mesh, axis: Vec3, tool_radius: f64, ratio: f64) -> Findings {
    let mut f = Findings::default();
    let size = body.size().max(1e-9);
    let parallel = |d: Vec3| d.normalized().is_some_and(|d| d.dot(axis).abs() > (1.0f64).to_radians().cos());
    // The triangles at each vertex, found by position (faces do not share their vertices).
    let q = 1e6 / size;
    let key = |p: Vec3| ((p.x * q).round() as i64, (p.y * q).round() as i64, (p.z * q).round() as i64);
    let mut at: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (t, tri) in mesh.triangles.iter().enumerate() {
        for v in tri {
            if let Some(p) = mesh.positions.get(*v as usize) {
                at.entry(key(*p)).or_default().push(t);
            }
        }
    }
    let surfaces: Vec<Option<FaceSurface>> = (0..body.face_count()).map(|i| solidcraft_kernel::face_surface(body, i)).collect();
    // Sharp internal corners: concave straight edges along the tool between two flat faces.
    for (e, pts) in mesh.edges.iter().enumerate() {
        if mesh.seams.get(e).copied().unwrap_or(false) || pts.len() < 2 {
            continue;
        }
        let (a, z) = (pts[0], pts[pts.len() - 1]);
        let dir = z - a;
        let len = dir.len();
        if len <= 1e-9 || !parallel(dir) || pts.iter().any(|p| (*p - a).cross(dir).len() / len > 1e-6 * size) {
            continue;
        }
        let k = pts.len() / 2;
        let (p0, p1) = (pts[k - 1], pts[k]);
        let (k0, k1) = (key(p0), key(p1));
        let (Some(t0), Some(t1)) = (at.get(&k0), at.get(&k1)) else { continue };
        // One triangle per side: those holding both ends of the segment.
        let mut sides: BTreeMap<u32, (Vec3, Vec3)> = BTreeMap::new();
        for t in t0.iter().filter(|t| t1.contains(t)) {
            let Some(face) = mesh.tri_face.get(*t).copied() else { continue };
            let Some([x, y, w]) = mesh.tri(&mesh.triangles[*t]) else { continue };
            let Some(n) = (y - x).cross(w - x).normalized() else { continue };
            // The triangle's third corner, away from the segment: the way into its face.
            let third = [x, y, w].into_iter().find(|v| key(*v) != k0 && key(*v) != k1).unwrap_or(x);
            let u = (p1 - p0).normalized().unwrap_or(axis);
            let Some(into) = ((third - p0) - u * (third - p0).dot(u)).normalized() else { continue };
            sides.entry(face).or_insert((n, into));
        }
        if sides.len() != 2 {
            continue;
        }
        let faces: Vec<u32> = sides.keys().copied().collect();
        let flat = faces.iter().all(|i| matches!(surfaces.get(*i as usize), Some(Some(FaceSurface::Plane { .. }))));
        let (n0, d0) = sides[&faces[0]];
        let (n1, d1) = sides[&faces[1]];
        // Concave: each face runs off towards the other's outside.
        let concave = n1.dot(d0) > 1e-3 && n0.dot(d1) > 1e-3;
        if flat && concave {
            f.sharp.push(json!({"at": round3((a + z) * 0.5), "length": (len * 1000.0).round() / 1000.0}));
        }
    }
    // Internal cylinders: rounds (part of a turn) and holes (a whole turn), by axis line and
    // radius, as a hole's two half faces make one.
    let mut groups: Vec<(solidcraft_kernel::CylinderFace, Vec<f64>)> = Vec::new();
    for i in 0..body.face_count() {
        let Some(c) = solidcraft_kernel::cylinder_face(body, mesh, i).filter(|c| c.internal) else { continue };
        let ax = *c.axis;
        let (s0, s1) = {
            let u = if ax.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
            let s0 = ax.cross(u).normalized().unwrap_or(Vec3::X);
            (s0, ax.cross(s0))
        };
        let angles: Vec<f64> = mesh
            .triangles
            .iter()
            .zip(&mesh.tri_face)
            .filter(|(_, fc)| **fc as usize == i)
            .flat_map(|(t, _)| t.iter().filter_map(|v| mesh.positions.get(*v as usize)).copied().collect::<Vec<_>>())
            .map(|p| {
                let v = p - c.axis_point;
                v.dot(s1).atan2(v.dot(s0))
            })
            .collect();
        let same = groups.iter_mut().find(|(g, _)| {
            let v = c.axis_point - g.axis_point;
            g.axis.dot(ax).abs() > 1.0 - 1e-9 && (v - *g.axis * v.dot(*g.axis)).len() < 1e-6 * size && (g.radius - c.radius).abs() < 1e-6 * size
        });
        match same {
            Some((g, a)) => {
                let (lo, hi) = (g.start.min(c.start), g.end.max(c.end));
                g.start = lo;
                g.end = hi;
                // The second face's angles in the first one's frame: same axis line, so the
                // frame differs only by the axis' sign.
                let flip = g.axis.dot(ax) < 0.0;
                a.extend(angles.into_iter().map(|t| if flip { -t } else { t }));
            }
            None => groups.push((c, angles)),
        }
    }
    for (c, mut angles) in groups {
        // How much of a turn the faces cover: a whole turn but its largest gap.
        angles.sort_by(f64::total_cmp);
        let gap = angles.windows(2).map(|w| w[1] - w[0]).chain(angles.first().zip(angles.last()).map(|(a, z)| a + std::f64::consts::TAU - z)).fold(0.0, f64::max);
        let turn = std::f64::consts::TAU - gap;
        let middle = c.axis_point + *c.axis * ((c.start + c.end) / 2.0);
        let depth = (c.end - c.start).abs();
        if turn > 0.95 * std::f64::consts::TAU {
            if depth > ratio * 2.0 * c.radius {
                f.deep.push(json!({"at": round3(middle), "diameter": (c.radius * 2000.0).round() / 1000.0, "depth": (depth * 1000.0).round() / 1000.0}));
            }
        } else if parallel(*c.axis) && c.radius < tool_radius - 1e-9 {
            f.small.push(json!({"at": round3(middle), "radius": (c.radius * 1000.0).round() / 1000.0}));
        }
    }
    f
}
