//! The phase-1 RTS game state: seeded from a scenario, ticked deterministically.

use sha2::{Digest, Sha256};

use crate::nav::field_pool::{FieldPool, FieldPoolError};
use crate::render::{Camera, IsoView, VIEW_HEIGHT, VIEW_WIDTH, screen_axes_to_cells};
use crate::scenario::{self, Cell, Scenario};
use crate::sim::{TICK_DT, dir_from_vector};

use super::build::{
    Placement, PlacementError, build_ticks, building_cost, placement_valid, supply_grant,
};
use super::collision::{moving_circle_hits_point, units_overlap};
use super::economy::{
    GATHER_TICKS, Resources, Supply, WORKER_CARRY_CAPACITY, WORKER_SUPPLY_COST, node_amount,
    supply_cost,
};
use super::entity::{
    BuildingKind, EntityId, EntityKind, EntityStore, MAX_ENTITIES, OWNER_NEUTRAL, OWNER_PLAYER,
    RTS_UNIT_BODY_DIAMETER_CELLS, RTS_UNIT_BODY_RADIUS_CELLS, ResourceKind, UnitKind,
};
use super::formation::{
    FORMATION_ARRIVAL_CELLS, FormationError, FormationGoal, FormationScratch,
    nearest_body_clear_cell,
};
use super::orders::{
    GatherPhase, Order, OrderTable, SOLDIER_SPEED_CELLS_PER_SEC, WORKER_SPEED_CELLS_PER_SEC,
    adaptive_reach, dist2, entity_approach_cell, rect_distance, step_admissible, unit_speed,
};
use super::production::{ProduceError, ProductionQueue, ProductionTable, can_produce, unit_cost};
use super::selection::{MAX_SELECTION, Pick, Selection, box_select, footprint_min, pick_at};
use super::static_nav::{StaticNav, circle_clear_of_cell_rect};

/// What kind of order a context click resolved a unit into. See
/// [`RtsWorld::issue_context_order_at`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssuedOrder {
    Move,
    Gather,
    Build,
}

/// What one group command is pointed at — the only thing that differs between
/// a ground move, a gather and a build, once [`RtsWorld::order_group`] has the
/// group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupTarget {
    /// A cell of open ground, which snaps to the nearest legal body position.
    Ground(Cell),
    /// A resource node: workers mine it, anything else forms up around it.
    Node(EntityId),
    /// A building under construction: workers attend it, anything else is
    /// rejected outright.
    Site(EntityId),
}

/// One unit's outcome from a context-order click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitOrderReceipt {
    pub id: EntityId,
    pub order: IssuedOrder,
}

/// Why a context-order click issued fewer orders than the selection's size.
///
/// Set only when at least one selected unit was rejected outright rather than
/// simply not eligible for this specific target (a per-unit rejection, e.g. a
/// Soldier at a build site, needs no shared reason — [`ContextOrderResult::rejected`]
/// already counts it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextOrderReason {
    /// Nothing was selected; the click still resolved a [`Pick`] but issued
    /// no orders.
    EmptySelection,
    /// No navigation field could be built to the target cell.
    Unreachable,
    /// The click landed outside the scenario grid.
    NoTargetCell,
    /// The grid holds fewer legal formation slots around the target than the
    /// selection has members, so the whole order was refused — see
    /// [`super::FormationError::NoFormationSpace`].
    NoFormationSpace,
}

/// The outcome of [`RtsWorld::issue_context_order_at`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextOrderResult {
    pub pick: Pick,
    pub accepted: usize,
    pub rejected: usize,
    pub reason: Option<ContextOrderReason>,
}

/// Reusable per-call receipt buffer for [`RtsWorld::issue_context_order_at`].
///
/// Reserved to [`MAX_SELECTION`] at construction; every call clears and
/// refills it in place, so issuing orders allocates nothing after `new`.
#[derive(Debug)]
pub struct OrderReceiptBuffer {
    receipts: Vec<UnitOrderReceipt>,
}

impl Default for OrderReceiptBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl OrderReceiptBuffer {
    pub fn new() -> Self {
        Self {
            receipts: Vec::with_capacity(MAX_SELECTION),
        }
    }

    pub fn clear(&mut self) {
        self.receipts.clear();
    }

    fn push(&mut self, id: EntityId, order: IssuedOrder) {
        self.receipts.push(UnitOrderReceipt { id, order });
    }

    /// Every receipt from the most recent call, ascending by entity slot —
    /// the order [`Selection::ids`] iterates in.
    pub fn as_slice(&self) -> &[UnitOrderReceipt] {
        &self.receipts
    }

    /// Current allocation, in receipts. Tests pin this across repeated calls.
    pub fn capacity(&self) -> usize {
        self.receipts.capacity()
    }
}

/// Starting amount in a freshly seeded Crystal node.
pub const NODE_CRYSTAL_AMOUNT: u32 = 1_500;
/// Starting amount in a freshly seeded Gas node.
pub const NODE_GAS_AMOUNT: u32 = 2_500;

/// Failures building an [`RtsWorld`].
#[derive(Debug, thiserror::Error)]
pub enum RtsWorldError {
    #[error(transparent)]
    Scenario(#[from] crate::scenario::ScenarioError),
    #[error("scenario {version} carries no rts block; the rts world needs one")]
    NotAnRtsScene { version: String },
    #[error("entity store full while seeding: {what}")]
    StoreFull { what: String },
    #[error("navigation pool: {0}")]
    Nav(#[from] FieldPoolError),
    #[error("static navigation: {0}")]
    StaticNav(#[from] super::static_nav::StaticNavError),
    #[error("no legal collision-free position exists for a starting unit")]
    NoFreeUnitPosition,
}

/// A failure inside one [`RtsWorld::tick`] that the tick cannot signal by
/// returning — it is stashed on the world and read back with
/// [`RtsWorld::last_tick_error`].
///
/// `Copy` and payload-free on purpose: a tick error is world state that a
/// replay must reproduce exactly, not a place to carry a formatted string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TickError {
    /// A tick started with two unit bodies merged and the grid had no legal
    /// free centre to repair one of them into. Only
    /// [`RtsWorld::force_position_for_test`] and a raw store mutation through
    /// [`RtsWorld::entities_mut`] can produce that, and both arm a repair pass
    /// the shipping build does not compile — so a shipping tick never sets
    /// this. The movement system is skipped for that tick rather than
    /// compounding an illegal state, the repair stays armed so the next tick
    /// retries, and the overlap is reported here rather than silently kept.
    #[error("a merged unit body could not be repaired: no legal free position exists")]
    UnrepairableOverlap,
}

/// The phase-1 RTS game state.
#[derive(Debug)]
pub struct RtsWorld {
    scenario: Scenario,
    entities: EntityStore,
    resources: Resources,
    supply: Supply,
    tick_index: u64,
    start_hq: Option<EntityId>,
    static_nav: StaticNav,
    nav: FieldPool,
    orders: OrderTable,
    /// Working storage for one formation plan, reserved at load: a group order
    /// arrives on a click and must not allocate.
    formation: FormationScratch,
    /// Live-slot buffer the per-tick sweeps reuse. Reserved to
    /// [`MAX_ENTITIES`] so a tick never grows it.
    live_scratch: Vec<usize>,
    /// Live **unit** slots, ascending, refilled every tick by the movement
    /// system. Reserved to [`MAX_ENTITIES`] so a tick never grows it.
    unit_scratch: Vec<usize>,
    /// Parallel to [`Self::unit_scratch`]: the collision body position of
    /// each unit *right now* — already committed for a unit the movement
    /// system has processed this tick, still last tick's for one it has not.
    /// Reserved to [`MAX_ENTITIES`] so a tick never grows it.
    candidate_pos: Vec<[f32; 2]>,
    /// Parallel to [`Self::unit_scratch`]: has this body already been pushed
    /// aside by a mover this tick? One push per body per tick is what keeps a
    /// crowd from shoving one unit several cells in a single tick. Reserved to
    /// [`MAX_ENTITIES`] so a tick never grows it.
    pushed: Vec<bool>,
    /// What went wrong in the most recent tick, if anything. Cleared at the
    /// start of every movement pass, so it always describes the last tick and
    /// never an older one.
    last_tick_error: Option<TickError>,
    /// Testkit only: has a raw store mutation happened since the last clean
    /// overlap-repair pass? Nothing in the game can merge two bodies — every
    /// path that places a unit (seeding, movement, production, the push off a
    /// finished building) already respects them — so the repair pass exists
    /// solely for [`Self::force_position_for_test`] and [`Self::entities_mut`]
    /// and runs only when one of them has armed it. The shipping tick compiles
    /// no repair pass at all.
    ///
    /// Sticky on failure: an overlap the grid cannot repair leaves this set, so
    /// the pass retries and re-reports [`TickError::UnrepairableOverlap`] every
    /// tick instead of going quiet after one.
    #[cfg(feature = "testkit")]
    repair_armed: bool,
    /// Testkit only: how many overlap-repair passes have actually run. The
    /// deterministic work count the repair-disabled invariants assert on —
    /// never a timing.
    #[cfg(feature = "testkit")]
    repair_runs: u64,
    selection: Selection,
    /// Scratch buffer for a box select's result, before it replaces
    /// [`Self::selection`]. Reserved to [`MAX_ENTITIES`] so no selection
    /// operation allocates.
    pick_scratch: Vec<EntityId>,
    /// The pending build ghost.
    placement: Placement,
    /// Per-slot "is this site attended this tick" scratch, reused every tick.
    /// Reserved to [`MAX_ENTITIES`] so construction never allocates.
    build_attend: Vec<bool>,
    /// Sites that finished this tick, reused every tick. Reserved to
    /// [`MAX_ENTITIES`] so construction never allocates.
    finished: Vec<EntityId>,
    /// Body positions one placement plan must avoid: every live body a plan
    /// does not move, plus the positions that plan has already handed out.
    /// Reserved to [`MAX_ENTITIES`] so a blocked production spawn or a
    /// discarded evacuation plan never allocates.
    body_scratch: Vec<[f32; 2]>,
    /// Slots of the bodies one finishing building would swallow, and where
    /// each is planned to stand — parallel, both reserved to
    /// [`MAX_ENTITIES`].
    evac_units: Vec<usize>,
    evac_to: Vec<[f32; 2]>,
    /// One production queue and rally point per entity slot.
    production: ProductionTable,
    /// The view every packer projects through. World state, not view state: a
    /// replay that ends looking somewhere else did not reproduce.
    camera: Camera,
    /// Held arrow-key pan direction, set by the input layer. Each component
    /// in `-1.0..=1.0`. Transient input, not world state.
    keyboard_pan_dir: [f32; 2],
    /// Pointer-edge pan direction, set by the input layer. Each component in
    /// `-1.0..=1.0`. Transient input, not world state.
    edge_pan_dir: [f32; 2],
    /// Keyboard pan speed, cells/second. Transient config, not world state.
    keyboard_pan_speed: f32,
    /// Edge pan speed, cells/second. Transient config, not world state.
    edge_pan_speed: f32,
}

/// The camera pan speed a world starts with, before the app applies the
/// player's persisted settings value. The app's settings module lives
/// outside this crate and cannot be imported here, so this mirrors its
/// default (`48`) rather than sharing it.
pub const DEFAULT_CAMERA_PAN_SPEED: f32 = 48.0;

/// How many bodies one mover may displace, in total, in a single step —
/// counting the bodies it touches directly and every body those in turn have
/// to be shoved out of the way of.
///
/// A bound, not a tuning knob. A body of one radius can be touched by at most
/// six others of the same radius at once (the hexagonal packing bound), so
/// eight leaves room for one direct ring plus the tail of a chain out of it
/// while keeping the cost of a rejected step flat. Past it, a "step" would be
/// a mover ploughing through a crowd: the candidate is rejected whole.
const MAX_PUSHED_BODIES: usize = 8;

/// How far a push may propagate: the mover displaces a body (depth 1), that
/// body may displace one it would land on (depth 2), and so on to this depth.
///
/// Three is what the seeded scene actually needs — its starting workers stand
/// in a row exactly one body diameter apart, so freeing the first requires
/// moving the second and third — and stopping there is what keeps a shove from
/// rippling across a whole base. A chain that would need to go deeper is
/// rejected whole; the mover waits instead.
const MAX_PUSH_DEPTH: u8 = 3;

/// The longest a single displacement can be, in cells, as a multiple of one
/// tick's walk step.
///
/// No two bodies overlap when a tick begins, so a mover that advanced by one
/// `step` can have penetrated a depth-1 body by at most `step`, and
/// [`RtsWorld::push_target`] pushes it out by `depth + step` — at most
/// `2 * step`. That displaced body can in turn have penetrated a depth-2 body
/// by at most `2 * step`, so it is pushed at most `3 * step`, and so on: the
/// deepest link in the chain moves at most `(MAX_PUSH_DEPTH + 1) * step`.
const MAX_DISPLACEMENT_STEPS: f32 = MAX_PUSH_DEPTH as f32 + 1.0;

/// Unit-speed ceiling the displaced-vs-displaced check in
/// [`RtsWorld::try_push_chain`] imposes.
///
/// Two displaced bodies are checked against each other at their **endpoints**
/// only. That is sound while they cannot have crossed on the way there, and
/// the rule the check relies on is that crossing takes a relative displacement
/// of at least one body diameter. Two bodies each moving up to
/// [`MAX_DISPLACEMENT_STEPS`] steps have a relative displacement of up to
/// twice that, so the bound is
/// `2 * MAX_DISPLACEMENT_STEPS * speed * TICK_DT < RTS_UNIT_BODY_DIAMETER_CELLS`.
///
/// At today's `MAX_PUSH_DEPTH = 3`, `TICK_DT = 1/60` and a 6-cell diameter
/// that is 45 cells/second. Raise a unit past it and the invariant stops
/// holding *silently* — no test fails, bodies just start clipping through each
/// other inside a push chain — so the ceiling is asserted at compile time
/// below rather than left as a comment.
pub const MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC: f32 =
    RTS_UNIT_BODY_DIAMETER_CELLS / (2.0 * MAX_DISPLACEMENT_STEPS * TICK_DT);

const _: () = assert!(
    WORKER_SPEED_CELLS_PER_SEC < MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC,
    "the worker outruns the push chain's endpoint check: raise MAX_PUSH_DEPTH's \
     cost or lower the speed — see MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC"
);
const _: () = assert!(
    SOLDIER_SPEED_CELLS_PER_SEC < MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC,
    "the soldier outruns the push chain's endpoint check: raise MAX_PUSH_DEPTH's \
     cost or lower the speed — see MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC"
);

/// The headings one mover tries in a tick, as `(cos, sin)` rotations of its
/// own field descent vector: straight ahead first, then 45 degrees to each
/// side, then 90 degrees to each side. First legal one wins, and a unit with
/// no legal heading stands still.
///
/// The fallback exists because a pooled flow field is body-blind: it routes
/// around walls, never around units, so a mover whose descent points at a body
/// that cannot legally be shoved — one pinned against a building's clearance,
/// say — would otherwise be frozen for good with open ground beside it. The
/// order is fixed and the set is closed, so which heading a unit takes is a
/// pure function of the world state; the clockwise-before-anticlockwise
/// convention is arbitrary but must not change, since it is hashed state.
const MOVE_DEFLECTIONS: [(f32, f32); 5] = [
    (1.0, 0.0),
    (COS_45, -COS_45),
    (COS_45, COS_45),
    (0.0, -1.0),
    (0.0, 1.0),
];

/// `cos 45 == sin 45`, the only rotation magnitude [`MOVE_DEFLECTIONS`] needs.
const COS_45: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// One body displaced by a push chain: which body, where it lands, and how
/// deep in the chain it sits.
#[derive(Clone, Copy, Debug)]
struct Displacement {
    body: usize,
    to: [f32; 2],
    depth: u8,
}

impl Displacement {
    const NONE: Self = Self {
        body: usize::MAX,
        to: [0.0, 0.0],
        depth: 0,
    };
}

/// What one unit's swept candidate runs into, among the other unit bodies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodySweep {
    /// Nothing: the candidate can be committed as it stands.
    Clear,
    /// `count` bodies, named by the first `count` entries of `hit` (indices
    /// into the movement system's buffers, ascending). Each is a push-aside
    /// candidate.
    Bodies {
        hit: [usize; MAX_PUSHED_BODIES],
        count: usize,
    },
    /// More bodies at once than [`MAX_PUSHED_BODIES`]. Never pushed.
    TooMany,
}

/// The nearest legal free body centre to `preferred`: a cell whose centre is
/// clear per `static_nav.center_blocked()` (the precomputed
/// [`StaticNav::position_clear`] answer for a unit's own body radius), is
/// clear of `exclude` when one is given, and does not overlap any `placed`
/// body (two bodies overlap when their centres are closer than one body
/// diameter — touching is legal). Ties (equal squared distance to
/// `preferred`) go to the lower flat cell index, so the choice never depends
/// on scan order. `None` only when the grid has no such cell.
///
/// When `preferred` is exactly a legal free unexcluded body centre, that
/// centre is the unique optimum (squared distance 0) and is returned without
/// scanning the map.
///
/// The one planner behind every body placement this world performs: seeding a
/// scenario's starting workers, repairing a penetration the world was handed,
/// placing a finished production unit, and evacuating the bodies a finishing
/// building would swallow. Each caller differs only in the exclusion set it
/// supplies.
///
/// `ignore` names one index in `placed` that is not an obstacle to itself —
/// the unit being relocated, when this runs as the tick's overlap repair.
/// `exclude` is a cell rectangle (minimum corner, edge) no candidate body may
/// penetrate: the footprint a building is about to occupy, which is not in
/// `static_nav` yet because the plan that would stamp it may still fail.
///
/// # Connectivity
///
/// A relocation is a body being *moved*, not teleported. Raw Euclidean
/// distance alone would happily pick the legal centre one cell across a wall
/// — nearer than anything on the body's own side — and drop an evacuated,
/// produced or repaired unit into a pocket it could never have walked to and
/// may never walk out of. So a candidate must also lie in the *same connected
/// region of legal centres* as `preferred`, by
/// [`StaticNav::connected`] — the same 8-neighbour, no-corner-cut rule the
/// pooled fields integrate with, so "connected" here means exactly what
/// "reachable" means to a walk.
///
/// `preferred` itself may be illegal (a body inside the footprint a building
/// is about to occupy is exactly that), and an illegal cell has no region.
/// The anchor region is then the one holding the nearest legal centre to
/// `preferred` — the pocket the body is standing in — which is the same
/// answer whenever `preferred` is legal, so there is one rule, not two.
fn nearest_free_body_center(
    static_nav: &StaticNav,
    placed: &[[f32; 2]],
    ignore: Option<usize>,
    exclude: Option<(Cell, u32)>,
    preferred: [f32; 2],
) -> Option<[f32; 2]> {
    let width = static_nav.width();
    let height = static_nav.height();
    let cb = static_nav.center_blocked();
    let diam2 = RTS_UNIT_BODY_DIAMETER_CELLS * RTS_UNIT_BODY_DIAMETER_CELLS;

    // --- FAST PATH (the exact preferred cell is the unique optimum) ---
    // Every predicate is required. Short-circuit order is fixed, and after the
    // bounds check the predicates are the exhaustive loop's own, in the loop's
    // own order:
    //  1. finite preferred coords
    //  2. exact cell-centre equality (`== floor + 0.5`) on both axes
    //  3. non-negative floor
    //  4. in-bounds cell (no clamp) — must precede any `cb` index
    //  5. `!cb[idx]`                          == loop arm 1
    //  6. `component_at(cell).is_some()`      == loop arm 2; a legal preferred
    //     cell is its own anchor, so `== Some(anchor)` here would compare a
    //     value with itself
    //  7. exclusion clear                     == loop arm 3
    //  8. body clear vs `placed`/`ignore`     == loop arm 4
    // Hit → return the reconstructed centre. Miss → fall through unchanged.
    if preferred[0].is_finite() && preferred[1].is_finite() {
        let fx = preferred[0].floor();
        let fy = preferred[1].floor();
        if preferred[0] == fx + 0.5 && preferred[1] == fy + 0.5 && fx >= 0.0 && fy >= 0.0 {
            let x = fx as u32;
            let y = fy as u32;
            if x < width && y < height {
                let idx = (x + y * width) as usize;
                let cell = Cell { x, y };
                if !cb[idx] && static_nav.component_at(cell).is_some() {
                    let p = [x as f32 + 0.5, y as f32 + 0.5];
                    let excluded = exclude.is_some_and(|(min, edge)| {
                        !circle_clear_of_cell_rect(p, RTS_UNIT_BODY_RADIUS_CELLS, min, edge)
                    });
                    if !excluded {
                        let blocked_by_body = placed
                            .iter()
                            .enumerate()
                            .any(|(j, &q)| Some(j) != ignore && dist2(p, q) < diam2);
                        if !blocked_by_body {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }

    let anchor = anchor_component(static_nav, preferred)?;

    let mut best: Option<(f32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            #[cfg(test)]
            {
                NEAREST_FREE_BODY_CENTER_CELL_VISITS
                    .set(NEAREST_FREE_BODY_CENTER_CELL_VISITS.get().saturating_add(1));
            }
            let idx = (x + y * width) as usize;
            if cb[idx] {
                continue;
            }
            if static_nav.component_at(Cell { x, y }) != Some(anchor) {
                continue;
            }
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            if exclude.is_some_and(|(min, edge)| {
                !circle_clear_of_cell_rect(p, RTS_UNIT_BODY_RADIUS_CELLS, min, edge)
            }) {
                continue;
            }
            if placed
                .iter()
                .enumerate()
                .any(|(j, &q)| Some(j) != ignore && dist2(p, q) < diam2)
            {
                continue;
            }
            let d = dist2(p, preferred);
            let idx = idx as u32;
            if best.is_none_or(|(bd, bi)| d < bd || (d == bd && idx < bi)) {
                best = Some((d, idx));
            }
        }
    }
    best.map(|(_, idx)| [(idx % width) as f32 + 0.5, (idx / width) as f32 + 0.5])
}

#[cfg(test)]
thread_local! {
    static NEAREST_FREE_BODY_CENTER_CELL_VISITS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_nearest_free_body_center_cell_visits() {
    NEAREST_FREE_BODY_CENTER_CELL_VISITS.set(0);
}

#[cfg(test)]
fn nearest_free_body_center_cell_visits() -> u64 {
    NEAREST_FREE_BODY_CENTER_CELL_VISITS.get()
}

/// The connected region [`nearest_free_body_center`] confines its search to:
/// the one holding `preferred`, or — when `preferred` is not itself a legal
/// body centre — the one holding the nearest legal centre to it.
///
/// `None` only when the grid has no legal body centre at all.
fn anchor_component(static_nav: &StaticNav, preferred: [f32; 2]) -> Option<u32> {
    let width = static_nav.width();
    let height = static_nav.height();
    let cell = Cell {
        x: (preferred[0].floor().max(0.0) as u32).min(width.saturating_sub(1)),
        y: (preferred[1].floor().max(0.0) as u32).min(height.saturating_sub(1)),
    };
    if let Some(c) = static_nav.component_at(cell) {
        return Some(c);
    }
    let cb = static_nav.center_blocked();
    let mut best: Option<(f32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            let idx = (x + y * width) as usize;
            if cb[idx] {
                continue;
            }
            let d = dist2([x as f32 + 0.5, y as f32 + 0.5], preferred);
            let idx = idx as u32;
            if best.is_none_or(|(bd, bi)| d < bd || (d == bd && idx < bi)) {
                best = Some((d, idx));
            }
        }
    }
    let (_, idx) = best?;
    static_nav.component_at(Cell {
        x: idx % width,
        y: idx / width,
    })
}

impl RtsWorld {
    /// Load a hash-verified scenario and seed the world from its RTS block.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, RtsWorldError> {
        let scenario = Scenario::load_verified(path)?;
        Self::from_scenario(scenario)
    }

    /// Seed from an already-validated scenario.
    pub fn from_scenario(scenario: Scenario) -> Result<Self, RtsWorldError> {
        let rts = scenario.rts().ok_or_else(|| RtsWorldError::NotAnRtsScene {
            version: scenario.version().to_string(),
        })?;

        let mut entities = EntityStore::new();

        // 1. The HQ, at its footprint centre. Finished, not a site.
        let hq_pos = [
            rts.hq_cell.x as f32 + scenario::HQ_FOOTPRINT_CELLS as f32 * 0.5,
            rts.hq_cell.y as f32 + scenario::HQ_FOOTPRINT_CELLS as f32 * 0.5,
        ];
        let start_hq = entities
            .spawn(EntityKind::Building(BuildingKind::Hq), OWNER_PLAYER, hq_pos)
            .ok_or_else(|| RtsWorldError::StoreFull {
                what: "hq".to_string(),
            })?;

        // 2. Crystal nodes, then gas nodes, each at its cell centre.
        for c in &rts.crystal_nodes {
            let pos = [c.x as f32 + 0.5, c.y as f32 + 0.5];
            let id = entities
                .spawn(EntityKind::Node(ResourceKind::Crystal), OWNER_NEUTRAL, pos)
                .ok_or_else(|| RtsWorldError::StoreFull {
                    what: "crystal node".to_string(),
                })?;
            entities.set_amount(id.index as usize, node_amount(ResourceKind::Crystal));
        }
        for c in &rts.gas_nodes {
            let pos = [c.x as f32 + 0.5, c.y as f32 + 0.5];
            let id = entities
                .spawn(EntityKind::Node(ResourceKind::Gas), OWNER_NEUTRAL, pos)
                .ok_or_else(|| RtsWorldError::StoreFull {
                    what: "gas node".to_string(),
                })?;
            entities.set_amount(id.index as usize, node_amount(ResourceKind::Gas));
        }

        // 3. `StaticNav`: terrain, resource nodes and the finished HQ. Built
        // now, before any unit exists, because units are placed *against*
        // it — the collision-free spawn search below reads its
        // `center_blocked` mask, and folding a unit's own body into that mask
        // would make it obstruct its own search.
        let static_nav = StaticNav::new(&scenario, &entities)?;

        // 4. One worker per scenario spawn cell, in scenario order, each
        // relocated to the nearest legal (collision-free, in-bounds) cell
        // centre — the preferred cell itself, when it is already legal.
        // Scenario spawn cells are historically packed one cell apart, far
        // closer than two 3-cell-radius bodies can share, so a body-blind
        // placement would seed the world with overlapping units before a
        // single tick ever ran. Ties (equal squared distance to the
        // preferred cell) go to the lower flat cell index, so relocation is
        // reproducible independent of scan order.
        let mut worker_count: u32 = 0;
        let mut placed: Vec<[f32; 2]> = Vec::with_capacity(scenario.spawn_cells().len());
        for c in scenario.spawn_cells() {
            let preferred = [c.x as f32 + 0.5, c.y as f32 + 0.5];
            let pos = nearest_free_body_center(&static_nav, &placed, None, None, preferred)
                .ok_or(RtsWorldError::NoFreeUnitPosition)?;
            entities
                .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, pos)
                .ok_or_else(|| RtsWorldError::StoreFull {
                    what: "worker".to_string(),
                })?;
            placed.push(pos);
            worker_count += 1;
        }

        let resources = Resources {
            crystal: rts.start_crystal,
            gas: rts.start_gas,
        };
        let mut supply = Supply::new(rts.start_supply_cap);
        supply.add_used(WORKER_SUPPLY_COST * worker_count);

        // The seeded HQ is already stamped into `static_nav.solids` (it was
        // spawned before `StaticNav::new` ran), so the pool is built straight
        // from the inflated centre mask — units, not raw terrain, is what a
        // pooled field must never route a body's centre across.
        let nav = FieldPool::from_blocked_mask(
            scenario.width(),
            scenario.height(),
            static_nav.center_blocked(),
        )?;

        let cells = scenario.width() as usize * scenario.height() as usize;

        // Opens on the base: the HQ's footprint centre is what a player wants
        // to see on frame 1, not the map's geometric middle.
        let camera = Camera::new(
            scenario.width(),
            scenario.height(),
            scenario.cell_size_px() as f32,
            [VIEW_WIDTH as f32, VIEW_HEIGHT as f32],
            hq_pos,
        );

        Ok(Self {
            scenario,
            entities,
            resources,
            supply,
            tick_index: 0,
            start_hq: Some(start_hq),
            static_nav,
            nav,
            orders: OrderTable::new(),
            formation: FormationScratch::new(cells),
            live_scratch: Vec::with_capacity(MAX_ENTITIES),
            unit_scratch: Vec::with_capacity(MAX_ENTITIES),
            candidate_pos: Vec::with_capacity(MAX_ENTITIES),
            pushed: Vec::with_capacity(MAX_ENTITIES),
            last_tick_error: None,
            #[cfg(feature = "testkit")]
            repair_armed: false,
            #[cfg(feature = "testkit")]
            repair_runs: 0,
            selection: Selection::new(),
            pick_scratch: Vec::with_capacity(MAX_ENTITIES),
            placement: Placement::None,
            build_attend: vec![false; MAX_ENTITIES],
            finished: Vec::with_capacity(MAX_ENTITIES),
            body_scratch: Vec::with_capacity(MAX_ENTITIES),
            evac_units: Vec::with_capacity(MAX_ENTITIES),
            evac_to: Vec::with_capacity(MAX_ENTITIES),
            production: ProductionTable::new(),
            camera,
            keyboard_pan_dir: [0.0, 0.0],
            edge_pan_dir: [0.0, 0.0],
            keyboard_pan_speed: DEFAULT_CAMERA_PAN_SPEED,
            edge_pan_speed: DEFAULT_CAMERA_PAN_SPEED,
        })
    }

    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    pub fn entities(&self) -> &EntityStore {
        &self.entities
    }

    /// How many pairs of live units currently penetrate each other (`T17`).
    ///
    /// The one shared body-safety oracle: the engine acceptance run asserts
    /// it at every milestone, and the app's `rts` exit line reports it, so a
    /// scripted run and a world-call run cannot disagree about what "hard
    /// bodies" means. Exactly [`units_overlap`]'s test — touching at
    /// `r1 + r2` is legal, anything closer is a penetration — over every
    /// unordered pair, buildings and nodes excluded (they are footprints,
    /// not circles).
    ///
    /// `O(n^2)` on purpose: this is an observation seam for tests and the
    /// exit line, never a per-tick path.
    pub fn body_overlap_count(&self) -> u32 {
        let store = &self.entities;
        let radius = |slot: usize| match store.kind(slot) {
            EntityKind::Unit(k) => Some(k.body_radius_cells()),
            _ => None,
        };
        let mut units: Vec<(usize, f32)> = Vec::new();
        for slot in 0..store.slot_count() {
            if store.alive(slot)
                && let Some(r) = radius(slot)
            {
                units.push((slot, r));
            }
        }
        let mut overlaps = 0u32;
        for (n, &(a, ra)) in units.iter().enumerate() {
            let pa = store.position(a);
            for &(b, rb) in &units[n + 1..] {
                if units_overlap(pa, ra, store.position(b), rb) {
                    overlaps += 1;
                }
            }
        }
        overlaps
    }

    /// Direct entity-store mutation. A test hook: hard unit collision makes a
    /// raw position or spawn a world invariant, so shipping code routes
    /// through [`RtsWorld`]'s own systems instead.
    ///
    /// Taking this borrow **arms the overlap-repair pass** ([`Self::repair_armed`]):
    /// the writes it hands out cannot be observed from here, so a raw store
    /// mutation is assumed to be able to merge two bodies. The next tick's
    /// repair pass is what clears one.
    #[cfg(feature = "testkit")]
    pub fn entities_mut(&mut self) -> &mut EntityStore {
        self.repair_armed = true;
        &mut self.entities
    }

    /// Test-only: place a live entity anywhere, **including on top of another
    /// body**.
    ///
    /// The one supported way to construct a penetrating world state. Nothing
    /// in the game can produce one, so this hook **arms the overlap-repair
    /// pass** ([`Self::repair_armed`]) and the next tick's pass is what must
    /// clear it, reporting [`Self::last_tick_error`] when it cannot. `false`
    /// for a stale id, and a stale id arms nothing.
    #[cfg(feature = "testkit")]
    pub fn force_position_for_test(&mut self, id: EntityId, pos: [f32; 2]) -> bool {
        let Some(slot) = self.entities.slot(id) else {
            return false;
        };
        self.entities.set_position(slot, pos);
        self.repair_armed = true;
        true
    }

    /// What went wrong in the most recent [`Self::tick`], if anything.
    ///
    /// Only ever `Some` after a tick that began with merged unit bodies the
    /// grid had no free legal centre to repair — see [`TickError`].
    pub fn last_tick_error(&self) -> Option<TickError> {
        self.last_tick_error
    }

    /// Testkit only: how many overlap-repair passes this world has run.
    ///
    /// The pass is armed by [`Self::force_position_for_test`] and
    /// [`Self::entities_mut`] and by nothing else, so a run that touches
    /// neither must report `0` — that is the observation the repair-disabled
    /// invariants assert on, and it is a work count, never a timing.
    #[cfg(feature = "testkit")]
    pub fn overlap_repair_runs(&self) -> u64 {
        self.repair_runs
    }

    /// Direct resource-stock mutation. A test hook: phase 1 has no order
    /// system to spend resources through yet, and [`Self::state_hash`] needs
    /// exercising against a spend regardless.
    #[cfg(feature = "testkit")]
    pub fn resources_mut(&mut self) -> &mut Resources {
        &mut self.resources
    }

    pub fn resources(&self) -> Resources {
        self.resources
    }

    pub fn supply(&self) -> Supply {
        self.supply
    }

    pub fn tick_index(&self) -> u64 {
        self.tick_index
    }

    /// The starting HQ. `None` only after it is destroyed, which nothing in
    /// phase 1 can do.
    pub fn start_hq(&self) -> Option<EntityId> {
        self.start_hq
    }

    /// The navigation pool. Buildings stamp obstacles into it (T10).
    pub fn nav(&self) -> &FieldPool {
        &self.nav
    }

    /// The static (terrain, nodes, finished buildings) world geometry every
    /// body-radius clearance check and approach-cell search reads.
    pub fn static_nav(&self) -> &StaticNav {
        &self.static_nav
    }

    /// Direct navigation mutation. A test hook: phase 1 has no gameplay path
    /// that blocks an arbitrary cell outside the construction system, and the
    /// approach-cell regression test needs to simulate a stamped HQ.
    #[cfg(feature = "testkit")]
    pub fn nav_mut(&mut self) -> &mut FieldPool {
        &mut self.nav
    }

    /// The camera this world is looked at through.
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Mutable camera access — "jump to base", or a direct pan.
    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    /// The projection this frame packs through.
    pub fn iso_view(&self) -> IsoView {
        self.camera.iso_view()
    }

    /// Set the held arrow-key pan direction, in **screen** space.
    /// Components are expected in `-1.0..=1.0`.
    pub fn set_keyboard_pan_dir(&mut self, dir: [f32; 2]) {
        self.keyboard_pan_dir = dir;
    }

    /// The held arrow-key pan direction.
    pub fn keyboard_pan_dir(&self) -> [f32; 2] {
        self.keyboard_pan_dir
    }

    /// Set the pointer-edge pan direction, in **screen** space. Components
    /// are expected in `-1.0..=1.0`.
    pub fn set_edge_pan_dir(&mut self, dir: [f32; 2]) {
        self.edge_pan_dir = dir;
    }

    /// The pointer-edge pan direction.
    pub fn edge_pan_dir(&self) -> [f32; 2] {
        self.edge_pan_dir
    }

    /// Set the keyboard and edge pan speeds, cells/second. Applied by the app
    /// from the player's persisted settings before the first tick.
    pub fn set_camera_speeds(&mut self, keyboard: f32, edge: f32) {
        self.keyboard_pan_speed = keyboard;
        self.edge_pan_speed = edge;
    }

    /// Centre the camera on a fractional map point (clamped to the
    /// frontier). Minimap-ready: `point` need not be a cell centre.
    pub fn look_at_map_point(&mut self, point: [f32; 2]) {
        self.camera.look_at_point(point);
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn selection_mut(&mut self) -> &mut Selection {
        &mut self.selection
    }

    /// Apply a plain click: replace the selection with what was picked, or
    /// clear it when nothing was.
    pub fn click_select(&mut self, view: &IsoView, screen: [f32; 2]) -> Pick {
        let pick = pick_at(self, view, screen);
        self.selection.clear();
        match pick {
            Pick::Unit(id) | Pick::Building(id) | Pick::Node(id) => {
                self.selection.insert(id);
            }
            Pick::Nothing => {}
        }
        pick
    }

    /// Apply an additive (shift) click: toggle what was picked. A click on
    /// nothing leaves the selection alone — a modifier click is a refinement,
    /// and clearing on a near-miss is the single most annoying selection bug in
    /// the genre.
    pub fn shift_click_select(&mut self, view: &IsoView, screen: [f32; 2]) -> Pick {
        let pick = pick_at(self, view, screen);
        match pick {
            Pick::Unit(id) | Pick::Building(id) | Pick::Node(id) => {
                self.selection.toggle(id);
            }
            Pick::Nothing => {}
        }
        pick
    }

    /// Replace the selection with exactly `id` — a HUD selection-icon click,
    /// not a screen pick. `false` (a no-op) for a stale `id`: a click on an
    /// icon that died the same tick must not wipe an otherwise-live
    /// selection.
    pub fn select_only(&mut self, id: EntityId) -> bool {
        if !self.entities.contains(id) {
            return false;
        }
        self.selection.clear();
        self.selection.insert(id);
        true
    }

    /// Toggle exactly `id` in the selection — a shift-click on a HUD
    /// selection icon. `false` (a no-op) for a stale `id`.
    pub fn toggle_selection(&mut self, id: EntityId) -> bool {
        if !self.entities.contains(id) {
            return false;
        }
        self.selection.toggle(id);
        true
    }

    /// Apply a drag rectangle: replace the selection with every own unit inside.
    /// An empty box clears the selection.
    pub fn box_select_into_selection(&mut self, view: &IsoView, a: [f32; 2], b: [f32; 2]) -> usize {
        let mut scratch = std::mem::take(&mut self.pick_scratch);
        box_select(self, view, a, b, &mut scratch);
        self.selection.replace(&scratch);
        self.pick_scratch = scratch;
        self.selection.len()
    }

    /// Order one unit to walk to `dest`.
    ///
    /// A group of one: the unit is given the anchor cell itself as its slot.
    /// `false` when `id` is stale, is not a player-owned unit, or `dest` is
    /// off the grid.
    pub fn order_move(&mut self, id: EntityId, dest: Cell) -> bool {
        self.order_move_group(&[id], dest).is_ok()
    }

    /// Order several units into a formation around `dest`, acquiring the
    /// anchor field **once**.
    ///
    /// This is the API the input layer uses. Issuing N single orders would
    /// acquire N times, and on a full pool that is N rebuilds of the same
    /// field — and, since T4, would send N bodies at one cell only one of them
    /// can stand on.
    ///
    /// Whole or nothing: `Ok(n)` means every one of the `n` orderable units in
    /// `ids` now holds a distinct slot around one shared anchor, and any error
    /// means nothing was written at all.
    pub fn order_move_group(
        &mut self,
        ids: &[EntityId],
        dest: Cell,
    ) -> Result<usize, FormationError> {
        self.order_group(ids, GroupTarget::Ground(dest), None)
    }

    /// Plan and commit one group order, whole or not at all.
    ///
    /// The one path every group command takes — ground move, gather, build,
    /// and every context click that resolves into one of them:
    ///
    /// 1. canonicalize the group: every live, player-owned unit in `ids` that
    ///    is eligible for `target`, ascending by entity slot, so the caller's
    ///    argument order cannot reach the plan;
    /// 2. resolve the target to one anchor cell;
    /// 3. acquire that anchor's field — **once**, for the whole group;
    /// 4. plan one distinct slot per member ([`FormationScratch::plan`]);
    /// 5. write every member's order, sharing the one field handle, and push
    ///    one receipt each in the same ascending order.
    ///
    /// `receipts`, when given, is cleared first: a refused order leaves an
    /// empty buffer rather than the previous click's answers.
    fn order_group(
        &mut self,
        ids: &[EntityId],
        target: GroupTarget,
        mut receipts: Option<&mut OrderReceiptBuffer>,
    ) -> Result<usize, FormationError> {
        if let Some(r) = receipts.as_deref_mut() {
            r.clear();
        }
        let anchor = self.group_anchor(target)?;

        self.formation.begin();
        for slot in 0..self.entities.slot_count() {
            let Some(id) = self.entities.id_at(slot) else {
                continue;
            };
            if !ids.contains(&id) || self.orderable_slot(id).is_none() {
                continue;
            }
            if !self.eligible_for(slot, target) {
                continue;
            }
            self.formation.push_unit(id);
        }
        if self.formation.len() == 0 {
            return Err(FormationError::NoUnits);
        }

        let field = self
            .nav
            .acquire(anchor)
            .map_err(|_| FormationError::Unreachable)?;
        self.formation.plan(
            &self.static_nav,
            &self.entities,
            &self.nav,
            field.slot,
            anchor,
        )?;

        for i in 0..self.formation.len() {
            let id = self.formation.unit(i);
            let goal = FormationGoal {
                anchor,
                slot: self.formation.slot(i),
            };
            let slot = self.entities.slot(id).expect("a planned unit is live");
            let is_worker = matches!(self.entities.kind(slot), EntityKind::Unit(UnitKind::Worker));
            let (order, issued) = match target {
                GroupTarget::Ground(_) => (Order::Move { goal, field }, IssuedOrder::Move),
                GroupTarget::Node(node) if is_worker => (
                    Order::Gather {
                        node,
                        phase: GatherPhase::ToNode { goal, field },
                    },
                    IssuedOrder::Gather,
                ),
                // A non-worker cannot mine, but it can still be sent to the
                // node: it takes a formation slot around the same anchor.
                GroupTarget::Node(_) => (Order::Move { goal, field }, IssuedOrder::Move),
                GroupTarget::Site(site) => (Order::Build { site, goal, field }, IssuedOrder::Build),
            };
            self.orders.set(slot, order);
            if let Some(r) = receipts.as_deref_mut() {
                r.push(id, issued);
            }
        }
        Ok(self.formation.len())
    }

    /// The one cell a group command forms up around.
    ///
    /// A ground click snaps to the nearest cell a body may legally stand on; a
    /// node or a site resolves to its deterministic approach cell, the same one
    /// [`entity_approach_cell`] has always produced.
    fn group_anchor(&self, target: GroupTarget) -> Result<Cell, FormationError> {
        match target {
            GroupTarget::Ground(cell) => {
                if cell.x >= self.scenario.width() || cell.y >= self.scenario.height() {
                    return Err(FormationError::Unreachable);
                }
                nearest_body_clear_cell(&self.static_nav, cell).ok_or(FormationError::Unreachable)
            }
            GroupTarget::Node(node) => {
                let Some(node_slot) = self.entities.slot(node) else {
                    return Err(FormationError::NoTarget);
                };
                if !matches!(self.entities.kind(node_slot), EntityKind::Node(_)) {
                    return Err(FormationError::NoTarget);
                }
                Ok(
                    entity_approach_cell(&self.static_nav, &self.entities, node, UnitKind::Worker)
                        .0,
                )
            }
            GroupTarget::Site(site) => {
                if !self.is_site(site) {
                    return Err(FormationError::NoTarget);
                }
                Ok(
                    entity_approach_cell(&self.static_nav, &self.entities, site, UnitKind::Worker)
                        .0,
                )
            }
        }
    }

    /// Whether the unit at `slot` can take this target's order at all. A unit
    /// that cannot is left out of the plan entirely — it keeps whatever it was
    /// already doing, and the caller counts it as rejected.
    fn eligible_for(&self, slot: usize, target: GroupTarget) -> bool {
        let is_worker = matches!(self.entities.kind(slot), EntityKind::Unit(UnitKind::Worker));
        match target {
            GroupTarget::Ground(_) => true,
            // A depleted node is nothing to mine: a worker is rejected, and
            // anything else still walks over.
            GroupTarget::Node(node) => {
                !is_worker
                    || self
                        .entities
                        .slot(node)
                        .is_some_and(|s| self.entities.amount(s) > 0)
            }
            GroupTarget::Site(_) => is_worker,
        }
    }

    /// The current order of a live entity.
    pub fn order_of(&self, id: EntityId) -> Option<Order> {
        self.entities.slot(id).map(|slot| self.orders.get(slot))
    }

    /// Slot of a live player-owned unit — the only thing an order applies to.
    fn orderable_slot(&self, id: EntityId) -> Option<usize> {
        let slot = self.entities.slot(id)?;
        if !matches!(self.entities.kind(slot), EntityKind::Unit(_)) {
            return None;
        }
        if self.entities.owner(slot) != OWNER_PLAYER {
            return None;
        }
        Some(slot)
    }

    /// Slot of a live player-owned worker — the only unit kind that can gather.
    fn worker_slot(&self, id: EntityId) -> Option<usize> {
        let slot = self.entities.slot(id)?;
        if !matches!(self.entities.kind(slot), EntityKind::Unit(UnitKind::Worker)) {
            return None;
        }
        if self.entities.owner(slot) != OWNER_PLAYER {
            return None;
        }
        Some(slot)
    }

    /// Order one worker to gather from `node`.
    ///
    /// `false` when: `id` is stale, is not a `UnitKind::Worker`, is not
    /// `OWNER_PLAYER`; `node` is stale or is not an `EntityKind::Node`; the
    /// node is already empty; or no legal approach slot is left around it.
    /// A Soldier cannot gather — refusing is what makes the HUD's "no valid
    /// order" state real rather than cosmetic.
    pub fn order_gather(&mut self, id: EntityId, node: EntityId) -> bool {
        self.worker_slot(id).is_some() && self.order_gather_group(&[id], node) == Ok(1)
    }

    /// Order a group onto one node, acquiring the anchor field once.
    ///
    /// Workers get a distinct legal approach slot each and mine; anything else
    /// in the group forms up around the same anchor instead — it cannot mine,
    /// but "go there" is still what the player asked for.
    pub fn order_gather_group(
        &mut self,
        ids: &[EntityId],
        node: EntityId,
    ) -> Result<usize, FormationError> {
        self.order_group(ids, GroupTarget::Node(node), None)
    }

    /// The nearest live drop-off building owned by the player, by distance
    /// from `pos` to its footprint rectangle. Ties go to the lower entity
    /// slot.
    pub fn nearest_drop_off(&self, pos: [f32; 2]) -> Option<EntityId> {
        let slot_count = self.entities.slot_count();
        let mut best: Option<(f32, usize)> = None;
        for slot in 0..slot_count {
            if !self.entities.alive(slot) {
                continue;
            }
            let EntityKind::Building(b) = self.entities.kind(slot) else {
                continue;
            };
            if !b.is_drop_off() {
                continue;
            }
            if self.entities.owner(slot) != OWNER_PLAYER {
                continue;
            }
            let d = rect_distance(pos, self.entities.position(slot), b.footprint_cells());
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, slot));
            }
        }
        best.and_then(|(_, slot)| self.entities.id_at(slot))
    }

    /// The pending build ghost.
    pub fn placement(&self) -> Placement {
        self.placement
    }

    /// Choose a building to place. `false` when the player cannot currently
    /// afford it — the cost check is repeated at [`Self::confirm_placement`],
    /// because the stock can fall while the ghost is up.
    pub fn begin_placement(&mut self, kind: BuildingKind) -> bool {
        if !self.resources.covers(building_cost(kind)) {
            return false;
        }
        self.placement = Placement::Pending { kind };
        true
    }

    /// Drop the ghost. Idempotent.
    pub fn cancel_placement(&mut self) {
        self.placement = Placement::None;
    }

    /// Commit the pending ghost at `min`, built by `builder`.
    ///
    /// On success: debits the cost, spawns the site with
    /// `set_progress(0, build_ticks(kind))`, orders `builder` to
    /// `Order::Build { site, field }`, clears the ghost, and returns the
    /// site's id. The footprint is **not** stamped into navigation yet — a site
    /// is walkable until it finishes, which is what lets the builder stand in it.
    pub fn confirm_placement(
        &mut self,
        min: Cell,
        builder: EntityId,
    ) -> Result<EntityId, PlacementError> {
        // No pending ghost is a caller-contract violation (the UI never calls
        // this without one); there is no dedicated error for it, so this falls
        // back to `NoBuilder` rather than adding an eighth variant for an
        // unreachable-in-practice path.
        let Placement::Pending { kind } = self.placement else {
            return Err(PlacementError::NoBuilder);
        };

        let Some(builder_slot) = self.entities.slot(builder) else {
            return Err(PlacementError::NoBuilder);
        };
        let is_worker = matches!(
            self.entities.kind(builder_slot),
            EntityKind::Unit(UnitKind::Worker)
        );
        if !is_worker || self.entities.owner(builder_slot) != OWNER_PLAYER {
            return Err(PlacementError::NoBuilder);
        }

        let cost = building_cost(kind);
        if !self.resources.covers(cost) {
            return Err(PlacementError::Unaffordable);
        }

        placement_valid(self, kind, min)?;

        let edge = kind.footprint_cells();
        let center = [
            min.x as f32 + edge as f32 * 0.5,
            min.y as f32 + edge as f32 * 0.5,
        ];
        let Some(site) = self
            .entities
            .spawn(EntityKind::Building(kind), OWNER_PLAYER, center)
        else {
            return Err(PlacementError::StoreFull);
        };
        let site_slot = self.entities.slot(site).expect("just spawned");
        self.entities.set_progress(site_slot, 0, build_ticks(kind));

        let debited = self.resources.try_debit(cost);
        debug_assert!(debited, "affordability was just checked above");

        self.order_build(builder, site);
        self.placement = Placement::None;
        Ok(site)
    }

    /// Cancel an unfinished site: refund the full cost, unstamp nothing (a site
    /// was never stamped), despawn it, and clear every worker whose `Build`
    /// order named it.
    ///
    /// A **finished** building is not cancellable and returns `false`.
    pub fn cancel_construction(&mut self, site: EntityId) -> bool {
        let Some(site_slot) = self.entities.slot(site) else {
            return false;
        };
        let EntityKind::Building(kind) = self.entities.kind(site_slot) else {
            return false;
        };
        if self.entities.progress_target(site_slot) == 0 {
            return false;
        }

        self.resources.credit(building_cost(kind));
        self.entities.despawn(site);
        self.production.clear(site_slot);

        for slot in 0..self.entities.slot_count() {
            if !self.entities.alive(slot) {
                continue;
            }
            if let Order::Build { site: s, .. } = self.orders.get(slot)
                && s == site
            {
                self.orders.clear(slot);
            }
        }
        true
    }

    /// Whether a building entity is still under construction.
    pub fn is_site(&self, id: EntityId) -> bool {
        let Some(slot) = self.entities.slot(id) else {
            return false;
        };
        matches!(self.entities.kind(slot), EntityKind::Building(_))
            && self.entities.progress_target(slot) > 0
    }

    /// Order an existing worker to attend an existing site.
    pub fn order_build(&mut self, id: EntityId, site: EntityId) -> bool {
        self.order_group(&[id], GroupTarget::Site(site), None) == Ok(1)
    }

    /// Order a group onto one site, acquiring the anchor field once.
    ///
    /// Every eligible worker gets a distinct legal approach slot around the
    /// site and one receipt, ascending by entity slot; every other unit in
    /// `ids` is rejected outright (a Soldier cannot build) and the caller sees
    /// it as the difference between `ids.len()` and the returned count.
    pub fn order_build_group(
        &mut self,
        ids: &[EntityId],
        site: EntityId,
        receipts: &mut OrderReceiptBuffer,
    ) -> Result<usize, FormationError> {
        self.order_group(ids, GroupTarget::Site(site), Some(receipts))
    }

    /// Resolve a right-click context order against the shared pick geometry:
    /// gather a node, attend a build site, or move — one dispatcher shared by
    /// every input path (SDL, scripted, [`crate::testkit::RtsHarness`]).
    ///
    /// `receipts` is cleared and refilled with one [`UnitOrderReceipt`] per
    /// order actually issued, ascending by entity slot (the order
    /// [`Selection::ids`] iterates in). Capacity never grows past
    /// [`MAX_SELECTION`] — [`OrderReceiptBuffer::new`] reserves it there.
    ///
    /// Every branch resolves to one [`GroupTarget`] and one call to
    /// [`Self::order_group`], so a click is planned as a formation — one
    /// shared anchor field, one distinct slot per unit — exactly like the
    /// group APIs it shares that path with.
    ///
    /// Dispatch, by what [`pick_at`] found:
    /// - **Node** — selected workers Gather; every other selected orderable
    ///   unit takes a formation slot around the same node anchor. A depleted
    ///   node still places the non-workers but rejects Gather.
    /// - **Building under construction** (a site) — selected workers Build;
    ///   every other selected unit is rejected outright, no Move fallback.
    /// - Anything else (a finished building, empty ground, off-grid) — every
    ///   selected orderable unit forms up around the clicked cell.
    pub fn issue_context_order_at(
        &mut self,
        view: &IsoView,
        screen: [f32; 2],
        receipts: &mut OrderReceiptBuffer,
    ) -> ContextOrderResult {
        receipts.clear();
        let pick = pick_at(self, view, screen);

        let mut scratch = std::mem::take(&mut self.pick_scratch);
        scratch.clear();
        scratch.extend_from_slice(self.selection.ids());

        if scratch.is_empty() {
            self.pick_scratch = scratch;
            return ContextOrderResult {
                pick,
                accepted: 0,
                rejected: 0,
                reason: Some(ContextOrderReason::EmptySelection),
            };
        }

        // Only orderable units can be rejected: a selected building or
        // resource node was never a candidate for a move/gather/build order,
        // so counting it as "rejected" made the caller play a reject cue for a
        // selection that contained no refused unit at all.
        let selected = scratch
            .iter()
            .filter(|&&id| self.orderable_slot(id).is_some())
            .count();
        let target = match pick {
            Pick::Node(n) => Some(GroupTarget::Node(n)),
            Pick::Building(b) if self.is_site(b) => Some(GroupTarget::Site(b)),
            _ => view
                .cell_at(
                    screen[0],
                    screen[1],
                    self.scenario.width(),
                    self.scenario.height(),
                )
                .map(GroupTarget::Ground),
        };

        let Some(target) = target else {
            self.pick_scratch = scratch;
            return ContextOrderResult {
                pick,
                accepted: 0,
                rejected: selected,
                reason: Some(ContextOrderReason::NoTargetCell),
            };
        };

        let outcome = self.order_group(&scratch, target, Some(receipts));
        self.pick_scratch = scratch;

        let (accepted, reason) = match outcome {
            Ok(n) => (n, None),
            // A per-unit rejection needs no shared reason: `rejected` already
            // counts it, exactly as a mixed selection at a build site does.
            Err(FormationError::NoUnits | FormationError::NoTarget) => (0, None),
            Err(FormationError::Unreachable) => (0, Some(ContextOrderReason::Unreachable)),
            Err(FormationError::NoFormationSpace) => {
                (0, Some(ContextOrderReason::NoFormationSpace))
            }
        };
        ContextOrderResult {
            pick,
            accepted,
            rejected: selected - accepted,
            reason,
        }
    }

    /// Queue a unit at a building.
    ///
    /// Charges the **resources and the supply at enqueue time**, not at
    /// completion. Reserving supply up front is what makes the cap a real
    /// bound: charging on completion would let a player queue five Soldiers
    /// into two free supply and get all five.
    ///
    /// Checked in this exact order, first failure wins: the building must be
    /// live and player-owned, finished (not a site), able to produce `unit`,
    /// have queue room, have free supply, then have the resources — the
    /// supply check precedes the debit, so a supply-blocked enqueue never
    /// takes the player's money.
    pub fn enqueue_unit(&mut self, building: EntityId, unit: UnitKind) -> Result<(), ProduceError> {
        let Some(slot) = self.entities.slot(building) else {
            return Err(ProduceError::NoBuilding);
        };
        let EntityKind::Building(b) = self.entities.kind(slot) else {
            return Err(ProduceError::NoBuilding);
        };
        if self.entities.owner(slot) != OWNER_PLAYER {
            return Err(ProduceError::NoBuilding);
        }
        if self.entities.progress_target(slot) != 0 {
            return Err(ProduceError::UnderConstruction);
        }
        if !can_produce(b, unit) {
            return Err(ProduceError::WrongBuilding);
        }
        if self.production.queue(slot).is_full() {
            return Err(ProduceError::QueueFull);
        }
        if !self.supply.fits(supply_cost(unit)) {
            return Err(ProduceError::SupplyBlocked);
        }
        if !self.resources.try_debit(unit_cost(unit)) {
            return Err(ProduceError::Unaffordable);
        }
        self.production.queue_mut(slot).push(unit);
        self.supply.add_used(supply_cost(unit));
        Ok(())
    }

    /// Cancel queue entry `index` at `building`, refunding its cost and
    /// releasing its supply reservation. `false` when there is no such entry.
    pub fn cancel_queued(&mut self, building: EntityId, index: usize) -> bool {
        let Some(slot) = self.entities.slot(building) else {
            return false;
        };
        if !matches!(self.entities.kind(slot), EntityKind::Building(_)) {
            return false;
        }
        let Some(kind) = self.production.queue_mut(slot).cancel(index) else {
            return false;
        };
        self.resources.credit(unit_cost(kind));
        self.supply.remove_used(supply_cost(kind));
        true
    }

    /// The production queue of a live building. `None` for a stale id or a
    /// non-building.
    pub fn production_queue(&self, building: EntityId) -> Option<&ProductionQueue> {
        let slot = self.entities.slot(building)?;
        if !matches!(self.entities.kind(slot), EntityKind::Building(_)) {
            return None;
        }
        Some(self.production.queue(slot))
    }

    /// Where units produced here walk after they appear. `None` leaves them
    /// idle.
    pub fn rally(&self, building: EntityId) -> Option<Cell> {
        let slot = self.entities.slot(building)?;
        if !matches!(self.entities.kind(slot), EntityKind::Building(_)) {
            return None;
        }
        self.production.rally(slot)
    }

    /// Set or clear a rally point. `false` for a stale id or a non-building. A
    /// rally cell that is out of bounds or blocked is rejected.
    pub fn set_rally(&mut self, building: EntityId, cell: Option<Cell>) -> bool {
        let Some(slot) = self.entities.slot(building) else {
            return false;
        };
        if !matches!(self.entities.kind(slot), EntityKind::Building(_)) {
            return false;
        }
        if let Some(c) = cell {
            let width = self.scenario.width();
            let height = self.scenario.height();
            if c.x >= width || c.y >= height {
                return false;
            }
            if self.nav.blocked()[(c.x + c.y * width) as usize] {
                return false;
            }
        }
        self.production.set_rally(slot, cell);
        true
    }

    /// Supply reserved by every live production queue.
    pub fn reserved_supply(&self) -> u32 {
        let mut total = 0;
        for slot in 0..self.entities.slot_count() {
            if !self.entities.alive(slot) {
                continue;
            }
            if !matches!(self.entities.kind(slot), EntityKind::Building(_)) {
                continue;
            }
            for &k in self.production.queue(slot).entries() {
                total += supply_cost(k);
            }
        }
        total
    }

    /// Advance one fixed 1/60 s step.
    ///
    /// Systems are added by later tickets and each one runs at a fixed point in
    /// this order, so a reordering is a visible diff rather than an accident:
    /// 1. commands, 2. camera, 3. construction, 4. production, 5. orders,
    /// 6. movement, 7. supply recount.
    ///
    /// Today the tick counter, the camera pan (2), the construction system (3),
    /// the production system (4), the gather system (5), the movement system
    /// (6) and the supply recount (7) run, followed by pruning the selection of
    /// anything that died this tick — last, so a unit that died on this tick is
    /// out of the selection before anything reads it next tick.
    pub fn tick(&mut self) {
        self.tick_index += 1;
        self.entities.collect_live(&mut self.live_scratch);
        self.camera_system();
        self.construction();
        self.production_system();
        self.gather();
        self.movement();
        self.supply_recount();
        self.selection.retain_live(&self.entities);
    }

    /// System 2: apply one tick of the keyboard and edge pan the input layer
    /// set.
    ///
    /// Both directions are **screen** space — that is what a key or a screen
    /// edge gives you — so each is converted to its cell-space cardinal basis
    /// before it moves the centre. The two sources are additive and each
    /// keeps its own persisted speed; neither is diagonal-normalised.
    fn camera_system(&mut self) {
        let kb = screen_axes_to_cells(self.keyboard_pan_dir);
        let ed = screen_axes_to_cells(self.edge_pan_dir);
        let dx = kb[0] * self.keyboard_pan_speed + ed[0] * self.edge_pan_speed;
        let dy = kb[1] * self.keyboard_pan_speed + ed[1] * self.edge_pan_speed;
        self.camera.pan_cells(dx * TICK_DT, dy * TICK_DT);
    }

    /// System 3: advance every attended construction site by one tick, finish
    /// sites that reach their target, and clear the orders of workers whose
    /// site just finished.
    ///
    /// Runs before orders (5) and movement (6), so a site that finishes this
    /// tick is finished for everything downstream.
    fn construction(&mut self) {
        // Pass A: which sites have an attending worker this tick?
        self.build_attend.fill(false);
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            let Order::Build { site, goal, .. } = self.orders.get(slot) else {
                continue;
            };
            if !matches!(self.entities.kind(slot), EntityKind::Unit(UnitKind::Worker)) {
                continue;
            }
            let Some(site_slot) = self.entities.slot(site) else {
                self.orders.clear(slot);
                continue;
            };
            let EntityKind::Building(b) = self.entities.kind(site_slot) else {
                self.orders.clear(slot);
                continue;
            };
            if self.entities.progress_target(site_slot) == 0 {
                self.orders.clear(slot);
                continue;
            }
            if self.approach_done(
                self.entities.position(slot),
                goal,
                site_slot,
                b.footprint_cells(),
                UnitKind::Worker,
            ) {
                self.build_attend[site_slot] = true;
            }
        }

        // Pass B: advance every attended site by exactly one tick.
        self.finished.clear();
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            let EntityKind::Building(b) = self.entities.kind(slot) else {
                continue;
            };
            let target = self.entities.progress_target(slot);
            if target == 0 {
                continue;
            }
            if !self.build_attend[slot] {
                continue;
            }
            let p = self.entities.progress(slot) + 1;
            if p < target {
                self.entities.set_progress(slot, p, target);
            } else if self.finish_site(slot, b) {
                self.finished.push(self.entities.id_at(slot).expect("live"));
            }
            // A completion that could not evacuate every body it covers is
            // simply not applied: progress stays one tick short of its target,
            // the site stays walkable, and the attempt is repeated next tick.
        }
        // Each finish above rebuilt the inflated centre mask for itself (a
        // later completion on the same tick has to see an earlier one as
        // solid); the pooled fields are dropped once for the whole batch
        // rather than once per finished building.
        if !self.finished.is_empty() {
            let replaced = self
                .nav
                .replace_blocked_mask(self.static_nav.center_blocked());
            debug_assert!(
                replaced.is_ok(),
                "pool and static_nav grids must agree in size"
            );
        }

        // Clear the orders of every worker that was building something now
        // finished.
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            if let Order::Build { site, .. } = self.orders.get(slot)
                && self.finished.contains(&site)
            {
                self.orders.clear(slot);
            }
        }
    }

    /// Whether a unit standing at `p` has finished its approach to the entity
    /// at `target_slot`.
    ///
    /// Two ways, one rule — "it got where it was sent":
    ///
    /// - it is within the target's reach, measured (as T3's
    ///   [`adaptive_reach`] requires) from the *anchor* the group shares. The
    ///   anchor is the nearest legal approach cell, so this stays a tight,
    ///   bounded distance whatever the formation does;
    /// - or it is standing on its own assigned slot. A slot on the outer
    ///   rings of a large formation can sit farther out than the anchor's own
    ///   reach, and a unit that has arrived where it was routed must be able
    ///   to finish its order rather than grind against the neighbour standing
    ///   between it and the anchor.
    fn approach_done(
        &self,
        p: [f32; 2],
        goal: FormationGoal,
        target_slot: usize,
        edge: u32,
        kind: UnitKind,
    ) -> bool {
        let target_pos = self.entities.position(target_slot);
        let anchor_dist = rect_distance(goal.anchor_center(), target_pos, edge);
        rect_distance(p, target_pos, edge) <= adaptive_reach(kind, anchor_dist)
            || dist2(p, goal.slot_center()) <= FORMATION_ARRIVAL_CELLS * FORMATION_ARRIVAL_CELLS
    }

    /// Turn the site at `slot` into a finished building — but only if every
    /// body its footprint would swallow can be given a legal place to stand
    /// first. `false` leaves the world **exactly** as it was.
    ///
    /// A unit is not an obstruction to placement (see
    /// [`super::build::placement_valid`]), so a building routinely finishes on
    /// top of one, and a site is walkable until it finishes, so its own
    /// builder is usually standing in it. Since T4 a body may never penetrate
    /// static geometry, so "stamp the footprint, then shove whoever is inside"
    /// is not available: it would create the illegal state it then tries to
    /// repair, and a shove with nowhere to go would leave a body sealed inside
    /// a solid.
    ///
    /// So the whole transition is planned before any of it is applied:
    ///
    /// 1. every live body the proposed footprint penetrates is an evacuee;
    ///    every other live body is an obstacle to the plan, where it stands;
    /// 2. evacuees are placed one at a time, in the same tick-rotated priority
    ///    the movement sweep uses, each taking the nearest legal free centre
    ///    to where it stands — with the proposed footprint excluded, since it
    ///    is not in [`StaticNav`] yet, and with every centre already handed
    ///    out in this plan counted as occupied;
    /// 3. one evacuee with nowhere to go discards the whole plan, and the site
    ///    holds at `build_ticks - 1` and stays walkable;
    /// 4. only a complete plan is committed — and then, in the same tick, the
    ///    building is marked finished, stamped into the static masks, and its
    ///    supply granted.
    ///
    /// The centre mask is rebuilt here, per completed building rather than
    /// once for the tick's batch, because two sites can finish on the same
    /// tick: the second one's evacuation plan must see the first one as solid
    /// ground, not as the walkable site it was at the top of the tick. The
    /// pooled fields are still replaced once, by the caller, for the whole
    /// batch.
    fn finish_site(&mut self, slot: usize, b: BuildingKind) -> bool {
        let edge = b.footprint_cells();
        let min = footprint_min(self.entities.position(slot), edge);

        // 1. Split every live body into "this footprint covers it" and "this
        //    footprint does not", the second being the plan's fixed obstacles.
        self.evac_units.clear();
        self.body_scratch.clear();
        for s in 0..self.entities.slot_count() {
            if !self.entities.alive(s) {
                continue;
            }
            let EntityKind::Unit(kind) = self.entities.kind(s) else {
                continue;
            };
            let p = self.entities.position(s);
            if circle_clear_of_cell_rect(p, kind.body_radius_cells(), min, edge) {
                self.body_scratch.push(p);
            } else {
                self.evac_units.push(s);
            }
        }

        // 2. Plan one destination per evacuee, whole or not at all.
        let n = self.evac_units.len();
        self.evac_to.clear();
        self.evac_to.resize(n, [0.0, 0.0]);
        if n > 0 {
            let start = (self.tick_index % n as u64) as usize;
            for k in 0..n {
                let i = (start + k) % n;
                let from = self.entities.position(self.evac_units[i]);
                let Some(to) = nearest_free_body_center(
                    &self.static_nav,
                    &self.body_scratch,
                    None,
                    Some((min, edge)),
                    from,
                ) else {
                    // 3. Nowhere to put this body: nothing has been mutated,
                    //    so the site simply does not finish this tick.
                    return false;
                };
                self.evac_to[i] = to;
                // Every centre already handed out is occupied for the rest of
                // this plan.
                self.body_scratch.push(to);
            }
        }

        // 4. Commit: the moves, then the building itself.
        for i in 0..n {
            self.entities
                .set_position(self.evac_units[i], self.evac_to[i]);
        }
        self.entities.set_progress(slot, 0, 0);
        self.static_nav.stamp_finished_building(min, edge);
        self.static_nav
            .rebuild_center_blocked(RTS_UNIT_BODY_RADIUS_CELLS);
        self.supply.grant_cap(supply_grant(b));
        true
    }

    /// System 4: advance every finished building's production queue by one
    /// tick, and place the head on the grid — outside every body already
    /// standing there — once it is ready.
    ///
    /// Runs after construction, so a Barracks that finished this tick can
    /// already hold a queue, and before orders, so a unit produced this tick
    /// can be given its rally order in the same tick. Buildings are processed
    /// ascending by slot, so two producers finishing on the same tick resolve
    /// in a fixed order and the second sees the first's unit as a body.
    ///
    /// Ticking and popping are separate ([`ProductionQueue::tick_head`] /
    /// [`ProductionQueue::pop_ready`]) because placing a unit can fail: the
    /// preferred spot may be occupied and every legal centre on the grid taken
    /// or blocked, or the entity store may be full. A head that cannot be
    /// placed stays ready — paid for, its supply still reserved — and is
    /// retried next tick, rather than being spawned into another body or
    /// silently dropped. The player keeps exactly what they bought.
    fn production_system(&mut self) {
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            if !matches!(self.entities.kind(slot), EntityKind::Building(_)) {
                continue;
            }
            if self.entities.progress_target(slot) != 0 {
                continue; // still a site
            }
            if self.production.queue(slot).head().is_none() {
                continue;
            }
            self.production.queue_mut(slot).tick_head();
            if !self.production.queue(slot).head_ready() {
                continue;
            }
            let done = self
                .production
                .queue(slot)
                .head()
                .expect("a ready head is a head");

            // The approach cell is where the player expects the unit: the
            // spawn is the nearest legal free body centre to it, which is that
            // cell itself whenever nothing is standing on it.
            let building_id = self.entities.id_at(slot).expect("live");
            let (cell, _) =
                entity_approach_cell(&self.static_nav, &self.entities, building_id, done);
            let preferred = [cell.x as f32 + 0.5, cell.y as f32 + 0.5];
            self.collect_unit_bodies_into_scratch();
            let Some(pos) = nearest_free_body_center(
                &self.static_nav,
                &self.body_scratch,
                None,
                None,
                preferred,
            ) else {
                // Nowhere legal and free on the whole grid. The ready head
                // waits.
                continue;
            };
            let Some(id) = self
                .entities
                .spawn(EntityKind::Unit(done), OWNER_PLAYER, pos)
            else {
                // Store full: same wait, same reason.
                continue;
            };
            let popped = self.production.queue_mut(slot).pop_ready();
            debug_assert_eq!(popped, Some(done), "a ready head must pop what it produced");
            // `live_scratch` was collected before production. Include this unit
            // in later systems and the same tick's supply recount; the movement
            // sweep collects it too, so it is a body from this tick on.
            self.live_scratch.push(id.index as usize);
            if let Some(rally) = self.production.rally(slot) {
                // Ignores its own return; a blocked rally is a no-op.
                self.order_move(id, rally);
            }
        }
    }

    /// Fill [`Self::body_scratch`] with every live unit's body position,
    /// ascending slot — the obstacle set a placement plan scores against.
    fn collect_unit_bodies_into_scratch(&mut self) {
        self.body_scratch.clear();
        for slot in 0..self.entities.slot_count() {
            if !self.entities.alive(slot) {
                continue;
            }
            if !matches!(self.entities.kind(slot), EntityKind::Unit(_)) {
                continue;
            }
            self.body_scratch.push(self.entities.position(slot));
        }
    }

    /// System 7: recompute `Supply::used` from scratch — live units plus every
    /// live production queue's reservations — rather than maintaining it
    /// incrementally, so it can never drift.
    fn supply_recount(&mut self) {
        let mut used = 0u32;
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            if let EntityKind::Unit(k) = self.entities.kind(slot) {
                used += supply_cost(k);
            }
        }
        used += self.reserved_supply();
        self.supply.set_used(used);
    }

    /// System 5: advance every gathering worker's round trip one step.
    ///
    /// Runs before movement, so a phase change decided this tick is walked
    /// on this same tick — otherwise the round trip would lag its own state
    /// by one frame.
    fn gather(&mut self) {
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            let Order::Gather { node, phase } = self.orders.get(slot) else {
                continue;
            };

            // The node may have been removed; the order dies with it.
            let Some(node_slot) = self.entities.slot(node) else {
                self.orders.clear(slot);
                continue;
            };
            let EntityKind::Node(res) = self.entities.kind(node_slot) else {
                self.orders.clear(slot);
                continue;
            };
            let p = self.entities.position(slot);

            match phase {
                GatherPhase::ToNode { goal, .. } => {
                    if self.entities.amount(node_slot) == 0 {
                        self.orders.clear(slot);
                        continue;
                    }
                    if self.approach_done(p, goal, node_slot, 1, UnitKind::Worker) {
                        self.orders.set(
                            slot,
                            Order::Gather {
                                node,
                                phase: GatherPhase::Mining {
                                    ticks_left: GATHER_TICKS,
                                },
                            },
                        );
                    }
                    // else: leave it; the movement system walks it down
                    // field_slot toward the node cell.
                }
                GatherPhase::Mining { ticks_left } => {
                    if ticks_left > 1 {
                        self.orders.set(
                            slot,
                            Order::Gather {
                                node,
                                phase: GatherPhase::Mining {
                                    ticks_left: ticks_left - 1,
                                },
                            },
                        );
                    } else {
                        // Load up: take min(capacity, remaining).
                        let take = WORKER_CARRY_CAPACITY.min(self.entities.amount(node_slot));
                        if take == 0 {
                            self.orders.clear(slot);
                            continue;
                        }
                        self.entities
                            .set_amount(node_slot, self.entities.amount(node_slot) - take);
                        self.entities.set_carry(slot, Some((res, take)));
                        match self.nearest_drop_off(p) {
                            Some(d) => {
                                let (cell, _) = entity_approach_cell(
                                    &self.static_nav,
                                    &self.entities,
                                    d,
                                    UnitKind::Worker,
                                );
                                match self.nav.acquire(cell) {
                                    Ok(field) => self.orders.set(
                                        slot,
                                        Order::Gather {
                                            node,
                                            phase: GatherPhase::Returning { drop_off: d, field },
                                        },
                                    ),
                                    Err(_) => self.orders.clear(slot),
                                }
                            }
                            // nowhere to deliver: stop, holding the cargo
                            None => self.orders.clear(slot),
                        }
                    }
                }
                GatherPhase::Returning { drop_off, .. } => {
                    let Some(d_slot) = self.entities.slot(drop_off) else {
                        self.orders.clear(slot);
                        continue;
                    };
                    let EntityKind::Building(b) = self.entities.kind(d_slot) else {
                        self.orders.clear(slot);
                        continue;
                    };
                    let (_, drop_off_cell_dist) = entity_approach_cell(
                        &self.static_nav,
                        &self.entities,
                        drop_off,
                        UnitKind::Worker,
                    );
                    if rect_distance(p, self.entities.position(d_slot), b.footprint_cells())
                        <= adaptive_reach(UnitKind::Worker, drop_off_cell_dist)
                    {
                        if let Some((kind, amount)) = self.entities.carry(slot) {
                            match kind {
                                ResourceKind::Crystal => self.resources.credit(Resources {
                                    crystal: amount,
                                    gas: 0,
                                }),
                                ResourceKind::Gas => self.resources.credit(Resources {
                                    crystal: 0,
                                    gas: amount,
                                }),
                            }
                            self.entities.set_carry(slot, None);
                        }
                        if self.entities.amount(node_slot) == 0 {
                            self.orders.clear(slot);
                            continue;
                        }
                        // Re-plan this one worker's approach slot rather than
                        // reusing the one it left: another worker may have
                        // taken it while this one was hauling, and a slot two
                        // bodies cannot share is not an approach.
                        let id = self.entities.id_at(slot).expect("live worker");
                        if self
                            .order_group(&[id], GroupTarget::Node(node), None)
                            .is_err()
                        {
                            self.orders.clear(slot);
                        }
                    }
                }
            }
        }
    }

    /// System 6: walk every unit under a `Move` order, or a `Gather` order
    /// mid-transit (`ToNode` or `Returning`), one step down its field —
    /// **without ever merging two unit bodies**.
    ///
    /// Three phases, in this order:
    ///
    /// 1. collect every live unit's slot and body position
    ///    ([`Self::collect_unit_bodies`]);
    /// 2. **testkit builds only, and only when a test hook armed it**: repair
    ///    any penetration the world was handed
    ///    ([`Self::repair_body_overlaps`]) — nothing in the game can produce
    ///    one, only [`Self::force_position_for_test`] and a raw store mutation
    ///    can, so the shipping tick skips straight from phase 1 to phase 3;
    /// 3. propose and commit one candidate step per unit, sequentially, in an
    ///    order rotated by the tick index.
    ///
    /// **Why sequential, and why that is a proof.** Phase 3 accepts a
    /// candidate only when its whole swept segment clears every *other*
    /// unit's current body — final position for a unit already processed this
    /// tick, last tick's position for one not yet processed. So by induction
    /// on the traversal: the set of committed bodies starts non-overlapping
    /// (no in-game path can hand this system a merged pair, and phase 2
    /// repairs the ones a test hook forces), and each accepted candidate is
    /// non-overlapping against every member of that set at the moment it joins
    /// it, including the ones that will move later — because they have not
    /// moved yet and their own candidates will in turn be tested against this
    /// one. A rejected candidate simply does not move, which cannot create an
    /// overlap either.
    ///
    /// The one mutation that touches a body other than the mover's is
    /// [`Self::try_push_chain`], and it preserves the same induction: it
    /// commits nothing unless the displaced body's own new position is clear
    /// of the mover's candidate and of every other body (swept, so it cannot
    /// tunnel), which is exactly the property the step above assumes of the
    /// set it joins. Therefore no completed tick leaves two bodies merged.
    ///
    /// The traversal start rotates by `tick_index % unit_count` so that
    /// contention is not settled by slot number forever: a unit queued behind
    /// another gets the first proposal on its share of ticks instead of being
    /// starved by a lower-slot neighbour. The rotation is derived from the
    /// tick counter, which the state hash already covers, so it adds no cursor
    /// state a replay would have to carry.
    ///
    /// The step obeys the same admissibility rule the horde walk obeys
    /// ([`super::orders::step_admissible`]), so a unit can never be placed in a
    /// walkable-but-unreachable pocket it could not then leave.
    ///
    /// Every order re-checks that the field it cached is still the field it
    /// asked for, and re-acquires when it is not — see step 2 in
    /// [`Self::step_one_unit`].
    fn movement(&mut self) {
        self.collect_unit_bodies();
        // Always describes the last tick and never an older one, in every
        // build — the repair pass below is the only thing that can set it,
        // and the shipping build has no repair pass.
        self.last_tick_error = None;
        #[cfg(feature = "testkit")]
        if self.repair_armed {
            self.repair_body_overlaps();
            if self.last_tick_error.is_some() {
                // The world was handed a penetration nothing could repair.
                // Moving anyone now would build on an illegal state; hold last
                // tick's positions, stay armed so the next tick retries, and
                // let the caller see `last_tick_error`.
                return;
            }
            self.repair_armed = false;
        }
        let n = self.unit_scratch.len();
        if n == 0 {
            return;
        }
        let start = (self.tick_index % n as u64) as usize;
        for k in 0..n {
            self.step_one_unit((start + k) % n);
        }
    }

    /// Phase 1: every live unit slot, ascending, with its current body
    /// position, into the two buffers reserved at load.
    ///
    /// Scanned straight off the store rather than off `live_scratch`, because
    /// a unit produced earlier *this* tick is appended to `live_scratch` at
    /// whatever free slot it reused — which need not be ascending, and the
    /// traversal rotation must be over a stable, ascending order.
    fn collect_unit_bodies(&mut self) {
        self.unit_scratch.clear();
        self.candidate_pos.clear();
        self.pushed.clear();
        for slot in 0..self.entities.slot_count() {
            if !self.entities.alive(slot) {
                continue;
            }
            if !matches!(self.entities.kind(slot), EntityKind::Unit(_)) {
                continue;
            }
            self.unit_scratch.push(slot);
            self.candidate_pos.push(self.entities.position(slot));
            self.pushed.push(false);
        }
    }

    /// Phase 2, **testkit only**: move any unit that starts the tick merged
    /// into another body to the nearest legal free cell centre, in the same
    /// rotated order phase 3 walks.
    ///
    /// Only an explicitly invalid state reaches this, and only a test can
    /// build one: every in-game path that places a unit (seeding, movement,
    /// production, the push off a finished building) already respects bodies,
    /// so the shipping tick compiles this pass out entirely rather than
    /// scanning every pair of live bodies for a penetration that cannot exist.
    /// [`Self::force_position_for_test`] and [`Self::entities_mut`] arm it;
    /// nothing else does. When a penetration *is* found, the first penetrating
    /// unit in the rotated order is the one relocated — its partner is then no
    /// longer penetrating and is left alone, so a pair costs one relocation,
    /// not two.
    ///
    /// A unit with nowhere legal to go stashes [`TickError::UnrepairableOverlap`]
    /// rather than letting the tick complete with a merged pair unreported.
    /// The caller then leaves [`Self::repair_armed`] set, so the pass retries
    /// on the next tick.
    #[cfg(feature = "testkit")]
    fn repair_body_overlaps(&mut self) {
        self.repair_runs += 1;
        let n = self.unit_scratch.len();
        if n < 2 {
            return;
        }
        let start = (self.tick_index % n as u64) as usize;
        for k in 0..n {
            let i = (start + k) % n;
            if !self.body_penetrates_any(i) {
                continue;
            }
            let preferred = self.candidate_pos[i];
            match nearest_free_body_center(
                &self.static_nav,
                &self.candidate_pos,
                Some(i),
                None,
                preferred,
            ) {
                Some(p) => {
                    self.entities.set_position(self.unit_scratch[i], p);
                    self.candidate_pos[i] = p;
                }
                None => self.last_tick_error = Some(TickError::UnrepairableOverlap),
            }
        }
    }

    /// Whether unit `i`'s current body penetrates any other unit's.
    fn body_penetrates_any(&self, i: usize) -> bool {
        let r = self.body_radius(i);
        self.candidate_pos.iter().enumerate().any(|(j, &q)| {
            j != i && units_overlap(self.candidate_pos[i], r, q, self.body_radius(j))
        })
    }

    /// What the swept segment `from -> to` of unit `i`'s body runs into.
    ///
    /// Unit `i` is skipped against itself; every other unit counts, whatever
    /// its owner, kind or order — an idle, mining or site-attending unit is a
    /// body exactly like a walking one.
    fn body_sweep_hit(&self, i: usize, from: [f32; 2], to: [f32; 2]) -> BodySweep {
        let r = self.body_radius(i);
        let mut hit = [0usize; MAX_PUSHED_BODIES];
        let mut count = 0usize;
        for (j, &q) in self.candidate_pos.iter().enumerate() {
            if j == i || !moving_circle_hits_point(from, to, r, q, self.body_radius(j)) {
                continue;
            }
            if count == MAX_PUSHED_BODIES {
                return BodySweep::TooMany;
            }
            hit[count] = j;
            count += 1;
        }
        if count == 0 {
            BodySweep::Clear
        } else {
            BodySweep::Bodies { hit, count }
        }
    }

    /// Where `body` lands when a pusher of `pusher_radius` centred at
    /// `pusher_at` shoves it out of its own space: along the contact normal,
    /// far enough to clear contact by one `step`.
    ///
    /// `None` when there is no usable normal — concentric bodies have none,
    /// and a body the pusher's *sweep* clipped in passing (already clear of
    /// the pusher's end position) has none worth trusting either.
    ///
    /// The extra `step` is what keeps the displaced body clear of contact by a
    /// whole step rather than balanced exactly on it, so the legality checks
    /// resolve without an epsilon anywhere in the collision rule itself.
    fn push_target(
        &self,
        pusher_at: [f32; 2],
        pusher_radius: f32,
        body: usize,
        step: f32,
    ) -> Option<[f32; 2]> {
        let q = self.candidate_pos[body];
        let dx = q[0] - pusher_at[0];
        let dy = q[1] - pusher_at[1];
        let d2 = dx * dx + dy * dy;
        if d2 <= 0.0 {
            return None;
        }
        let d = d2.sqrt();
        let depth = (pusher_radius + self.body_radius(body)) - d;
        if depth <= 0.0 {
            return None;
        }
        let push = depth + step;
        Some([q[0] + dx / d * push, q[1] + dy / d * push])
    }

    /// Whether a displaced body may legally travel `from -> to`: inside the
    /// map and clear of static geometry along the whole swept segment, and
    /// admissible by the same rule a walk obeys — a shove must not park a body
    /// in a pocket it could never walk out of.
    fn displacement_is_legal_statically(&self, from: [f32; 2], to: [f32; 2], radius: f32) -> bool {
        if !self.static_nav.sweep_clear(from, to, radius) {
            return false;
        }
        let width = self.scenario.width();
        let height = self.scenario.height();
        let cx = from[0].floor() as i32;
        let cy = from[1].floor() as i32;
        if cx < 0 || cy < 0 || cx >= width as i32 || cy >= height as i32 {
            return false;
        }
        step_admissible(cx, cy, to[0], to[1], width, height, self.nav.blocked())
    }

    /// Try to shove the bodies mover `i`'s candidate touches out of its way,
    /// each along its own contact normal, propagating to the bodies *they*
    /// would land on.
    ///
    /// This is the one deliberate deviation from "a rejected candidate simply
    /// stands still": without it, a unit whose field descent points at a
    /// stationary neighbour is frozen for good — flow fields are body-blind,
    /// so nothing would ever re-route it — and the tracked scene's workers,
    /// seeded in a row exactly one body diameter apart, could never leave
    /// their own cluster to gather or build. Genre-standard behaviour, and an
    /// amendment to `docs/ADR/017`, which records why it is not the bounded
    /// relaxation that ADR rejects.
    ///
    /// The rules that keep it from becoming soft collision:
    /// - **Bounded, and iterative.** At most [`MAX_PUSHED_BODIES`] bodies move
    ///   in total, no further than [`MAX_PUSH_DEPTH`] links from the mover,
    ///   and each body moves at most once per tick ([`Self::pushed`]). A
    ///   worklist, never recursion, so the cost of a step has a hard ceiling.
    /// - **Whole-or-nothing, over the entire chain.** Every displaced body
    ///   must end fully legal: inside the map, clear of static geometry along
    ///   its own swept segment, admissible, non-overlapping with the mover's
    ///   candidate, with every body that is *not* displaced (swept, so a shove
    ///   cannot tunnel one body through a third), and with every other
    ///   displaced body's final position. One illegal link rejects the mover's
    ///   candidate whole and nothing moves at all.
    /// - **State is untouched.** Only positions move; order, cargo, facing and
    ///   animation stay exactly as they were.
    ///
    /// The deepest link in a chain moves at most
    /// [`MAX_DISPLACEMENT_STEPS`] walk steps — 2.0 cells at today's speeds, not
    /// "under one cell" as this used to claim — so two displaced bodies have a
    /// relative displacement of at most twice that. Crossing takes a whole body
    /// diameter, so while that bound holds the endpoint check between two
    /// displaced bodies cannot miss a crossing. It is a real ceiling on unit
    /// speed, and [`MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC`] asserts it at
    /// compile time.
    fn try_push_chain(
        &mut self,
        i: usize,
        hit: [usize; MAX_PUSHED_BODIES],
        count: usize,
        candidate: [f32; 2],
        step: f32,
    ) -> bool {
        let ri = self.body_radius(i);
        let mut moved = [Displacement::NONE; MAX_PUSHED_BODIES];
        let mut n_moved = 0usize;

        // Depth 1: the bodies the mover's own swept candidate touches.
        for &j in &hit[..count] {
            if self.pushed[j] || n_moved == MAX_PUSHED_BODIES {
                return false;
            }
            let Some(to) = self.push_target(candidate, ri, j, step) else {
                return false;
            };
            moved[n_moved] = Displacement {
                body: j,
                to,
                depth: 1,
            };
            n_moved += 1;
        }

        // Depths 2..=MAX_PUSH_DEPTH: whatever each displacement runs into.
        let mut head = 0usize;
        while head < n_moved {
            let d = moved[head];
            head += 1;
            let from = self.candidate_pos[d.body];
            let r = self.body_radius(d.body);
            if !self.displacement_is_legal_statically(from, d.to, r) {
                return false;
            }
            // The mover is never displaced by its own push.
            if units_overlap(candidate, ri, d.to, r) {
                return false;
            }
            for k in 0..self.candidate_pos.len() {
                if k == i
                    || k == d.body
                    || moved[..n_moved].iter().any(|m| m.body == k)
                    || !moving_circle_hits_point(
                        from,
                        d.to,
                        r,
                        self.candidate_pos[k],
                        self.body_radius(k),
                    )
                {
                    continue;
                }
                if d.depth >= MAX_PUSH_DEPTH || self.pushed[k] || n_moved == MAX_PUSHED_BODIES {
                    return false;
                }
                let Some(to) = self.push_target(d.to, r, k, step) else {
                    return false;
                };
                moved[n_moved] = Displacement {
                    body: k,
                    to,
                    depth: d.depth + 1,
                };
                n_moved += 1;
            }
        }

        // Every displaced body against every other displaced body, at their
        // final positions.
        for a in 0..n_moved {
            for b in (a + 1)..n_moved {
                if units_overlap(
                    moved[a].to,
                    self.body_radius(moved[a].body),
                    moved[b].to,
                    self.body_radius(moved[b].body),
                ) {
                    return false;
                }
            }
        }

        for m in &moved[..n_moved] {
            self.entities.set_position(self.unit_scratch[m.body], m.to);
            self.candidate_pos[m.body] = m.to;
            self.pushed[m.body] = true;
        }
        true
    }

    /// Body radius of the unit at index `i` of [`Self::unit_scratch`].
    fn body_radius(&self, i: usize) -> f32 {
        match self.entities.kind(self.unit_scratch[i]) {
            EntityKind::Unit(k) => k.body_radius_cells(),
            // `unit_scratch` holds unit slots only.
            _ => unreachable!("unit_scratch holds units"),
        }
    }

    /// Try one candidate step for unit `i`: every gate, then commit.
    ///
    /// `true` when the candidate (and any push chain it needed) was committed,
    /// `false` when it was rejected — in which case **nothing** has been
    /// mutated, which is what makes trying several candidates in a row safe.
    #[allow(clippy::too_many_arguments)]
    fn try_commit_step(
        &mut self,
        i: usize,
        slot: usize,
        from: [f32; 2],
        candidate: [f32; 2],
        radius: f32,
        cx: i32,
        cy: i32,
        step: f32,
    ) -> bool {
        let width = self.scenario.width();
        let height = self.scenario.height();
        if !step_admissible(
            cx,
            cy,
            candidate[0],
            candidate[1],
            width,
            height,
            self.nav.blocked(),
        ) || !self.static_nav.sweep_clear(from, candidate, radius)
        {
            return false;
        }
        let clear = match self.body_sweep_hit(i, from, candidate) {
            BodySweep::Clear => true,
            BodySweep::Bodies { hit, count } => self.try_push_chain(i, hit, count, candidate, step),
            // A whole crowd at once is not a step. The mover waits.
            BodySweep::TooMany => false,
        };
        if clear {
            self.entities.set_position(slot, candidate);
            self.candidate_pos[i] = candidate;
        }
        clear
    }

    /// Phase 3, for one unit: propose this tick's step and commit it only if
    /// the whole swept body clears the static world, and every other body is
    /// either clear of it or can legally be shoved aside.
    fn step_one_unit(&mut self, i: usize) {
        let width = self.scenario.width();
        let height = self.scenario.height();
        let slot = self.unit_scratch[i];
        let EntityKind::Unit(kind) = self.entities.kind(slot) else {
            return;
        };
        let order = self.orders.get(slot);
        let (goal, field) = match order {
            Order::Move { goal, field } => (goal, field),
            Order::Gather {
                node,
                phase: GatherPhase::ToNode { goal, field },
            } => {
                if self.entities.slot(node).is_none() {
                    return;
                }
                (goal, field)
            }
            Order::Gather {
                phase: GatherPhase::Returning { drop_off, field },
                ..
            } => {
                if self.entities.slot(drop_off).is_none() {
                    return;
                }
                // A hauler has no formation: the whole shift converges on one
                // drop-off, and the reach test against the building's
                // footprint — not a slot — is what ends the trip.
                let (cell, _) =
                    entity_approach_cell(&self.static_nav, &self.entities, drop_off, kind);
                (FormationGoal::at(cell), field)
            }
            Order::Build { site, goal, field } => {
                let Some(site_slot) = self.entities.slot(site) else {
                    return;
                };
                let EntityKind::Building(b) = self.entities.kind(site_slot) else {
                    return;
                };
                // A worker that has reached the site stops and attends it;
                // it does not clear the order, since the construction
                // system — not the mover — decides when a `Build` order ends.
                if self.approach_done(
                    self.entities.position(slot),
                    goal,
                    site_slot,
                    b.footprint_cells(),
                    kind,
                ) {
                    return;
                }
                (goal, field)
            }
            // Idle, and Mining (a mining worker stands still).
            _ => return,
        };
        let is_move_order = matches!(order, Order::Move { .. });
        let dest = goal.anchor;

        let p = self.entities.position(slot);
        // 1. Arrival, against this unit's **own slot** centre, and only with
        //    its body clear of every other.
        //
        //    Only `Order::Move` stops here. A gathering or building worker's
        //    real completion condition is a reach test against a footprint
        //    rectangle ([`Self::approach_done`]), not proximity to a cell
        //    centre. Freezing such an order here, before its own reach test
        //    is satisfied, would strand the unit short of the building it
        //    was sent to.
        if is_move_order
            && dist2(p, goal.slot_center()) <= FORMATION_ARRIVAL_CELLS * FORMATION_ARRIVAL_CELLS
            && !self.body_penetrates_any(i)
        {
            self.orders.clear(slot);
            return;
        }

        // 2. Terminal steering: inside the capture ring a unit stops
        //    descending the shared field and walks straight at its own slot.
        //
        //    This is local formation placement, not pathfinding: one straight
        //    segment, gated by exactly the rules a field step is gated by, and
        //    it never touches the pool. Outside the ring, or when the direct
        //    segment is refused, the shared field below is what moves the
        //    unit — which is also how a unit shoved out of its lane finds its
        //    way back in.
        let cell_x = p[0].floor() as i32;
        let cell_y = p[1].floor() as i32;
        if cell_x < 0 || cell_y < 0 || cell_x >= width as i32 || cell_y >= height as i32 {
            return;
        }
        let step = unit_speed(kind) * TICK_DT;
        let body = kind.body_radius_cells();
        if dist2(p, goal.anchor_center()) <= goal.capture_radius2() {
            let blocked =
                self.static_nav.center_blocked()[(goal.slot.x + goal.slot.y * width) as usize];
            if blocked {
                // The slot was built over after the plan was made. A move
                // order aimed at a cell no body may stand on can never
                // finish, so it stops here rather than hovering forever; a
                // gather or build order has its own reach test and falls
                // through to the shared field.
                if is_move_order {
                    self.orders.clear(slot);
                    return;
                }
            } else {
                let target = goal.slot_center();
                let dx = target[0] - p[0];
                let dy = target[1] - p[1];
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.0 {
                    // Clamped, never overshot: the last step of a walk lands
                    // on the slot centre exactly, which is what makes the
                    // arrival test above reachable at a whole step per tick.
                    let candidate = if len <= step {
                        target
                    } else {
                        [p[0] + dx / len * step, p[1] + dy / len * step]
                    };
                    if self.try_commit_step(i, slot, p, candidate, body, cell_x, cell_y, step) {
                        self.advance_animation(slot, dx, dy);
                        return;
                    }
                }
            }
        }

        // 3. Re-path if the cached field is no longer the field this order
        //    asked for.
        //
        //    A slot is rebuilt for someone else's destination on an LRU
        //    miss, and a building finishing drops every key, so the handle
        //    an order cached may now name a field to somewhere else — or
        //    the same place across a wall that did not exist when it was
        //    built. Riding one is how a unit walks into a building that
        //    went up ten seconds ago, and how an order that is not
        //    `Order::Move` (which at least stops) hangs forever.
        let field = if self.nav.is_current(field, dest) {
            field
        } else {
            match self.nav.acquire(dest) {
                Ok(fresh) => {
                    self.orders.set(slot, order.with_field(fresh));
                    fresh
                }
                // No field can be built to `dest` any more — it is off the
                // grid or has been built over. Stop, rather than keep an
                // order alive that nothing can finish.
                Err(_) => {
                    self.orders.clear(slot);
                    return;
                }
            }
        };

        // 4. Sample the field at the unit's own cell.
        let (cx, cy) = (cell_x, cell_y);
        let (vx, vy) = self.nav.field(field.slot).vector_at(cx as u32, cy as u32);
        if vx == 0.0 && vy == 0.0 {
            // A zero vector means one of two things: this cell is the
            // field's own sink (cost 0 — the unit already stands on `dest`
            // itself, which an approach cell close to its adaptive reach
            // can leave the unit sitting on exactly), or `dest` is
            // genuinely unreachable from here (cost never resolved).
            // `FieldPool::reachable` is the one source of truth for which:
            // only the second case is a dead order. The first is not a
            // failure to stop on — the reach tests above (the gather
            // system's, and the `Order::Build` guard above) own completion
            // and will see it next tick from wherever this cell leaves the
            // unit.
            if !self.nav.reachable(
                field.slot as usize,
                Cell {
                    x: cx as u32,
                    y: cy as u32,
                },
            ) {
                self.orders.clear(slot);
            }
            return;
        }

        // 5. Propose the step, with the horde's admissibility rule...
        //
        // 6. ...and commit it whole, or not at all. A candidate must clear the
        //    admissibility rule (no corner cut into an unreachable pocket),
        //    then the static world along its whole swept segment, then every
        //    other unit body along that same segment. Sweeps, not endpoints:
        //    at 30 cells/s a body covers half a cell per tick, and an endpoint
        //    test would let a future faster unit step straight over a body.
        //    There is no partial step: a candidate is taken whole or not at
        //    all, and a unit with no legal candidate stands still and
        //    re-proposes next tick.
        //
        //    Two things stop that from meaning "frozen for good", because a
        //    pooled field is body-blind and will never re-route around a unit:
        //    the mover may shove bodies out of the way
        //    ([`Self::try_push_chain`]), and, only when that fails, it may try
        //    the deflected headings in [`MOVE_DEFLECTIONS`].
        for (c, sn) in MOVE_DEFLECTIONS {
            let dx = vx * c - vy * sn;
            let dy = vx * sn + vy * c;
            let candidate = [p[0] + dx * step, p[1] + dy * step];
            if self.try_commit_step(i, slot, p, candidate, body, cx, cy, step) {
                break;
            }
        }
        self.advance_animation(slot, vx, vy);
    }

    /// Face a unit along `(vx, vy)` and advance its walk cycle by one frame.
    fn advance_animation(&mut self, slot: usize, vx: f32, vy: f32) {
        self.entities.set_dir(slot, dir_from_vector(vx, vy));
        let f = (self.entities.frame(slot) + 1) % 4;
        self.entities.set_frame(slot, f);
    }

    /// Exact same-host state digest.
    ///
    /// Covers `tick_index`, live entity count, then every live slot in
    /// ascending order (kind tag, owner, x bits, y bits, dir, frame, progress,
    /// progress_target, amount, carry kind, carry amount), then every live
    /// slot's order, then the selection, production queues and rally points,
    /// then the camera centre, then the pending placement ghost (one tag byte
    /// plus the kind byte), then resources and supply. `f32` goes in
    /// as raw bits, matching `Simulation::state_hash`.
    ///
    /// The camera is in here because it is world state, not view state: a
    /// replay that ends looking somewhere else did not reproduce.
    pub fn state_hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.tick_index.to_le_bytes());
        h.update((self.entities.len() as u64).to_le_bytes());
        self.entities.hash_into(&mut h);
        // Not `live_scratch`: hashing is a `&self` read and must not disturb a
        // buffer the tick owns. This path runs outside the tick, never in it.
        let mut live = Vec::with_capacity(self.entities.len());
        self.entities.collect_live(&mut live);
        self.orders.hash_into(&mut h, &live);
        self.selection.hash_into(&mut h);
        self.production.hash_into(&mut h, &live);
        let center = self.camera.center();
        h.update(center[0].to_bits().to_le_bytes());
        h.update(center[1].to_bits().to_le_bytes());
        match self.placement {
            Placement::None => h.update([0u8, 0u8]),
            Placement::Pending { kind } => h.update([1u8, kind as u8]),
        }
        h.update(self.resources.crystal.to_le_bytes());
        h.update(self.resources.gas.to_le_bytes());
        h.update(self.supply.used().to_le_bytes());
        h.update(self.supply.cap().to_le_bytes());
        h.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rts::entity::RTS_UNIT_BODY_RADIUS_CELLS;

    const W: u32 = 40;
    const H: u32 = 40;

    fn idx(x: u32, y: u32) -> usize {
        (x + y * W) as usize
    }

    /// A 40x40 grid, solid except for a sealed 7 x 7 room at
    /// `[2, 9) x [2, 9)` and a wide open field at `[20, 38) x [2, 38)`.
    ///
    /// A 7-cell room has exactly one legal centre for a 3-cell body \u2014 its
    /// middle, `(5, 5)` \u2014 so the room is a one-body pocket with no route
    /// anywhere. The open field is full of legal centres, all of them
    /// disconnected from the room.
    fn sealed_room_and_open_field() -> StaticNav {
        let mut solids = vec![true; (W * H) as usize];
        for y in 2..9u32 {
            for x in 2..9u32 {
                solids[idx(x, y)] = false;
            }
        }
        for y in 2..38u32 {
            for x in 20..38u32 {
                solids[idx(x, y)] = false;
            }
        }
        StaticNav::from_raw(W, H, solids, RTS_UNIT_BODY_RADIUS_CELLS)
    }

    #[test]
    fn the_sealed_room_holds_exactly_one_legal_centre() {
        let nav = sealed_room_and_open_field();
        let legal: Vec<(u32, u32)> = (0..H)
            .flat_map(|y| (0..W).map(move |x| (x, y)))
            .filter(|&(x, y)| !nav.center_blocked()[idx(x, y)])
            .filter(|&(x, _)| x < 20)
            .collect();
        assert_eq!(
            legal,
            vec![(5, 5)],
            "the room's geometry must leave exactly one legal body centre"
        );
        assert!(
            !nav.connected(Cell { x: 5, y: 5 }, Cell { x: 28, y: 20 }),
            "the room and the open field must be separate regions"
        );
    }

    /// A relocation moves a body; it does not teleport it. When the only free
    /// legal centre near a unit is on the far side of a wall, the answer is
    /// \"nowhere\", not \"through the wall\".
    ///
    /// Without the connectivity rule this returned the nearest cell of the
    /// open field by raw Euclidean distance \u2014 dropping an evacuated, produced
    /// or overlap-repaired body into a pocket it could never have walked to.
    #[test]
    fn a_relocation_never_crosses_into_a_disconnected_region() {
        let nav = sealed_room_and_open_field();
        let occupied = [[5.5f32, 5.5]];

        // The room's one legal centre is taken, and everything else legal is
        // across the wall.
        assert_eq!(
            nearest_free_body_center(&nav, &occupied, None, None, [5.5, 5.5]),
            None,
            "a body in a full one-slot pocket has nowhere to go, and the open \
             field on the far side of the wall is not an answer"
        );

        // Same call with the pocket empty: it still finds the pocket's own
        // centre, so the rule refuses a teleport rather than refusing to work.
        assert_eq!(
            nearest_free_body_center(&nav, &[], None, None, [5.5, 5.5]),
            Some([5.5, 5.5])
        );
    }

    /// The anchor is resolved even when the body is standing somewhere no
    /// body may legally stand \u2014 which is exactly the case a building
    /// finishing on top of one produces. It resolves to the pocket the body
    /// is in, never to the far side of a wall.
    #[test]
    fn an_illegal_start_still_anchors_to_its_own_region() {
        let nav = sealed_room_and_open_field();
        // A corner of the room: legal for nothing, one cell inside the wall's
        // clearance.
        let inside_the_room = [2.5f32, 2.5];
        assert!(nav.center_blocked()[idx(2, 2)]);
        assert_eq!(
            nearest_free_body_center(&nav, &[], None, None, inside_the_room),
            Some([5.5, 5.5]),
            "an illegal start anchors to the region holding the nearest legal \
             centre, which is its own room"
        );
        assert_eq!(
            nearest_free_body_center(&nav, &[[5.5, 5.5]], None, None, inside_the_room),
            None,
            "and with that region full, there is no answer at all"
        );
    }

    #[test]
    fn exact_preferred_centre_returns_without_full_scan() {
        let nav = sealed_room_and_open_field();
        reset_nearest_free_body_center_cell_visits();
        let got = nearest_free_body_center(&nav, &[], None, None, [5.5, 5.5]);
        let visits = nearest_free_body_center_cell_visits();
        assert_eq!(got, Some([5.5, 5.5]));
        assert_eq!(
            visits, 0,
            "a legal free preferred centre is the unique optimum and must skip the \
             exhaustive scan"
        );
    }

    #[test]
    fn exact_preferred_centre_ignores_self_body_on_fast_path() {
        let nav = sealed_room_and_open_field();
        reset_nearest_free_body_center_cell_visits();
        let got = nearest_free_body_center(&nav, &[[5.5, 5.5]], Some(0), None, [5.5, 5.5]);
        let visits = nearest_free_body_center_cell_visits();
        assert_eq!(got, Some([5.5, 5.5]));
        assert_eq!(
            visits, 0,
            "the ignored index is not an obstacle to itself on the fast path either"
        );
    }

    #[test]
    fn occupied_preferred_falls_back_with_full_scan() {
        let nav = sealed_room_and_open_field();
        let preferred = [28.5f32, 20.5];
        reset_nearest_free_body_center_cell_visits();
        let got = nearest_free_body_center(&nav, &[preferred], None, None, preferred);
        let visits = nearest_free_body_center_cell_visits();
        assert_eq!(
            got,
            Some([28.5, 14.5]),
            "an occupied preferred centre falls back to the exhaustive optimum: \
             three legal centres tie one body diameter away and the lowest flat \
             index (588) wins"
        );
        assert_eq!(visits, (W * H) as u64);
    }

    #[test]
    fn blocked_preferred_centre_does_not_fast_path() {
        let nav = sealed_room_and_open_field();
        assert!(nav.center_blocked()[idx(2, 2)]);
        reset_nearest_free_body_center_cell_visits();
        let got = nearest_free_body_center(&nav, &[], None, None, [2.5, 2.5]);
        let visits = nearest_free_body_center_cell_visits();
        assert_eq!(got, Some([5.5, 5.5]));
        assert_eq!(visits, (W * H) as u64);
    }

    #[test]
    fn excluded_preferred_centre_does_not_fast_path() {
        let nav = sealed_room_and_open_field();
        assert!(!nav.center_blocked()[idx(28, 20)]);
        let preferred = [28.5f32, 20.5];
        let exclude = Some((Cell { x: 28, y: 20 }, 1u32));
        reset_nearest_free_body_center_cell_visits();
        let got = nearest_free_body_center(&nav, &[], None, exclude, preferred);
        let visits = nearest_free_body_center_cell_visits();
        assert_eq!(
            got,
            Some([28.5, 16.5]),
            "the excluded preferred centre falls back to the nearest centre clear \
             of the rectangle, ties going to the lowest flat index (668)"
        );
        assert_eq!(visits, (W * H) as u64);
    }

    #[test]
    fn non_centre_preferred_does_not_fast_path() {
        let nav = sealed_room_and_open_field();
        reset_nearest_free_body_center_cell_visits();
        let got = nearest_free_body_center(&nav, &[], None, None, [5.25, 5.5]);
        let visits = nearest_free_body_center_cell_visits();
        assert_eq!(got, Some([5.5, 5.5]));
        assert_eq!(
            visits,
            (W * H) as u64,
            "a point that is not a cell centre is not the optimum by inspection"
        );
    }

    #[test]
    fn out_of_bounds_preferred_centre_does_not_fast_path() {
        let nav = sealed_room_and_open_field();
        reset_nearest_free_body_center_cell_visits();
        let got = nearest_free_body_center(&nav, &[], None, None, [45.5, 20.5]);
        let visits = nearest_free_body_center_cell_visits();
        assert_eq!(
            got,
            Some([34.5, 20.5]),
            "an exact centre off the grid is bounds-rejected before any cell index \
             and answered by the clamped fallback"
        );
        assert_eq!(visits, (W * H) as u64);
    }

    #[test]
    fn negative_preferred_centre_does_not_fast_path() {
        let nav = sealed_room_and_open_field();
        reset_nearest_free_body_center_cell_visits();
        let got = nearest_free_body_center(&nav, &[], None, None, [-3.5, -2.5]);
        let visits = nearest_free_body_center_cell_visits();
        assert_eq!(
            got,
            Some([5.5, 5.5]),
            "a negative exact centre is rejected before any cast and answered by \
             the clamped fallback"
        );
        assert_eq!(visits, (W * H) as u64);
    }

    #[test]
    fn empty_grid_still_returns_none() {
        let solids = vec![true; (W * H) as usize];
        let nav = StaticNav::from_raw(W, H, solids, RTS_UNIT_BODY_RADIUS_CELLS);
        reset_nearest_free_body_center_cell_visits();
        let got = nearest_free_body_center(&nav, &[], None, None, [5.5, 5.5]);
        let visits = nearest_free_body_center_cell_visits();
        assert_eq!(got, None);
        assert_eq!(
            visits, 0,
            "a None anchor returns before the exhaustive loop"
        );
    }
}
