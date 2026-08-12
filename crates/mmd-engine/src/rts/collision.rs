//! Hard body collision between RTS unit bodies.
//!
//! The horde sim (`crate::sim`) resolves crowding with *soft separation*: a
//! repulsion sum bends a descent vector and two agents may overlap. RTS units
//! do not get that. A unit is a circle of `entity::RTS_UNIT_BODY_RADIUS_CELLS`
//! cells, and no completed [`super::world::RtsWorld::tick`] may leave two of
//! those circles merged.
//!
//! Both predicates here are pure and exact, and both treat **contact as
//! legal**: two bodies whose centres are exactly one summed-radius apart are
//! touching, not penetrating. Penetration is the strict `<` case, tested on
//! squared distances so nothing ever takes a square root.
//!
//! [`moving_circle_hits_point`] is the one that makes the invariant safe at
//! speed: testing only the candidate *endpoint* would let a fast unit step
//! straight over a body it should have hit — the classic tunneling bug — so a
//! candidate is tested as the whole swept segment it traverses.

/// Whether two circular bodies penetrate.
///
/// `false` at exactly `ar + br` apart: touching is legal.
pub fn units_overlap(a: [f32; 2], ar: f32, b: [f32; 2], br: f32) -> bool {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let sum = ar + br;
    dx * dx + dy * dy < sum * sum
}

/// Whether a circle of `radius` sweeping the straight segment `from -> to`
/// ever penetrates the stationary circle of `other_radius` at `other`.
///
/// Exact: the swept region of a circle along a segment is the set of points
/// within `radius` of that segment, so the test is point-to-segment distance
/// against the summed radii — no sampling, no step size, and therefore no
/// tunneling however long the segment is.
pub fn moving_circle_hits_point(
    from: [f32; 2],
    to: [f32; 2],
    radius: f32,
    other: [f32; 2],
    other_radius: f32,
) -> bool {
    let sum = radius + other_radius;
    dist2_point_seg(other, from, to) < sum * sum
}

/// Squared distance from a point to a closed segment. A zero-length segment
/// degenerates to the point-to-point case.
fn dist2_point_seg(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let abx = b[0] - a[0];
    let aby = b[1] - a[1];
    let apx = p[0] - a[0];
    let apy = p[1] - a[1];
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0.0 {
        ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = apx - abx * t;
    let dy = apy - aby * t;
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_is_not_overlap_on_either_axis() {
        assert!(!units_overlap([0.0, 0.0], 3.0, [6.0, 0.0], 3.0));
        assert!(!units_overlap([0.0, 0.0], 3.0, [0.0, 6.0], 3.0));
        assert!(units_overlap([0.0, 0.0], 3.0, [5.9, 0.0], 3.0));
    }

    #[test]
    fn a_zero_length_sweep_is_the_static_test() {
        let p = [4.0, 0.0];
        assert_eq!(
            moving_circle_hits_point([0.0, 0.0], [0.0, 0.0], 3.0, p, 3.0),
            units_overlap([0.0, 0.0], 3.0, p, 3.0)
        );
    }

    #[test]
    fn a_long_sweep_cannot_tunnel() {
        // 1 000 cells in one step, straight through a body at the midpoint.
        assert!(moving_circle_hits_point(
            [0.0, 0.0],
            [1000.0, 0.0],
            3.0,
            [500.0, 0.0],
            3.0
        ));
        // ...and misses one that sits a summed radius off the line.
        assert!(!moving_circle_hits_point(
            [0.0, 0.0],
            [1000.0, 0.0],
            3.0,
            [500.0, 6.0],
            3.0
        ));
    }
}
