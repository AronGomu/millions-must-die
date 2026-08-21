//! Shared behavioural assertions for the T30 sim/nav suites.
//!
//! These helpers exist so `simulation.rs` and `flow_field.rs` state the same
//! invariants the same way: an agent is *in bounds* when its cell is inside the
//! world rect, *finite* when neither coordinate is NaN/inf, and has *made
//! progress* when the flow-field integration cost under it has fallen below the
//! cost at its spawn. Nothing here reads a clock or asserts on timing — the
//! merge gate (`docs/05-testing.md`) forbids it.
//!
//! [`Tracker`] is the workhorse: it steps a [`Harness`] tick by tick and
//! records what the run did, so one bounded run can answer several independent
//! questions (recycled fraction, stuck agents, obstacle entries, per-group
//! progress) without each test re-running the sim.

// Each integration binary uses a subset of these helpers; `mod common` is
// compiled per binary, so unused-in-this-binary is expected, not dead code.
#![allow(dead_code)]

use mmd_engine::nav::flow_field::{COST_OBSTACLE, COST_UNREACHABLE};
use mmd_engine::sim::{SPEED_CELLS_PER_SEC, TICK_DT};
use mmd_engine::testkit::Harness;

/// Distance one agent covers in one tick (cells).
pub fn step_len() -> f32 {
    SPEED_CELLS_PER_SEC * TICK_DT
}

/// A jump larger than this can only be a recycle: normal movement is capped at
/// one `step_len` per tick, so twice that is unreachable by walking.
pub fn recycle_jump_threshold() -> f32 {
    step_len() * 2.0
}

/// Cell under a continuous position, or `None` when the position is outside the
/// world rect (or not finite).
pub fn cell_of(x: f32, y: f32, width: u32, height: u32) -> Option<(u32, u32)> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    if x < 0.0 || y < 0.0 || x >= width as f32 || y >= height as f32 {
        return None;
    }
    Some((x.floor() as u32, y.floor() as u32))
}

/// Every agent is finite and inside the world rect. Panics naming the offender.
pub fn assert_positions_finite_and_in_bounds(h: &Harness, context: &str) {
    let (w, ht) = (h.scenario().width(), h.scenario().height());
    let v = h.agents();
    for i in 0..v.x.len() {
        let (x, y) = (v.x[i], v.y[i]);
        assert!(
            x.is_finite() && y.is_finite(),
            "{context}: agent {i} left the numeric domain at ({x}, {y})"
        );
        assert!(
            cell_of(x, y, w, ht).is_some(),
            "{context}: agent {i} left the {w}x{ht} world rect at ({x}, {y})"
        );
    }
}

/// Agents currently standing on an obstacle cell.
pub fn agents_in_obstacles(h: &Harness) -> Vec<(usize, f32, f32)> {
    let (w, ht) = (h.scenario().width(), h.scenario().height());
    let v = h.agents();
    let mut out = Vec::new();
    for i in 0..v.x.len() {
        if let Some((cx, cy)) = cell_of(v.x[i], v.y[i], w, ht)
            && h.flow_field().cost_at(cx, cy) == COST_OBSTACLE
        {
            out.push((i, v.x[i], v.y[i]));
        }
    }
    out
}

/// Integration cost under an agent — the routing distance to the destination,
/// which is what "progress" means on a grid with walls (straight-line distance
/// is not: an agent walking around a wall moves *away* in Euclidean terms).
/// Out-of-world or non-finite positions return `None`.
pub fn cost_under(h: &Harness, x: f32, y: f32) -> Option<u32> {
    let (w, ht) = (h.scenario().width(), h.scenario().height());
    cell_of(x, y, w, ht).map(|(cx, cy)| h.flow_field().cost_at(cx, cy))
}

/// Per-agent record of one bounded run.
///
/// Recycles are detected *geometrically* (a jump no walk could produce), not by
/// reading the sim's counter, so the counter itself can be checked against an
/// independent observation instead of against itself.
pub struct Tracker {
    width: u32,
    height: u32,
    spawn_cells: Vec<(u32, u32)>,
    /// Spawn group per agent: index into `spawn_cells` of the cell it started on.
    group: Vec<usize>,
    spawn_cost: Vec<u32>,
    min_cost: Vec<u32>,
    recycles: Vec<u32>,
    moved: Vec<bool>,
    prev: Vec<(f32, f32)>,
    ticks: u64,
    obstacle_samples: u64,
    bounds_violations: u64,
    /// Mean integration cost over all agents, one entry per tick.
    mean_cost: Vec<f64>,
    /// First tick on which any agent recycled (aggregate progress is only
    /// meaningful before the first agent is teleported back to a spawn).
    first_recycle_tick: Option<u64>,
    alive_history: Vec<usize>,
}

impl Tracker {
    pub fn new(h: &Harness) -> Self {
        let width = h.scenario().width();
        let height = h.scenario().height();
        let spawn_cells: Vec<(u32, u32)> = h
            .scenario()
            .spawn_cells()
            .iter()
            .map(|c| (c.x, c.y))
            .collect();
        let v = h.agents();
        let n = v.x.len();
        let mut group = Vec::with_capacity(n);
        let mut spawn_cost = Vec::with_capacity(n);
        let mut prev = Vec::with_capacity(n);
        for i in 0..n {
            let cell = cell_of(v.x[i], v.y[i], width, height)
                .unwrap_or_else(|| panic!("agent {i} starts outside the world"));
            group.push(
                spawn_cells
                    .iter()
                    .position(|s| *s == cell)
                    .unwrap_or_else(|| panic!("agent {i} starts off-spawn at {cell:?}")),
            );
            spawn_cost.push(h.flow_field().cost_at(cell.0, cell.1));
            prev.push((v.x[i], v.y[i]));
        }
        Self {
            width,
            height,
            spawn_cells,
            group,
            min_cost: spawn_cost.clone(),
            spawn_cost,
            recycles: vec![0; n],
            moved: vec![false; n],
            prev,
            ticks: 0,
            obstacle_samples: 0,
            bounds_violations: 0,
            mean_cost: Vec::new(),
            first_recycle_tick: None,
            alive_history: Vec::new(),
        }
    }

    /// Step the harness `ticks` times, sampling every agent every tick.
    pub fn run(&mut self, h: &mut Harness, ticks: u64) {
        let jump = recycle_jump_threshold();
        for _ in 0..ticks {
            h.step_exact(1);
            self.ticks += 1;
            let n = h.alive_count();
            self.alive_history.push(n);
            let mut sum_cost = 0f64;
            let mut counted = 0u32;
            {
                let v = h.agents();
                for i in 0..n.min(self.prev.len()) {
                    let (x, y) = (v.x[i], v.y[i]);
                    match cell_of(x, y, self.width, self.height) {
                        None => {
                            self.bounds_violations += 1;
                            continue;
                        }
                        Some((cx, cy)) => {
                            let c = h.flow_field().cost_at(cx, cy);
                            if c == COST_OBSTACLE {
                                self.obstacle_samples += 1;
                            } else if c < COST_UNREACHABLE {
                                sum_cost += c as f64;
                                counted += 1;
                                self.min_cost[i] = self.min_cost[i].min(c);
                            }
                        }
                    }
                    let d = ((x - self.prev[i].0).powi(2) + (y - self.prev[i].1).powi(2)).sqrt();
                    if d > jump {
                        self.recycles[i] += 1;
                        if self.first_recycle_tick.is_none() {
                            self.first_recycle_tick = Some(self.ticks);
                        }
                    } else if d > 0.0 {
                        self.moved[i] = true;
                    }
                    self.prev[i] = (x, y);
                }
            }
            if counted > 0 {
                self.mean_cost.push(sum_cost / counted as f64);
            }
        }
    }

    pub fn agent_count(&self) -> usize {
        self.recycles.len()
    }

    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    pub fn recycles(&self) -> &[u32] {
        &self.recycles
    }

    /// Total recycles observed geometrically, independent of the sim counter.
    pub fn observed_recycles(&self) -> u64 {
        self.recycles.iter().map(|&r| u64::from(r)).sum()
    }

    /// Agents that reached the destination and were recycled at least once.
    pub fn agents_recycled_at_least_once(&self) -> usize {
        self.recycles.iter().filter(|&&r| r > 0).count()
    }

    pub fn recycled_fraction(&self) -> f64 {
        self.agents_recycled_at_least_once() as f64 / self.agent_count() as f64
    }

    /// Agents that never moved a single tick of the run — the "wedged against a
    /// wall forever" failure mode.
    pub fn never_moved(&self) -> Vec<usize> {
        (0..self.agent_count())
            .filter(|&i| !self.moved[i])
            .collect()
    }

    pub fn obstacle_samples(&self) -> u64 {
        self.obstacle_samples
    }

    pub fn bounds_violations(&self) -> u64 {
        self.bounds_violations
    }

    pub fn alive_history(&self) -> &[usize] {
        &self.alive_history
    }

    pub fn first_recycle_tick(&self) -> Option<u64> {
        self.first_recycle_tick
    }

    pub fn group_count(&self) -> usize {
        self.spawn_cells.len()
    }

    pub fn spawn_cell(&self, group: usize) -> (u32, u32) {
        self.spawn_cells[group]
    }

    pub fn group_members(&self, group: usize) -> Vec<usize> {
        (0..self.agent_count())
            .filter(|&i| self.group[i] == group)
            .collect()
    }

    /// Mean fraction of the spawn's routing cost this group closed: 0.0 = the
    /// group never got closer to the destination than its spawn, 1.0 = it
    /// reached the destination.
    pub fn group_mean_progress(&self, group: usize) -> f64 {
        let members = self.group_members(group);
        assert!(!members.is_empty(), "group {group} has no agents");
        let sum: f64 = members
            .iter()
            .map(|&i| {
                let closed = self.spawn_cost[i].saturating_sub(self.min_cost[i]) as f64;
                closed / self.spawn_cost[i].max(1) as f64
            })
            .sum();
        sum / members.len() as f64
    }

    pub fn group_recycles(&self, group: usize) -> u64 {
        self.group_members(group)
            .iter()
            .map(|&i| u64::from(self.recycles[i]))
            .sum()
    }

    /// Ticks on which the mean routing cost rose instead of falling, counted
    /// only over the stretch that ends *before* the first recycle tick — from
    /// that tick on, teleporting an arrival back to its spawn legitimately
    /// raises the mean (`mean_cost[i]` is the mean after tick `i + 1`, so the
    /// first recycle tick maps to index `first_recycle_tick - 1`).
    pub fn mean_cost_increases(&self) -> Vec<(u64, f64, f64)> {
        let horizon = match self.first_recycle_tick {
            Some(t) => (t as usize).saturating_sub(1),
            None => self.mean_cost.len(),
        };
        let mut out = Vec::new();
        for t in 1..horizon.min(self.mean_cost.len()) {
            if self.mean_cost[t] > self.mean_cost[t - 1] {
                out.push((t as u64 + 1, self.mean_cost[t - 1], self.mean_cost[t]));
            }
        }
        out
    }

    pub fn mean_cost_series(&self) -> &[f64] {
        &self.mean_cost
    }

    /// Longest run of consecutive ticks, before the first recycle, during which
    /// the mean routing cost never improved on the best value seen so far.
    ///
    /// Complements the strict tick-by-tick monotonicity claim rather than
    /// replacing it. With agent-agent separation the mean *may* legitimately
    /// rise for a few ticks while a spawn stack pushes itself apart; what must
    /// stay true regardless is that the horde never *stalls* — never goes a
    /// long stretch closing no distance at all.
    ///
    /// Returns `0` when the pre-recycle window is shorter than two ticks, so a
    /// caller must check that window itself before trusting a low result — see
    /// `aggregate_progress_never_stalls`.
    pub fn longest_progress_stall(&self) -> u64 {
        let horizon = match self.first_recycle_tick {
            Some(t) => (t as usize).saturating_sub(1),
            None => self.mean_cost.len(),
        }
        .min(self.mean_cost.len());
        if horizon == 0 {
            return 0;
        }
        let mut best = self.mean_cost[0];
        let mut run = 0u64;
        let mut worst = 0u64;
        for t in 1..horizon {
            if self.mean_cost[t] < best {
                best = self.mean_cost[t];
                run = 0;
            } else {
                run += 1;
                worst = worst.max(run);
            }
        }
        worst
    }
}

// ---------------------------------------------------------------------------
// RTS pocket scenes (T6)
// ---------------------------------------------------------------------------
//
// Body-safe production and construction only *wait* when the grid holds no
// legal free body centre at all, and the tracked 320 x 320 scene is far too
// open for that: there is always somewhere else to put a unit. These synthetic
// scenes are the smallest shapes that pin the waiting branches — a pocket with
// exactly one legal body centre, and a pocket a finished Depot swallows whole.

/// Grid edge of every pocket scene below.
pub const POCKET_GRID: u32 = 48;

/// Half-open cell rectangle `[x0, x1) x [y0, y1)`.
pub type OpenRect = (u32, u32, u32, u32);

/// The HQ's own 12 x 12 footprint, which `StaticNav` stamps solid at load.
const POCKET_HQ: OpenRect = (0, 24, 0, 24);
/// A 4-cell corridor: raw-walkable, so validation's point-agent reachability
/// passes, but far too narrow for a 3-cell body centre.
const POCKET_CORRIDOR: OpenRect = (24, 28, 8, 12);

/// A valid RTS scenario whose only open ground is `open`, plus one spawn cell
/// and one destination.
fn pocket_spec(
    open: &[OpenRect],
    spawn: mmd_engine::scenario::Cell,
    destination: mmd_engine::scenario::Cell,
) -> mmd_engine::scenario::ScenarioSpec {
    use mmd_engine::scenario::{Cell, RtsSpec, ScenarioSpec};

    let mut obstacle_cells = Vec::new();
    for y in 0..POCKET_GRID {
        for x in 0..POCKET_GRID {
            let inside = open
                .iter()
                .any(|&(x0, x1, y0, y1)| x >= x0 && x < x1 && y >= y0 && y < y1);
            if !inside {
                obstacle_cells.push(x + y * POCKET_GRID);
            }
        }
    }
    ScenarioSpec {
        version: "rts_prototype_v1".to_string(),
        width: POCKET_GRID,
        height: POCKET_GRID,
        cell_size_px: 4,
        sprite_size_px: 48,
        hard_agent_count: 0,
        stretch_agent_count: 0,
        seed: 1,
        destination,
        spawn_cells: vec![spawn],
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 0,
        separation_strength_q8: 0,
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
        obstacle_cells,
        rts: Some(RtsSpec {
            start_crystal: 500,
            start_gas: 100,
            start_supply_cap: 10,
            hq_cell: Cell { x: 0, y: 0 },
            // Both nodes sit in the corridor: raw-walkable and reachable, and
            // no body can stand there anyway, so they never add or remove a
            // legal centre.
            crystal_nodes: vec![Cell { x: 25, y: 9 }],
            gas_nodes: vec![Cell { x: 26, y: 10 }],
            enemies: None,
            buildings: vec![],
            start_units: vec![],
        }),
    }
}

/// A scene whose grid holds **exactly one** legal body centre, `(31.5, 9.5)`,
/// which the single seeded worker occupies: a 7 x 7 pocket is the smallest
/// open square a 3-cell body fits in, and it fits in exactly one place.
///
/// Production at the HQ therefore has nowhere at all to put a finished unit
/// until that worker is despawned.
pub fn one_free_centre_spec() -> mmd_engine::scenario::ScenarioSpec {
    use mmd_engine::scenario::Cell;
    pocket_spec(
        &[POCKET_HQ, POCKET_CORRIDOR, (28, 35, 6, 13)],
        Cell { x: 30, y: 9 },
        Cell { x: 31, y: 9 },
    )
}

/// The one legal body centre of [`one_free_centre_spec`].
pub const ONE_FREE_CENTRE: [f32; 2] = [31.5, 9.5];

/// A scene with a 9 x 9 pocket — nine legal body centres, every one of them
/// inside the 8 x 8 footprint a Depot at [`SEALED_SITE_MIN`] would occupy. The
/// site is walkable while it builds and swallows every legal centre on the
/// grid the moment it finishes, so its completion can never be evacuated.
pub fn sealed_site_spec() -> mmd_engine::scenario::ScenarioSpec {
    use mmd_engine::scenario::Cell;
    pocket_spec(
        &[POCKET_HQ, POCKET_CORRIDOR, (28, 37, 6, 15)],
        Cell { x: 32, y: 10 },
        Cell { x: 32, y: 10 },
    )
}

/// Minimum corner of the Depot footprint [`sealed_site_spec`] is shaped for.
pub const SEALED_SITE_MIN: mmd_engine::scenario::Cell = mmd_engine::scenario::Cell { x: 28, y: 6 };
