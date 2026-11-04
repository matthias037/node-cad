//! A sketch can hold dimensions to the far edge of a circle or arc (the arc condition max):
//! `round_distance`, between ends `a` and `b`, each a point (`{"point": i}`) or a circle or arc
//! measured to its near (`{"near": c}`) or far (`{"far": c}`) edge, saved with its `value` and,
//! for edges that reach past each other, `inside`; and `far` on a `line_circle_distance`, measured
//! to the circle's far edge. Older designs need no transformation: they have no such dimensions,
//! and NØDE CAD 0.21.0-rc.7 saved them under its format, so its designs open as they are. The
//! last release asks for an update instead of meeting a constraint type it doesn't know.
