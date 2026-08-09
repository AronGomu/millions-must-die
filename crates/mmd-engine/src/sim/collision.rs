//! Agent-agent soft separation.
//!
//! The model is steering, not resolution: each agent sums a repulsion vector
//! from the neighbours overlapping its body, that sum is added to the
//! flow-field descent vector, and the *blended* direction is what the agent
//! walks along at its unchanged speed. Overlap is therefore reduced, never
//! forbidden — no caller may claim a zero-overlap guarantee.
//!
//! Determinism: the neighbour scan visits each row of the window as one
//! contiguous run, in a fixed order, and each bin's
//! agents in ascending index order (`super::spatial::SpatialGrid`), the
//! coincidence tie-break is a table lookup keyed on the index pair, and nothing
//! reads a clock or an RNG.

use crate::scenario::{COLLISION_Q8, Scenario};

use super::spatial::SpatialGrid;

/// Neighbours one agent may accumulate in a tick.
///
/// A cap is required, not an optimisation: 50 000 agents seeded onto 127 spawn
/// cells start ~394 deep on one coordinate, and an uncapped scan would be
/// quadratic in that stack. Capping bounds the *contributions* one agent
/// accumulates and, because the scan order is fixed, keeps them reproducible.
///
/// It does not bound the candidates examined: a neighbour outside contact is
/// skipped without spending a slot, so an agent whose 3x3 window holds a dense
/// but mostly out-of-contact cluster still walks that whole window.
///
/// Do not "fix" the cap after observing that a freshly spawned stack deeper
/// than this moves in lockstep for the first few ticks: while every member has
/// a full complement of coincident neighbours, index-aligned sub-blocks share a
/// capped neighbour set and therefore share a tie-break sum. That resolves
/// itself — as soon as an agent has fewer than `MAX_SEPARATION_NEIGHBORS`
/// neighbours still in contact, the `(i ^ j)` tie-break re-enters and the block
/// breaks up.
pub const MAX_SEPARATION_NEIGHBORS: usize = 8;

/// Below this squared distance two agents count as coincident and the tie-break
/// table replaces the (undefined) direction between them.
pub const COINCIDENT_EPS2: f32 = 1e-12;

/// Deterministic push directions for coincident agents, keyed by `(i ^ j) & 15`
/// and signed by `i < j`, so a pair pushes equally and oppositely — *provided
/// both members saw each other*. [`MAX_SEPARATION_NEIGHBORS`] can truncate one
/// side of a pair, and then the two pushes do not cancel. That is accepted:
/// this is steering, and nothing here conserves momentum.
/// Sixteen unit vectors at 22.5-degree steps.
///
/// Written out as literals rather than assembled from `std::f32::consts`: the
/// four diagonal entries are `FRAC_1_SQRT_2` and the rest are not, so naming
/// only those would obscure the table's one job — being a readable ring of
/// sixteen evenly spaced directions. These values reach the state hash, so they
/// are also deliberately fixed rather than computed.
#[allow(clippy::approx_constant)]
pub const SEPARATION_DIR16: [(f32, f32); 16] = [
    (1.0, 0.0),
    (0.923_879_5, 0.382_683_43),
    (0.707_106_78, 0.707_106_78),
    (0.382_683_43, 0.923_879_5),
    (0.0, 1.0),
    (-0.382_683_43, 0.923_879_5),
    (-0.707_106_78, 0.707_106_78),
    (-0.923_879_5, 0.382_683_43),
    (-1.0, 0.0),
    (-0.923_879_5, -0.382_683_43),
    (-0.707_106_78, -0.707_106_78),
    (-0.382_683_43, -0.923_879_5),
    (0.0, -1.0),
    (0.382_683_43, -0.923_879_5),
    (0.707_106_78, -0.707_106_78),
    (0.923_879_5, -0.382_683_43),
];

/// Body size and steering weight for one scenario.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionParams {
    /// Body radius in cells. Two agents are in contact at twice this.
    pub radius_cells: f32,
    /// Weight of the repulsion sum against the unit flow vector.
    pub strength: f32,
    /// Ticks the separation pass is spread over. 1 = identity.
    pub phases: u32,
    /// Distinct push-priority classes. 1 = identity.
    pub mass_classes: u32,
    /// Worker threads for the separation pass. 1 = identity.
    pub threads: u32,
}

impl CollisionParams {
    /// No body: the separation pass is skipped entirely and the tick is
    /// bit-identical to a pure flow-field walk.
    pub const NONE: Self = Self {
        radius_cells: 0.0,
        strength: 0.0,
        phases: 1,
        mass_classes: 1,
        threads: 1,
    };

    /// Convert from the scenario's Q8 fixed point. Exact: the divisor is a
    /// power of two. The tuning knobs stay at their identity values; only
    /// `from_scenario` reads a scenario's.
    pub fn from_q8(radius_q8: u32, strength_q8: u32) -> Self {
        Self {
            radius_cells: radius_q8 as f32 / COLLISION_Q8 as f32,
            strength: strength_q8 as f32 / COLLISION_Q8 as f32,
            phases: 1,
            mass_classes: 1,
            threads: 1,
        }
    }

    pub fn from_scenario(scenario: &Scenario) -> Self {
        Self {
            radius_cells: scenario.collision_radius_q8() as f32 / COLLISION_Q8 as f32,
            strength: scenario.separation_strength_q8() as f32 / COLLISION_Q8 as f32,
            phases: scenario.separation_phases(),
            mass_classes: scenario.mass_class_count(),
            threads: scenario.separation_threads(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.radius_cells > 0.0 && self.strength > 0.0
    }

    /// Bin edge that makes a 3x3 bin scan cover every possible contact.
    pub fn bin_size_cells(&self) -> f32 {
        (2.0 * self.radius_cells).max(1.0)
    }
}

/// Write the repulsion sum for the agents of `phase` into `sep_x` / `sep_y`,
/// leaving every other entry exactly as it was.
///
/// An agent belongs to `i % phases`. The stride, rather than a contiguous
/// block, is deliberate: agent index correlates with spawn cell, so a
/// blocked split would refresh one region of the crowd at a time.
///
/// `phases == 1` visits every agent and is bit-identical to the unamortised
/// pass.
///
/// `grid` must have been rebuilt from the same positions this tick, every slice
/// must be the same length, and `radius_cells` must be positive — a bodyless
/// scenario skips this pass rather than calling it with zero.
///
/// Each contribution is scaled by `mass[j] / mass[i]`. With one class every
/// mass is 1 and the scale is exactly 1.0, so the pass is bit-identical to
/// the unweighted one.
///
/// # Panics
/// If `phases == 0` or `phase >= phases`.
#[allow(clippy::too_many_arguments)]
pub fn accumulate_separation_phase(
    x: &[f32],
    y: &[f32],
    grid: &SpatialGrid,
    radius_cells: f32,
    mass: &[u8],
    inv_mass: &[f32],
    phases: u32,
    phase: u32,
    sep_x: &mut [f32],
    sep_y: &mut [f32],
) {
    let hi = x.len();
    accumulate_separation_range(
        x,
        y,
        grid,
        radius_cells,
        mass,
        inv_mass,
        phases,
        phase,
        0,
        hi,
        sep_x,
        sep_y,
    );
}

/// As [`accumulate_separation_phase`], restricted to agent indices in `lo..hi`.
///
/// Splitting by index range is what makes this pass safe to run on several
/// threads: each output element is written by exactly one caller, from
/// immutable inputs, in the same intra-agent order it would have had serially.
/// No partial sum crosses a range boundary, so no floating-point reassociation
/// is possible and the result does not depend on how the range was cut.
///
/// The **input** slices are always the whole population: a neighbour scan has
/// to read agents outside this caller's range, and the grid stores absolute
/// indices. The **output** slices cover `lo..hi` only, so `sep_x[i - lo]` is
/// agent `i`'s push. That asymmetry is load-bearing rather than cosmetic —
/// handing every thread a `&mut [f32]` over the whole buffer would be two
/// overlapping unique references to the same memory, which is undefined
/// behaviour even when the elements each thread touches are disjoint. Slicing
/// the output makes the disjointness a property of the references themselves.
///
/// # Panics
/// If `phases == 0`, `phase >= phases`, or `lo > hi`, or `hi > x.len()`.
#[allow(clippy::too_many_arguments)]
pub fn accumulate_separation_range(
    x: &[f32],
    y: &[f32],
    grid: &SpatialGrid,
    radius_cells: f32,
    mass: &[u8],
    inv_mass: &[f32],
    phases: u32,
    phase: u32,
    lo: usize,
    hi: usize,
    sep_x: &mut [f32],
    sep_y: &mut [f32],
) {
    let n = x.len();
    debug_assert_eq!(y.len(), n);
    debug_assert_eq!(grid.len(), n);
    debug_assert_eq!(mass.len(), n);
    debug_assert_eq!(inv_mass.len(), n);
    // Contact distance is the divisor below; a zero radius would poison every
    // sum with an infinity.
    debug_assert!(radius_cells > 0.0, "radius must be positive");
    assert!(phases > 0, "phases must be >= 1");
    assert!(phase < phases, "phase {phase} out of range for {phases}");
    assert!(hi <= n && lo <= hi, "range {lo}..{hi} out of bounds");
    debug_assert_eq!(sep_x.len(), hi - lo);
    debug_assert_eq!(sep_y.len(), hi - lo);

    let contact = 2.0 * radius_cells;
    let contact2 = contact * contact;
    let inv_contact = 1.0 / contact;
    let last_col = grid.cols() - 1;
    let last_row = grid.rows() - 1;

    let step = phases as usize;
    // The first index of this phase at or after `lo`, i.e. the smallest
    // `i >= lo` with `i % step == phase`. Written as
    // `lo + ((phase - lo) mod step)` with the subtraction kept non-negative,
    // since `lo % step < step` and `phase < step`. With `lo == 0` this is
    // exactly `phase`, so the unsplit walk is unchanged.
    let mut i = lo + ((phase as usize + step - lo % step) % step);
    while i < hi {
        let px = x[i];
        let py = y[i];
        let inv_mi = inv_mass[i];
        let (bx, by) = grid.bin_of(px, py);
        let bx0 = bx.saturating_sub(1);
        let bx1 = (bx + 1).min(last_col);
        let by0 = by.saturating_sub(1);
        let by1 = (by + 1).min(last_row);

        // A window holding one agent holds only this one. The scan below would
        // find no `j` and write zeroes, so write them without touching `items`.
        let mut window = 0usize;
        for cy in by0..=by1 {
            window += grid.agents_in_bin_row(bx0, bx1, cy).len();
        }
        if window <= 1 {
            sep_x[i - lo] = 0.0;
            sep_y[i - lo] = 0.0;
            i += step;
            continue;
        }

        let mut sx = 0.0f32;
        let mut sy = 0.0f32;
        let mut taken = 0usize;

        'scan: for cy in by0..=by1 {
            for &raw in grid.agents_in_bin_row(bx0, bx1, cy) {
                let j = raw as usize;
                if j == i {
                    continue;
                }
                let dx = px - x[j];
                let dy = py - y[j];
                let d2 = dx * dx + dy * dy;
                if d2 >= contact2 {
                    continue;
                }
                let scale = mass[j] as f32 * inv_mi;
                if d2 <= COINCIDENT_EPS2 {
                    // No direction exists between two identical points.
                    // The table gives one that is stable across runs and
                    // opposite for the two members of the pair.
                    let (ux, uy) = SEPARATION_DIR16[(i ^ j) & 15];
                    let sign = if i < j { 1.0 } else { -1.0 };
                    sx += sign * ux * scale;
                    sy += sign * uy * scale;
                } else {
                    let d = d2.sqrt();
                    // Linear falloff: full push at coincidence, none at
                    // contact distance.
                    let w = (contact - d) * inv_contact;
                    let inv_d = 1.0 / d;
                    sx += dx * inv_d * w * scale;
                    sy += dy * inv_d * w * scale;
                }
                taken += 1;
                if taken == MAX_SEPARATION_NEIGHBORS {
                    break 'scan;
                }
            }
        }

        sep_x[i - lo] = sx;
        sep_y[i - lo] = sy;
        i += step;
    }
}

/// Write every agent's repulsion sum. Equivalent to
/// [`accumulate_separation_phase`] with `phases = 1, phase = 0`.
#[allow(clippy::too_many_arguments)]
pub fn accumulate_separation(
    x: &[f32],
    y: &[f32],
    grid: &SpatialGrid,
    radius_cells: f32,
    mass: &[u8],
    inv_mass: &[f32],
    sep_x: &mut [f32],
    sep_y: &mut [f32],
) {
    accumulate_separation_phase(x, y, grid, radius_cells, mass, inv_mass, 1, 0, sep_x, sep_y);
}
