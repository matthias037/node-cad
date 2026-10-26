//! Small 2D helpers on closed polylines (rings). A ring is a list of points; the last point
//! joins back to the first and is not repeated.

use solidcraft_geom::Vec2;

/// Signed area: positive when the ring runs counter-clockwise.
pub(crate) fn signed_area(r: &[Vec2]) -> f64 {
    let mut a = 0.0;
    for (i, p) in r.iter().enumerate() {
        let q = r.get((i + 1) % r.len().max(1)).copied().unwrap_or(*p);
        a += p.cross(q);
    }
    a * 0.5
}

/// Length of the closed ring.
pub(crate) fn perimeter(r: &[Vec2]) -> f64 {
    r.iter().enumerate().map(|(i, p)| p.dist(r.get((i + 1) % r.len().max(1)).copied().unwrap_or(*p))).sum()
}

/// Distance from `p` to the segment `a`–`b`.
pub(crate) fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let l2 = ab.len2();
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + ab * t)
}

/// Parameters `(t, u)` along `a`–`b` and `c`–`d` where they cross, both within the segments
/// (parallel and collinear pairs report nothing).
pub(crate) fn segment_hit(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(f64, f64)> {
    let r = b - a;
    let s = d - c;
    let den = r.cross(s);
    let scale = r.len() * s.len();
    if !(den.abs() > 1e-14 * scale.max(1e-300)) {
        return None;
    }
    let ca = c - a;
    let t = ca.cross(s) / den;
    let u = ca.cross(r) / den;
    const E: f64 = 1e-12;
    ((-E..=1.0 + E).contains(&t) && (-E..=1.0 + E).contains(&u)).then_some((t.clamp(0.0, 1.0), u.clamp(0.0, 1.0)))
}

/// Even-odd containment of `p` in the ring.
pub(crate) fn contains(r: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let n = r.len();
    for i in 0..n {
        let (Some(a), Some(b)) = (r.get(i), r.get((i + 1) % n)) else { continue };
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// The ring without consecutive points closer than `eps` (and without a last point that
/// repeats the first).
pub(crate) fn dedupe(r: &[Vec2], eps: f64) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(r.len());
    for p in r {
        if out.last().is_none_or(|q| q.dist(*p) > eps) {
            out.push(*p);
        }
    }
    while out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| a.dist(*b) <= eps) {
        out.pop();
    }
    out
}

/// Whether every coordinate is finite.
pub(crate) fn all_finite(r: &[Vec2]) -> bool {
    r.iter().all(|p| p.is_finite())
}
