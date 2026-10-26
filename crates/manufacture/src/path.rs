//! Toolpaths: the machine-independent description of where the tool goes, in the setup's
//! work coordinates (millimetres, Z up along the tool axis).

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

/// A drilling cycle at one position. Posts either emit a canned cycle or spell it out.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrillCycle {
    pub x: f64,
    pub y: f64,
    /// Where the hole starts (the surface).
    pub top: f64,
    /// Where the tip ends.
    pub bottom: f64,
    /// The plane the tool rapids down to and back to between holes.
    pub retract: f64,
    /// Peck depth (each peck goes this much deeper than the last); `None` drills in one go.
    pub peck: Option<f64>,
    /// Dwell at the bottom (seconds).
    pub dwell: f64,
    /// Feed (mm/min).
    pub feed: f64,
}

/// One move.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "move", rename_all = "snake_case")]
pub enum Move {
    /// Rapid traverse.
    Rapid { to: Vec3 },
    /// Straight cutting move at `feed` mm/min.
    Feed { to: Vec3, feed: f64 },
    /// A drilling cycle (rapids to `retract` above the position, drills, returns).
    Drill(DrillCycle),
}

/// The moves of one operation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Toolpath {
    pub moves: Vec<Move>,
}

/// What a toolpath amounts to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Stats {
    pub moves: usize,
    pub rapid_length: f64,
    pub feed_length: f64,
    /// Estimated run time (minutes): feed moves at their feed, rapids at the rate given.
    pub minutes: f64,
    pub min: Vec3,
    pub max: Vec3,
}

impl Toolpath {
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    /// The same path with every drilling cycle spelled out as rapids and feeds.
    pub fn expanded(&self) -> Vec<Move> {
        let mut out = Vec::with_capacity(self.moves.len());
        for m in &self.moves {
            match m {
                Move::Drill(c) => expand_cycle(c, &mut out),
                other => out.push(*other),
            }
        }
        out
    }

    /// Lengths, time and extents. `rapid_rate` is the machine's rapid speed (mm/min); the
    /// tool is taken to start at `start`.
    pub fn stats(&self, start: Vec3, rapid_rate: f64) -> Stats {
        let mut at = start;
        let (mut rapid, mut feed_len, mut minutes) = (0.0, 0.0, 0.0);
        let (mut lo, mut hi) = (start, start);
        let rate = if rapid_rate.is_finite() && rapid_rate > 0.0 { rapid_rate } else { 5000.0 };
        let moves = self.expanded();
        for m in &moves {
            let (to, f) = match m {
                Move::Rapid { to } => (*to, None),
                Move::Feed { to, feed } => (*to, Some(*feed)),
                Move::Drill(_) => continue,
            };
            let len = at.dist(to);
            match f {
                None => {
                    rapid += len;
                    minutes += len / rate;
                }
                Some(f) => {
                    feed_len += len;
                    if f > 0.0 {
                        minutes += len / f;
                    }
                }
            }
            lo = lo.min(to);
            hi = hi.max(to);
            at = to;
        }
        Stats { moves: self.moves.len(), rapid_length: rapid, feed_length: feed_len, minutes, min: lo, max: hi }
    }
}

/// A cycle as plain moves: rapid to the retract plane, feed down (in pecks, retracting to the
/// retract plane between them), then rapid out.
fn expand_cycle(c: &DrillCycle, out: &mut Vec<Move>) {
    let at = |z: f64| Vec3::new(c.x, c.y, z);
    out.push(Move::Rapid { to: at(c.retract) });
    let depth = (c.top - c.bottom).max(0.0);
    // Pecks smaller than a ten-thousandth of the depth are taken as no pecking: a hostile value
    // must not turn one hole into millions of moves.
    let peck = c.peck.filter(|p| p.is_finite() && *p >= depth / 10_000.0 && *p < depth);
    let mut reached = 0.0;
    while reached < depth - 1e-12 {
        let next = match peck {
            Some(p) => (reached + p).min(depth),
            None => depth,
        };
        if reached > 0.0 {
            // Back down to just above where the last peck ended.
            out.push(Move::Rapid { to: at((c.top - reached + 0.5).min(c.retract)) });
        }
        out.push(Move::Feed { to: at(c.top - next), feed: c.feed });
        reached = next;
        if reached < depth - 1e-12 {
            out.push(Move::Rapid { to: at(c.retract) });
        }
    }
    out.push(Move::Rapid { to: at(c.retract) });
}
