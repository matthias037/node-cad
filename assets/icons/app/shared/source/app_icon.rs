//! NØDE CAD mark, shared by desktop, packaging and CLI: the NØDE "Ø" (a ring cut by the coral
//! bar) with the ring drawn as a hexagon, a cube's outline seen from a corner, on the NØDE
//! night tile. Units are those of the NØDE OS app icon: a 1024 canvas, the tile 824 wide at
//! 100, the mark in a 100-unit box placed at 262 and scaled 5.
type P = (f32, f32);

const TILE: [u8; 3] = [0x0a, 0x0e, 0x18];
const RING: [u8; 3] = [0xe5, 0xe5, 0xe5];
const BAR: [u8; 3] = [0xe5, 0x6e, 0x6e];

/// Tile: left/top edge, width and corner radius in canvas units.
const TILE_EDGE: f32 = 100.0;
const TILE_SIZE: f32 = 824.0;
const TILE_RADIUS: f32 = 185.0;
/// The mark's box in canvas units: origin and scale (mark units are 0..100).
const MARK_AT: f32 = 262.0;
const MARK_SCALE: f32 = 5.0;
/// The bar, its stroke, and the gap it cuts in the ring (as the NØDE OS mask).
const BAR_FROM: P = (11.0, 89.0);
const BAR_TO: P = (89.0, 11.0);
const BAR_HALF: f32 = 6.5;
const GAP_HALF: f32 = 10.5;
/// The cube outline: circumradius of the hexagon and the strokes' half widths.
const HEX_R: f32 = 41.0;
const LINE_HALF: f32 = 5.5;
const INNER_HALF: f32 = 3.5;

fn segment_distance(p: P, a: P, b: P) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

/// Pointy-top hexagon around the mark's centre: top, upper right, lower right, bottom, lower
/// left, upper left.
fn hexagon() -> [P; 6] {
    let (c, h) = (50.0, HEX_R * 0.5);
    let w = HEX_R * 0.866_025_4;
    [(c, c - HEX_R), (c + w, c - h), (c + w, c + h), (c, c + HEX_R), (c - w, c + h), (c - w, c - h)]
}

fn inside_tile(p: P) -> bool {
    let half = TILE_SIZE / 2.0;
    let centre = TILE_EDGE + half;
    let (qx, qy) = ((p.0 - centre).abs() - (half - TILE_RADIUS), (p.1 - centre).abs() - (half - TILE_RADIUS));
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) <= TILE_RADIUS
}

/// The colour at one point of the canvas, or None outside the tile.
fn colour(p: P, hex: &[P; 6]) -> Option<[u8; 3]> {
    if !inside_tile(p) {
        return None;
    }
    let m = ((p.0 - MARK_AT) / MARK_SCALE, (p.1 - MARK_AT) / MARK_SCALE);
    let bar = segment_distance(m, BAR_FROM, BAR_TO);
    if bar <= BAR_HALF {
        return Some(BAR);
    }
    if bar > GAP_HALF {
        let outline = (0..6).map(|i| segment_distance(m, hex[i], hex[(i + 1) % 6])).fold(f32::MAX, f32::min);
        // The edges from the centre to the upper left corner and to the bottom: the top face.
        let centre = (50.0, 50.0);
        let inner = segment_distance(m, centre, hex[5]).min(segment_distance(m, centre, hex[3]));
        if outline <= LINE_HALF || inner <= INNER_HALF {
            return Some(RING);
        }
    }
    Some(TILE)
}

/// This block is the icon image data
pub fn rgba(size: usize) -> Result<Vec<u8>, String> {
    if !(1..=2048).contains(&size) {
        return Err("icon size must be 1..=2048".into());
    }
    // 4x4 samples per pixel (2x2 for big sizes), averaged to straight alpha = covered share.
    let ss = if size > 512 { 2 } else { 4 };
    let hex = hexagon();
    let step = 1024.0 / (size * ss) as f32;
    let mut rgba = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let (mut sum, mut cnt) = ([0u32; 3], 0u32);
            for j in 0..ss {
                for i in 0..ss {
                    let p = (((x * ss + i) as f32 + 0.5) * step, ((y * ss + j) as f32 + 0.5) * step);
                    if let Some(c) = colour(p, &hex) {
                        cnt += 1;
                        for k in 0..3 {
                            sum[k] += u32::from(c[k]);
                        }
                    }
                }
            }
            if cnt == 0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let a = 255 * cnt / (ss * ss) as u32;
                rgba.extend_from_slice(&[(sum[0] / cnt) as u8, (sum[1] / cnt) as u8, (sum[2] / cnt) as u8, a as u8]);
            }
        }
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    #[test]
    fn mark_has_transparent_corners_and_brand_colours_at_window_and_package_sizes() {
        for size in [16, 24, 32, 48, 64, 128, 256] {
            let pixels = super::rgba(size).expect("valid size");
            assert_eq!(pixels.len(), size * size * 4);
            let px = pixels.as_chunks::<4>().0;
            assert_eq!(px[0][3], 0, "the tile's rounded corner leaves the canvas corner see-through");
            assert!(px.iter().any(|p| p[3] == 255 && p[0] > 200 && p[2] < 140), "the coral bar");
            assert!(px.iter().any(|p| p[3] == 255 && p[0] < 30 && p[2] < 40), "the night tile");
        }
        assert!(super::rgba(0).is_err());
        assert!(super::rgba(2049).is_err());
    }

    /// The bar cuts the outline: where the hexagon's lower left edge meets the bar there is
    /// bar, and away from it the edge is drawn.
    #[test]
    fn bar_cuts_the_outline() {
        let hex = super::hexagon();
        let canvas = |m: super::P| (super::MARK_AT + m.0 * super::MARK_SCALE, super::MARK_AT + m.1 * super::MARK_SCALE);
        let on_edge = |t: f32| (hex[3].0 + (hex[4].0 - hex[3].0) * t, hex[3].1 + (hex[4].1 - hex[3].1) * t);
        let cross = (0..=1000).map(|i| on_edge(i as f32 / 1000.0)).min_by(|a, b| (a.0 + a.1 - 100.0).abs().total_cmp(&(b.0 + b.1 - 100.0).abs()));
        assert_eq!(cross.and_then(|c| super::colour(canvas(c), &hex)), Some(super::BAR));
        assert_eq!(super::colour(canvas(on_edge(0.0)), &hex), Some(super::RING), "the edge away from the bar");
        assert_eq!(super::colour((0.0, 0.0), &hex), None);
    }
}
