//! The phase-1 RTS game state: seeded from a scenario, ticked deterministically.

use sha2::{Digest, Sha256};

use crate::nav::field_pool::{FieldPool, FieldPoolError, FieldRef};
use crate::render::{Camera, IsoView, VIEW_HEIGHT, VIEW_WIDTH, screen_axes_to_cells};
use crate::scenario::{self, Cell, Scenario};
use crate::sim::{TICK_DT, dir_from_vector};

use super::build::{
    Placement, PlacementError, STALLED_SITE_TICKS, build_ticks, building_cost, placement_valid,
    supply_grant,
};
use super::collision::{
    GATHER_PAIR_ACTIVE, GATHER_SEPARATION_STEP_CELLS, GATHER_SEPARATION_TICKS,
    GatherCollisionState, concentric_pair_normal, moving_circle_hits_point, pair_byte_exempts,
    pair_byte_is_transition, units_overlap,
};
use super::combat::{building_weapon, surface_distance, weapon};
use super::economy::{
    GATHER_TICKS, Resources, Supply, WORKER_CARRY_CAPACITY, WORKER_SUPPLY_COST, node_amount,
    supply_cost,
};
use super::entity::{
    BuildingKind, EntityId, EntityKind, EntityStore, MAX_ENTITIES, OWNER_ENEMY, OWNER_NEUTRAL,
    OWNER_PLAYER, RTS_UNIT_BODY_DIAMETER_CELLS, RTS_UNIT_BODY_RADIUS_CELLS, ResourceKind, UnitKind,
    armor,
};
use super::formation::{
    FORMATION_ARRIVAL_CELLS, FormationError, FormationGoal, FormationScratch,
    nearest_body_clear_cell,
};
use super::orders::{
    FOLLOW_REPATH_CELLS, GHOUL_SPEED_CELLS_PER_SEC, GatherPhase, Order, OrderTable,
    SOLDIER_SPEED_CELLS_PER_SEC, WORKER_SPEED_CELLS_PER_SEC, adaptive_reach, dist2,
    entity_approach_cell, node_cell, rect_distance, step_admissible, unit_speed,
};
use super::production::{
    ProduceError, ProductionQueue, ProductionTable, RallyTarget, can_produce, unit_cost,
};
use super::selection::{MAX_SELECTION, Pick, Selection, box_select, footprint_min, pick_at};
use super::static_nav::{StaticNav, circle_clear_of_cell_rect};

/// What kind of order a context click resolved a unit into. See
/// [`RtsWorld::issue_context_order_at`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssuedOrder {
    Move,
    Gather,
    Build,
    Attack,
    AttackMove,
    Stop,
    Follow,
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
    /// Attack-move: armed members fight on the way, unarmed members just walk.
    AttackGround(Cell),
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

/// Why a [`RtsWorld::cmd_attack_target`], [`RtsWorld::cmd_attack_move`] or
/// [`RtsWorld::cmd_stop`] was refused whole. Every variant leaves the world
/// untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandRejectReason {
    /// The selection holds no live player-owned unit.
    EmptySelection,
    /// Attack-target needs at least one armed unit in the selection.
    NoArmedUnits,
    /// The attack target is stale, dead, or not enemy-owned.
    NoTarget,
    /// No navigation field could be built to the command's anchor cell.
    Unreachable,
    /// Fewer legal formation slots than the walking members need —
    /// whole-order refusal, the same rule Move has always had.
    NoFormationSpace,
}

/// The outcome of one [`RtsWorld::cmd_attack_target`],
/// [`RtsWorld::cmd_attack_move`] or [`RtsWorld::cmd_stop`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandReceipt {
    /// Orders actually written — one receipt each in the caller's buffer,
    /// ascending by entity slot.
    pub accepted: usize,
    /// Selected orderable units this command left unordered.
    pub rejected: usize,
    /// Set exactly when the whole command was refused (`accepted == 0`).
    pub reason: Option<CommandRejectReason>,
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

/// Maximum number of live ground-order markers tracked at once.
pub const MAX_MOVE_MARKERS: usize = 8;
/// Ticks a ground-order marker lives before it decays away (1.5 s at 60 Hz).
pub const MOVE_MARKER_TICKS: u32 = 90;

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
    /// the shipping build does not compile. The movement system is skipped for
    /// that tick rather than compounding an illegal state, the repair stays
    /// armed so the next tick retries, and the overlap is reported here rather
    /// than silently kept.
    ///
    /// Also the terminal state of a gather exit that ran out of bound — the
    /// one path that reaches this in a shipping build: a pair that neither
    /// separated within [`GATHER_SEPARATION_TICKS`] attempts nor found a free
    /// centre to relocate into is put back to *hard* and reported here, so it
    /// is counted by [`RtsWorld::body_overlap_count`] for as long as it lasts.
    /// An exit that fails is a failure that says so, never a permanent
    /// exemption. In a testkit build it also arms the repair pass, so the next
    /// tick retries it and re-reports for as long as it really is a failure.
    #[error("a merged unit body could not be repaired: no legal free position exists")]
    UnrepairableOverlap,
}

/// What [`RtsWorld::apply_damage`] did to its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageResult {
    /// The hit landed and the target survives with this much HP left.
    Damaged { remaining_hp: u32 },
    /// The hit reduced the target to 0 HP; it died and was despawned
    /// inside this call.
    Killed,
    /// The target is a resource node. Nodes are indestructible; nothing
    /// changed.
    Indestructible,
    /// The id names no live entity; nothing changed.
    NoTarget,
}

/// One entity death, surfaced for render-side feedback (the death flash).
///
/// Not world state: the buffer holding these clears at the start of every
/// tick and never enters [`RtsWorld::state_hash`] — two worlds that agree
/// on their entities agree on their digest whether or not anyone drained
/// the events. `center` is the entity's position at the moment it died.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeathEvent {
    /// What died.
    pub kind: EntityKind,
    /// Who owned it (`OWNER_PLAYER` / `OWNER_ENEMY`).
    pub owner: u8,
    /// Where it stood, in cell space.
    pub center: [f32; 2],
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
    /// Parallel to [`Self::unit_scratch`]: every unit body's position as it
    /// stood when the gather-exit separation pass began, so that pass decides
    /// *which* pairs it acts on from one consistent picture of the tick rather
    /// than from a world its own earlier moves have already changed. Reserved
    /// to [`MAX_ENTITIES`] so a tick never grows it.
    snapshot_pos: Vec<[f32; 2]>,
    /// Parallel to [`Self::unit_scratch`]: has this body already been pushed
    /// aside by a mover this tick? One push per body per tick is what keeps a
    /// crowd from shoving one unit several cells in a single tick. Reserved to
    /// [`MAX_ENTITIES`] so a tick never grows it.
    pushed: Vec<bool>,
    /// One byte per unordered slot pair: which pairs the collision policy
    /// currently exempts from *mutual dynamic* collision. Preallocated at load
    /// (see [`GatherCollisionState`]) so a tick never grows it.
    gather_pairs: GatherCollisionState,
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
    /// Per-slot count of consecutive ticks a site's completion plan was
    /// discarded because it could not evacuate every body its footprint
    /// covers. Reset when the site makes progress, finishes, is cancelled, or
    /// the slot is handed to a new site; frozen (not reset) while the site is
    /// unattended, since an unattended site plans nothing to discard. Reaching
    /// [`STALLED_SITE_TICKS`] cancels the site. Sized to [`MAX_ENTITIES`] so
    /// construction never allocates.
    ///
    /// Deliberately **not** in [`Self::state_hash`]: this is a retry counter,
    /// not a decision. Everything it can ever produce — a despawned site,
    /// refunded resources, a cleared `Order::Build` — is hashed on the tick it
    /// happens, so a divergence cannot hide here, and keeping it out leaves
    /// the hash byte stream (and every golden pinned to it) unmoved.
    site_stall: Vec<u32>,
    /// Sites this tick's construction pass decided to cancel, drained after
    /// the pass so a cancellation never despawns a slot the pass is still
    /// iterating. Reserved to [`MAX_ENTITIES`].
    stalled_cancels: Vec<EntityId>,
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
    /// Index of the first scenario enemy wave not yet fully spawned.
    enemy_wave_cursor: usize,
    /// Ghouls the wave at [`Self::enemy_wave_cursor`] still owes the world —
    /// nonzero exactly while a wave is mid-deferral (store full, or no legal
    /// free centre). Bounded state, not a queue: waves drain strictly in
    /// list order, so one pending count is the whole backlog.
    enemy_wave_pending: u32,
    /// Cumulative Ghouls ever spawned (pre-placed + waves). Never
    /// decremented on death — the combat gate's exit token reads it as a
    /// spawn odometer, not a head-count.
    enemies_spawned: u32,
    /// One production queue and rally point per entity slot.
    production: ProductionTable,
    /// Per-slot "this unit has a live target in weapon range this tick",
    /// written by the combat system and read by the movement arms for
    /// `Attack`/`AttackMove` — a unit that can shoot stands still. Reserved
    /// to [`MAX_ENTITIES`] so combat never allocates.
    combat_hold: Vec<bool>,
    /// Where the enemy faction marches: the approach cell of the current
    /// objective building. `None` when no player building is left.
    enemy_objective: Option<Cell>,
    /// Recompute [`Self::enemy_objective`] before the next enemy-AI pass.
    /// Set at load and by a player building's death — never per tick.
    enemy_objective_dirty: bool,
    /// The starting HQ's centre, kept after its death: the fixed point
    /// "nearest remaining player building" is measured from.
    enemy_objective_origin: [f32; 2],
    /// Enemy entities destroyed by combat fire.
    kills: u32,
    /// Player units and buildings destroyed by combat fire.
    losses: u32,
    /// Tick index of the first combat shot ever applied; `None` while the
    /// run is bloodless.
    first_combat_tick: Option<u32>,
    /// Deaths since the start of the current tick, for the render side to
    /// drain ([`Self::drain_death_events`]). Cleared at the top of every
    /// tick and bounded by [`MAX_ENTITIES`], so an offscreen run that never
    /// drains costs nothing and accumulates nothing.
    death_events: Vec<DeathEvent>,
    /// Ground-order markers: where the player last sent someone, and for how
    /// much longer to say so. Bounded and overwritten oldest-first, so a player
    /// spamming move orders cannot grow this.
    move_markers: [(Cell, u32); MAX_MOVE_MARKERS],
    move_marker_len: usize,
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
const _: () = assert!(
    GHOUL_SPEED_CELLS_PER_SEC < MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC,
    "the ghoul outruns the push chain's endpoint check: raise MAX_PUSH_DEPTH's \
     cost or lower the speed — see MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC"
);

/// How close two body centres have to be before "directly away from the
/// partner" stops being a direction at all.
///
/// Not a collision tolerance — the collision rule itself is exact and has no
/// epsilon. This is only the point below which normalising the separation
/// vector stops producing a usable unit vector, and
/// [`concentric_pair_normal`] takes over.
const CONCENTRIC_CELLS: f32 = 1e-6;

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

        // 1b. Declared pre-built buildings, in scenario order, each at its
        //     footprint centre and each already finished: a fresh spawn's
        //     `progress_target` is 0, which is exactly what "finished" means
        //     everywhere else in this file. They are spawned *before*
        //     `StaticNav::new` for the same reason the HQ is — that
        //     constructor stamps every finished building in the store into
        //     `solids` and `placement_solids`, so seeding here is the same
        //     path a live `finish_site` takes, not a second one.
        let mut prebuilt_supply_grant: u32 = 0;
        for spec in &rts.buildings {
            let kind = BuildingKind::from(spec.kind);
            let edge = kind.footprint_cells() as f32;
            let pos = [
                spec.cell.x as f32 + edge * 0.5,
                spec.cell.y as f32 + edge * 0.5,
            ];
            entities
                .spawn(EntityKind::Building(kind), OWNER_PLAYER, pos)
                .ok_or_else(|| RtsWorldError::StoreFull {
                    what: "pre-built building".to_string(),
                })?;
            prebuilt_supply_grant += supply_grant(kind);
        }

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
        // 3b. Declared start units, before the workers: an authored squad owns
        //     the ground it names and the workers relocate around it, rather
        //     than the other way round. Every body goes through the same
        //     collision-free search, against the same `placed` list, so a
        //     batch of eight spreads out instead of stacking.
        let mut placed: Vec<[f32; 2]> = Vec::with_capacity(scenario.spawn_cells().len());
        let mut start_unit_supply: u32 = 0;
        for spec in &rts.start_units {
            let kind = UnitKind::from(spec.kind);
            for _ in 0..spec.count {
                let preferred = [spec.cell.x as f32 + 0.5, spec.cell.y as f32 + 0.5];
                let pos = nearest_free_body_center(&static_nav, &placed, None, None, preferred)
                    .ok_or(RtsWorldError::NoFreeUnitPosition)?;
                entities
                    .spawn(EntityKind::Unit(kind), OWNER_PLAYER, pos)
                    .ok_or_else(|| RtsWorldError::StoreFull {
                        what: "start unit".to_string(),
                    })?;
                placed.push(pos);
                start_unit_supply += supply_cost(kind);
            }
        }

        let mut worker_count: u32 = 0;
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

        // 5. Pre-placed Ghouls, in scenario order, through the same body-safe
        //    search the workers used, against the bodies already seeded.
        //    Enemies charge no supply. Failure is fatal here, exactly as it is
        //    for a worker: a scene that cannot seed its own script is broken,
        //    and the wave spawner's bounded deferral is a runtime behaviour,
        //    not a construction one.
        let mut enemies_spawned: u32 = 0;
        if let Some(enemies) = rts.enemies.as_ref() {
            for c in &enemies.pre_placed {
                let preferred = [c.x as f32 + 0.5, c.y as f32 + 0.5];
                let pos = nearest_free_body_center(&static_nav, &placed, None, None, preferred)
                    .ok_or(RtsWorldError::NoFreeUnitPosition)?;
                entities
                    .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, pos)
                    .ok_or_else(|| RtsWorldError::StoreFull {
                        what: "ghoul".to_string(),
                    })?;
                placed.push(pos);
                enemies_spawned += 1;
            }
        }

        let resources = Resources {
            crystal: rts.start_crystal,
            gas: rts.start_gas,
        };
        let mut supply = Supply::new(rts.start_supply_cap);
        supply.grant_cap(prebuilt_supply_grant);
        supply.add_used(WORKER_SUPPLY_COST * worker_count + start_unit_supply);

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
            snapshot_pos: Vec::with_capacity(MAX_ENTITIES),
            pushed: Vec::with_capacity(MAX_ENTITIES),
            gather_pairs: GatherCollisionState::new(),
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
            site_stall: vec![0; MAX_ENTITIES],
            stalled_cancels: Vec::with_capacity(MAX_ENTITIES),
            body_scratch: Vec::with_capacity(MAX_ENTITIES),
            evac_units: Vec::with_capacity(MAX_ENTITIES),
            evac_to: Vec::with_capacity(MAX_ENTITIES),
            enemy_wave_cursor: 0,
            enemy_wave_pending: 0,
            enemies_spawned,
            production: ProductionTable::new(),
            combat_hold: vec![false; MAX_ENTITIES],
            enemy_objective: None,
            enemy_objective_dirty: true,
            enemy_objective_origin: hq_pos,
            kills: 0,
            losses: 0,
            first_combat_tick: None,
            death_events: Vec::with_capacity(MAX_ENTITIES),
            move_markers: [(Cell { x: 0, y: 0 }, 0); MAX_MOVE_MARKERS],
            move_marker_len: 0,
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

    /// How many pairs of live units penetrate each other **in violation of the
    /// collision policy** (`T17`, redefined by `T13`).
    ///
    /// The one shared body-safety oracle: the engine acceptance run asserts
    /// it at every milestone, and the app's `rts` exit line reports it as
    /// `body_overlaps`, so a scripted run and a world-call run cannot disagree
    /// about what "hard bodies" means. Exactly [`units_overlap`]'s test —
    /// touching at `r1 + r2` is legal, anything closer is a penetration — over
    /// every unordered pair, buildings and nodes excluded (they are
    /// footprints, not circles).
    ///
    /// What changed with `T13`: an *active gather pair* — two workers both
    /// under [`Order::Gather`], whatever their owners or phases — is exempt
    /// from mutual collision, so its overlap is legal and is not counted. The
    /// token this feeds keeps its name; its meaning is now "policy
    /// violations", not "raw geometric overlaps". The raw count stays
    /// available to tests as [`Self::raw_body_overlap_count`], which is what
    /// keeps a reading of `0` here from being vacuous.
    ///
    /// What `T14` added: a pair that has *stopped* gathering is exempt too,
    /// but only while it is inside its [`GATHER_SEPARATION_TICKS`] bound. That
    /// is a strictly finite licence — [`Self::separate_exiting_pairs`] spends
    /// one attempt of it per tick and cannot renew it — so a pair that never
    /// separates ends up counted here rather than hidden here forever.
    ///
    /// The policy this reads is the one the **most recent completed tick**
    /// recorded ([`Self::mark_active_gather_pairs`]), not one re-derived at
    /// call time: the oracle and the movement gates must agree about which
    /// pairs were exempt while the bodies were being moved, and re-deriving
    /// here would let an order issued between two ticks change the verdict on
    /// a tick that had already run under the old policy.
    ///
    /// `O(n^2)` on purpose: this is an observation seam for tests and the
    /// exit line, never a per-tick path.
    pub fn body_overlap_count(&self) -> u32 {
        self.count_body_overlaps(true)
    }

    /// Every penetrating pair of live units, policy ignored.
    ///
    /// A test hook, and specifically the anti-vacuity twin of
    /// [`Self::body_overlap_count`]: an exemption that produced no geometric
    /// overlap at all would let the policy oracle read `0` without proving
    /// anything.
    #[cfg(feature = "testkit")]
    pub fn raw_body_overlap_count(&self) -> u32 {
        self.count_body_overlaps(false)
    }

    /// The one scan behind both oracles. `policy` selects whether an exempt
    /// pair's overlap counts.
    fn count_body_overlaps(&self, policy: bool) -> u32 {
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
                if policy && pair_byte_exempts(self.gather_pairs.state(a, b)) {
                    continue;
                }
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

    /// Test-only: put any live entity under any order, bypassing the issue
    /// rules.
    ///
    /// The gather-collision policy is deliberately owner-blind and
    /// phase-blind, and phase 1 has no way to hand another owner's worker a
    /// gather order or to park one in a chosen phase — so without this seam
    /// the "any owner, any phase" half of the rule could only be asserted by
    /// reading the code. `false` for a stale id.
    #[cfg(feature = "testkit")]
    pub fn force_order_for_test(&mut self, id: EntityId, order: Order) -> bool {
        let Some(slot) = self.entities.slot(id) else {
            return false;
        };
        self.orders.set(slot, order);
        true
    }

    /// Test-only: the collision-policy byte the most recent completed tick
    /// left on the pair `{a, b}`.
    ///
    /// `0` hard, [`GATHER_PAIR_ACTIVE`] active gather provenance, and
    /// `1..=`[`GATHER_SEPARATION_TICKS`] the number of separation attempts an
    /// exit has already spent. The two overlap oracles can only see the
    /// *effect* of a byte — whether an overlap counts — which cannot tell a
    /// pair one attempt into its bound from a pair eleven attempts in, and the
    /// whole point of `T14` is that those are different. `0` for a stale id.
    #[cfg(feature = "testkit")]
    pub fn gather_pair_state_for_test(&self, a: EntityId, b: EntityId) -> u8 {
        let (Some(sa), Some(sb)) = (self.entities.slot(a), self.entities.slot(b)) else {
            return 0;
        };
        if sa == sb {
            return 0;
        }
        self.gather_pairs.state(sa, sb)
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

    /// Cumulative Ghouls ever spawned (pre-placed + waves). Never
    /// decremented on death.
    pub fn enemies_spawned(&self) -> u32 {
        self.enemies_spawned
    }

    /// Enemy entities destroyed by combat fire. An exit token reads this.
    pub fn kills(&self) -> u32 {
        self.kills
    }

    /// Player units and buildings destroyed by combat fire.
    pub fn losses(&self) -> u32 {
        self.losses
    }

    /// Tick index of the first combat shot, `None` while nothing has fired.
    pub fn first_combat_tick(&self) -> Option<u32> {
        self.first_combat_tick
    }

    /// The starting HQ. `None` only after it is destroyed
    /// ([`Self::apply_damage`]).
    pub fn start_hq(&self) -> Option<EntityId> {
        self.start_hq
    }

    /// Iterator over live ground-order markers: `(cell, ticks_remaining)`.
    pub fn move_markers(&self) -> impl Iterator<Item = (Cell, u32)> + '_ {
        self.move_markers[..self.move_marker_len].iter().copied()
    }

    /// Plant a ground-order marker at `cell`. When the ring buffer is full,
    /// the oldest marker is overwritten — call from the player-command layer
    /// only, not from [`Self::order_move`] (which the rally and production
    /// paths also invoke and must not plant markers).
    pub fn push_move_marker(&mut self, cell: Cell) {
        if self.move_marker_len < MAX_MOVE_MARKERS {
            self.move_markers[self.move_marker_len] = (cell, MOVE_MARKER_TICKS);
            self.move_marker_len += 1;
        } else {
            self.move_markers.copy_within(1.., 0);
            self.move_markers[MAX_MOVE_MARKERS - 1] = (cell, MOVE_MARKER_TICKS);
        }
    }

    /// Decay live markers by one tick, compacting out any that reach zero.
    fn move_marker_decay(&mut self) {
        let mut write = 0;
        for i in 0..self.move_marker_len {
            let (cell, ticks) = self.move_markers[i];
            if ticks > 1 {
                self.move_markers[write] = (cell, ticks - 1);
                write += 1;
            }
        }
        self.move_marker_len = write;
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

    /// Current keyboard and edge pan speeds, cells/second.
    pub fn camera_speeds(&self) -> (f32, f32) {
        (self.keyboard_pan_speed, self.edge_pan_speed)
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
                let enemy = |eid: EntityId| {
                    self.entities
                        .slot(eid)
                        .is_some_and(|s| self.entities.owner(s) == OWNER_ENEMY)
                };
                // An enemy selection is read-only and always exactly one: additive
                // refinement never mixes owners, in either direction.
                if enemy(id) || self.selection.ids().iter().any(|&sel| enemy(sel)) {
                    self.selection.clear();
                    self.selection.insert(id);
                } else {
                    self.selection.toggle(id);
                }
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

    /// Player command: attack-move the current selection to `cell`.
    ///
    /// Armed members take [`Order::AttackMove`], unarmed ones a plain
    /// [`Order::Move`]; lattice, shared anchor field and the whole-or-nothing
    /// `NoFormationSpace` rule are exactly Move's.
    pub fn cmd_attack_move(
        &mut self,
        cell: Cell,
        receipts: &mut OrderReceiptBuffer,
    ) -> CommandReceipt {
        let mut scratch = std::mem::take(&mut self.pick_scratch);
        scratch.clear();
        scratch.extend_from_slice(self.selection.ids());
        let selected = scratch
            .iter()
            .filter(|&&id| self.orderable_slot(id).is_some())
            .count();
        let outcome = self.order_group(&scratch, GroupTarget::AttackGround(cell), Some(receipts));
        self.pick_scratch = scratch;
        match outcome {
            Ok(n) => CommandReceipt {
                accepted: n,
                rejected: selected - n,
                reason: None,
            },
            Err(e) => CommandReceipt {
                accepted: 0,
                rejected: selected,
                reason: Some(match e {
                    FormationError::NoUnits | FormationError::NoTarget => {
                        CommandRejectReason::EmptySelection
                    }
                    FormationError::Unreachable => CommandRejectReason::Unreachable,
                    FormationError::NoFormationSpace => CommandRejectReason::NoFormationSpace,
                }),
            },
        }
    }

    /// Player command: every selected armed player unit chases and attacks
    /// `target`; unarmed selected units walk to a formation slot at the
    /// target's cell instead.
    pub fn cmd_attack_target(
        &mut self,
        target: EntityId,
        receipts: &mut OrderReceiptBuffer,
    ) -> CommandReceipt {
        receipts.clear();
        let mut scratch = std::mem::take(&mut self.pick_scratch);
        scratch.clear();
        scratch.extend_from_slice(self.selection.ids());
        let selected = scratch
            .iter()
            .filter(|&&id| self.orderable_slot(id).is_some())
            .count();
        let reject = |reason| CommandReceipt {
            accepted: 0,
            rejected: selected,
            reason: Some(reason),
        };

        if selected == 0 {
            self.pick_scratch = scratch;
            return reject(CommandRejectReason::EmptySelection);
        }
        let target_ok = self
            .entities
            .slot(target)
            .is_some_and(|s| self.entities.owner(s) == OWNER_ENEMY);
        if !target_ok {
            self.pick_scratch = scratch;
            return reject(CommandRejectReason::NoTarget);
        }

        let mut armed_count = 0usize;
        self.formation.begin();
        for slot in 0..self.entities.slot_count() {
            let Some(id) = self.entities.id_at(slot) else {
                continue;
            };
            if !scratch.contains(&id) || self.orderable_slot(id).is_none() {
                continue;
            }
            if matches!(self.entities.kind(slot), EntityKind::Unit(k) if weapon(k).is_some()) {
                armed_count += 1;
            } else {
                self.formation.push_unit(id);
            }
        }
        if armed_count == 0 {
            self.pick_scratch = scratch;
            return reject(CommandRejectReason::NoArmedUnits);
        }

        let target_slot = self.entities.slot(target).expect("checked live above");
        let pos = self.entities.position(target_slot);
        let target_cell = Cell {
            x: pos[0] as u32,
            y: pos[1] as u32,
        };
        let Some(anchor) = nearest_body_clear_cell(&self.static_nav, target_cell) else {
            self.pick_scratch = scratch;
            return reject(CommandRejectReason::Unreachable);
        };
        let Ok(field) = self.nav.acquire(anchor) else {
            self.pick_scratch = scratch;
            return reject(CommandRejectReason::Unreachable);
        };
        if self.formation.len() > 0
            && let Err(e) = self.formation.plan(
                &self.static_nav,
                &self.entities,
                &self.nav,
                field.slot,
                anchor,
            )
        {
            self.pick_scratch = scratch;
            return reject(match e {
                FormationError::NoFormationSpace => CommandRejectReason::NoFormationSpace,
                _ => CommandRejectReason::Unreachable,
            });
        }

        let mut accepted = 0usize;
        let mut walker = 0usize;
        for slot in 0..self.entities.slot_count() {
            let Some(id) = self.entities.id_at(slot) else {
                continue;
            };
            if !scratch.contains(&id) || self.orderable_slot(id).is_none() {
                continue;
            }
            let armed =
                matches!(self.entities.kind(slot), EntityKind::Unit(k) if weapon(k).is_some());
            let (order, issued) = if armed {
                (Order::Attack { target, field }, IssuedOrder::Attack)
            } else {
                debug_assert_eq!(self.formation.unit(walker), id, "plan order is push order");
                let goal = FormationGoal {
                    anchor,
                    slot: self.formation.slot(walker),
                };
                walker += 1;
                (Order::Move { goal, field }, IssuedOrder::Move)
            };
            self.orders.set(slot, order);
            receipts.push(id, issued);
            accepted += 1;
        }
        self.pick_scratch = scratch;
        CommandReceipt {
            accepted,
            rejected: selected - accepted,
            reason: None,
        }
    }

    /// Player command: every selected player unit stops — [`Order::Idle`],
    /// cancelling gather, build, move and both attack orders in place.
    pub fn cmd_stop(&mut self, receipts: &mut OrderReceiptBuffer) -> CommandReceipt {
        receipts.clear();
        let mut scratch = std::mem::take(&mut self.pick_scratch);
        scratch.clear();
        scratch.extend_from_slice(self.selection.ids());
        let mut accepted = 0usize;
        for slot in 0..self.entities.slot_count() {
            let Some(id) = self.entities.id_at(slot) else {
                continue;
            };
            if !scratch.contains(&id) || self.orderable_slot(id).is_none() {
                continue;
            }
            self.orders.clear(slot);
            receipts.push(id, IssuedOrder::Stop);
            accepted += 1;
        }
        self.pick_scratch = scratch;
        if accepted == 0 {
            return CommandReceipt {
                accepted: 0,
                rejected: 0,
                reason: Some(CommandRejectReason::EmptySelection),
            };
        }
        CommandReceipt {
            accepted,
            rejected: 0,
            reason: None,
        }
    }

    /// Issue follow orders for every orderable selected unit toward `target`.
    ///
    /// Rejects a stale or enemy target, an empty selection, and self-follow.
    /// Returns a [`CommandReceipt`] summarising how many succeeded.
    pub fn cmd_follow(
        &mut self,
        target: EntityId,
        receipts: &mut OrderReceiptBuffer,
    ) -> CommandReceipt {
        receipts.clear();
        let mut scratch = std::mem::take(&mut self.pick_scratch);
        scratch.clear();
        scratch.extend_from_slice(self.selection.ids());
        let selected = scratch
            .iter()
            .filter(|&&id| self.orderable_slot(id).is_some())
            .count();

        if selected == 0 {
            self.pick_scratch = scratch;
            return CommandReceipt {
                accepted: 0,
                rejected: 0,
                reason: Some(CommandRejectReason::EmptySelection),
            };
        }
        let target_ok = self
            .entities
            .slot(target)
            .is_some_and(|s| self.entities.owner(s) != OWNER_ENEMY);
        if !target_ok {
            self.pick_scratch = scratch;
            return CommandReceipt {
                accepted: 0,
                rejected: selected,
                reason: Some(CommandRejectReason::NoTarget),
            };
        }
        let mut accepted = 0usize;
        for &id in &scratch {
            if id == target {
                continue;
            }
            if self.order_follow(id, target) {
                receipts.push(id, IssuedOrder::Follow);
                accepted += 1;
            }
        }
        self.pick_scratch = scratch;
        CommandReceipt {
            accepted,
            rejected: selected - accepted,
            reason: None,
        }
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
                GroupTarget::AttackGround(_) => {
                    let armed = matches!(
                        self.entities.kind(slot),
                        EntityKind::Unit(k) if weapon(k).is_some()
                    );
                    if armed {
                        (Order::AttackMove { goal, field }, IssuedOrder::AttackMove)
                    } else {
                        (Order::Move { goal, field }, IssuedOrder::Move)
                    }
                }
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
            GroupTarget::Ground(cell) | GroupTarget::AttackGround(cell) => {
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
            GroupTarget::Ground(_) | GroupTarget::AttackGround(_) => true,
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

    /// Order one player-owned unit to follow `target`, holding at interaction
    /// reach when it arrives. Returns `false` for a stale or ineligible id,
    /// self-targeting, or an enemy target.
    pub fn order_follow(&mut self, id: EntityId, target: EntityId) -> bool {
        let Some(slot) = self.orderable_slot(id) else {
            return false;
        };
        if id == target {
            return false;
        }
        let Some(t_slot) = self.entities.slot(target) else {
            return false;
        };
        if self.entities.owner(t_slot) == OWNER_ENEMY {
            return false;
        }
        let mover_kind = match self.entities.kind(slot) {
            EntityKind::Unit(k) => k,
            _ => return false,
        };
        let (approach, _dist) =
            entity_approach_cell(&self.static_nav, &self.entities, target, mover_kind);
        let Ok(field) = self.nav.acquire(approach) else {
            return false;
        };
        let goal = FormationGoal::at(approach);
        self.orders.set(
            slot,
            Order::Follow {
                target,
                goal,
                field,
            },
        );
        true
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
        // A recycled slot must not inherit the stall count of whatever stood
        // here before it.
        self.site_stall[site_slot] = 0;

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
        // Plant a marker only for successful ground orders from the player
        // command layer. Rally and production use order_move directly, so
        // neither reaches this site.
        if let (GroupTarget::Ground(cell), Ok(n)) = (target, outcome)
            && n > 0
        {
            self.push_move_marker(cell);
        }
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
    pub fn rally(&self, building: EntityId) -> Option<RallyTarget> {
        let slot = self.entities.slot(building)?;
        if !matches!(self.entities.kind(slot), EntityKind::Building(_)) {
            return None;
        }
        self.production.rally(slot)
    }

    /// Set or clear a rally point. Returns `false` for a stale building id,
    /// a non-building, a stale/enemy entity target, or a cell that is
    /// out-of-bounds or blocked.
    pub fn set_rally(&mut self, building: EntityId, target: Option<RallyTarget>) -> bool {
        let Some(slot) = self.entities.slot(building) else {
            return false;
        };
        if !matches!(self.entities.kind(slot), EntityKind::Building(_)) {
            return false;
        }
        match target {
            None => {}
            Some(RallyTarget::Cell(c)) => {
                let width = self.scenario.width();
                let height = self.scenario.height();
                if c.x >= width || c.y >= height {
                    return false;
                }
                if self.nav.blocked()[(c.x + c.y * width) as usize] {
                    return false;
                }
            }
            Some(RallyTarget::Entity(id)) => {
                let Some(t_slot) = self.entities.slot(id) else {
                    return false;
                };
                if self.entities.owner(t_slot) == OWNER_ENEMY {
                    return false;
                }
            }
        }
        self.production.set_rally(slot, target);
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

    /// Deal one hit to `target`: subtract `max(1, damage - armor(kind))`
    /// from its HP, resolving death inside this same call at 0.
    ///
    /// The one damage seam of the combat slice — turrets, unit attacks and
    /// scripted tests all enter here, so the death rules cannot diverge per
    /// caller:
    ///
    /// - a **unit** despawns; its order row is cleared for slot reuse. The
    ///   selection is pruned by the tick's existing last step, not here.
    /// - a **finished building** is un-stamped from [`StaticNav`], the
    ///   pooled blocked mask is replaced (invalidating every cached field —
    ///   the same all-or-nothing rule stamping obeys), its supply grant is
    ///   revoked, its production queue dies with **no refund**, and workers
    ///   hauling cargo back to it go [`Order::Idle`] in this call.
    /// - a **site** despawns and its attending builders go idle. No refund:
    ///   destruction is not [`Self::cancel_construction`].
    /// - a **resource node** is indestructible: the call is a no-op.
    ///
    /// A caller inside [`Self::tick`] must run before any system that
    /// consumes `live_scratch`, or re-collect it: a slot despawned here
    /// stays in that buffer until the next collect, and the store's
    /// accessors assert liveness. Nothing calls this from inside a tick in
    /// this slice.
    pub fn apply_damage(&mut self, target: EntityId, damage: u32) -> DamageResult {
        let Some(slot) = self.entities.slot(target) else {
            return DamageResult::NoTarget;
        };
        let kind = self.entities.kind(slot);
        if matches!(kind, EntityKind::Node(_)) {
            return DamageResult::Indestructible;
        }
        let dealt = damage.saturating_sub(armor(kind)).max(1);
        let hp = self.entities.hp(slot);
        if dealt < hp {
            self.entities.set_hp(slot, hp - dealt);
            return DamageResult::Damaged {
                remaining_hp: hp - dealt,
            };
        }
        self.apply_death(target, slot, kind);
        DamageResult::Killed
    }

    /// Resolve a death [`Self::apply_damage`] decided. `kind` is `slot`'s
    /// kind and is never a node.
    fn apply_death(&mut self, id: EntityId, slot: usize, kind: EntityKind) {
        // Surface the death before any routing mutates the slot. Bounded: a
        // caller that kills without ever ticking cannot grow the buffer.
        if self.death_events.len() < MAX_ENTITIES {
            self.death_events.push(DeathEvent {
                kind,
                owner: self.entities.owner(slot),
                center: self.entities.position(slot),
            });
        }
        match kind {
            EntityKind::Unit(_) => {
                self.orders.clear(slot);
            }
            EntityKind::Building(b) => {
                // Only a finished building was ever stamped or granted
                // supply; a site was neither.
                if self.entities.progress_target(slot) == 0 {
                    let edge = b.footprint_cells();
                    let min = footprint_min(self.entities.position(slot), edge);
                    self.static_nav.unstamp_finished_building(min, edge);
                    self.static_nav
                        .rebuild_center_blocked(RTS_UNIT_BODY_RADIUS_CELLS);
                    let replaced = self
                        .nav
                        .replace_blocked_mask(self.static_nav.center_blocked());
                    debug_assert!(
                        replaced.is_ok(),
                        "pool and static_nav grids must agree in size"
                    );
                    self.supply.revoke_cap(supply_grant(b));
                }
                self.production.clear(slot);
                self.orders.clear(slot);
                // Orders that named this building die with it, in this same
                // call: a hauler bound for a dead drop-off and a builder
                // attending a dead site go idle now rather than walking at a
                // ghost until their own system notices next tick.
                for s in 0..self.entities.slot_count() {
                    if s == slot || !self.entities.alive(s) {
                        continue;
                    }
                    match self.orders.get(s) {
                        Order::Build { site, .. } if site == id => self.orders.clear(s),
                        Order::Gather {
                            phase: GatherPhase::Returning { drop_off, .. },
                            ..
                        } if drop_off == id => self.orders.clear(s),
                        _ => {}
                    }
                }
                if self.start_hq == Some(id) {
                    self.start_hq = None;
                }
                if self.entities.owner(slot) == OWNER_PLAYER {
                    // The enemy faction may have just lost its objective.
                    self.enemy_objective_dirty = true;
                }
            }
            EntityKind::Node(_) => unreachable!("apply_damage refuses nodes before death"),
        }
        self.entities.despawn(id);
    }

    /// Move every death recorded since the current tick began into `out`
    /// (which is cleared first), emptying the internal buffer — a second
    /// drain in the same tick finds nothing.
    ///
    /// Allocation-free when `out` was reserved to [`MAX_ENTITIES`]; the
    /// events are feedback, not state, and are excluded from
    /// [`Self::state_hash`] by design.
    pub fn drain_death_events(&mut self, out: &mut Vec<DeathEvent>) {
        out.clear();
        out.append(&mut self.death_events);
    }

    /// Advance one fixed 1/60 s step.
    ///
    /// Systems are added by later tickets and each one runs at a fixed point in
    /// this order, so a reordering is a visible diff rather than an accident:
    /// 1. commands, 2. camera, 3. enemy waves, 4. construction, 5. production,
    /// 6. orders, 7. enemy AI, 8. combat, 9. movement, 10. marker decay,
    /// 11. supply recount.
    ///
    /// Today the tick counter, the camera pan (2), the enemy wave spawner
    /// (3), the construction system (4), the production system (5), the
    /// gather system (6), the enemy AI (7), the combat system (8), the
    /// movement system (9), the marker decay (10) and the supply recount (11)
    /// run, followed by pruning the selection of anything that died this tick
    /// — last, so a unit that died on this tick is out of the selection before
    /// anything reads it next tick.
    ///
    /// Enemy AI and combat sit between orders and movement on purpose: an
    /// order issued this tick still fires this tick, and a unit that fires
    /// has already had [`Self::combat_hold`] written when the movement
    /// system decides whether to walk it.
    pub fn tick(&mut self) {
        self.tick_index += 1;
        // Last tick's death events die here: drained or not, feedback never
        // outlives one tick inside the world.
        self.death_events.clear();
        self.entities.collect_live(&mut self.live_scratch);
        self.camera_system();
        self.enemy_wave_system();
        self.construction();
        self.production_system();
        self.gather();
        self.enemy_ai();
        self.combat();
        self.movement();
        self.move_marker_decay();
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

    /// System 3: spawn scheduled enemy waves at their exact tick.
    ///
    /// Waves drain strictly in list order: [`Self::enemy_wave_cursor`] names
    /// the first wave not fully spawned, [`Self::enemy_wave_pending`] how
    /// many of its Ghouls still owe the world a body. Placement is
    /// [`nearest_free_body_center`] around the wave's spawn point — the same
    /// deterministic planner every other body placement uses — with each
    /// spawn's centre added to the obstacle set before the next. A spawn
    /// that cannot be honoured this tick (no legal free centre anywhere, or
    /// the store is full) leaves the remainder pending and is retried next
    /// tick: a scheduled enemy is deferred, never dropped.
    fn enemy_wave_system(&mut self) {
        loop {
            // Copy the current wave and its origin out (`WaveSpec` and
            // `Cell` are `Copy`) so no borrow of the owned scenario
            // outlives the mutations below.
            let (wave, point) = {
                let Some(enemies) = self.scenario.rts().and_then(|r| r.enemies.as_ref()) else {
                    return;
                };
                let Some(&wave) = enemies.waves.get(self.enemy_wave_cursor) else {
                    return;
                };
                // In range by validation: spawn_point < spawn_points.len().
                (wave, enemies.spawn_points[wave.spawn_point as usize])
            };
            if self.tick_index < u64::from(wave.at_tick) {
                return;
            }
            if self.enemy_wave_pending == 0 {
                self.enemy_wave_pending = wave.count;
            }
            let preferred = [point.x as f32 + 0.5, point.y as f32 + 0.5];
            self.collect_unit_bodies_into_scratch();
            while self.enemy_wave_pending > 0 {
                let Some(pos) = nearest_free_body_center(
                    &self.static_nav,
                    &self.body_scratch,
                    None,
                    None,
                    preferred,
                ) else {
                    // No legal free centre on the whole grid: remainder waits.
                    return;
                };
                let Some(id) =
                    self.entities
                        .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, pos)
                else {
                    // Store full: same wait, same reason.
                    return;
                };
                self.body_scratch.push(pos);
                // `live_scratch` was collected at the top of the tick; make
                // this Ghoul a body for every later system this same tick,
                // exactly as the production system does for its unit.
                self.live_scratch.push(id.index as usize);
                self.enemies_spawned += 1;
                self.enemy_wave_pending -= 1;
            }
            // Wave fully spawned; the next wave may share this very tick.
            self.enemy_wave_cursor += 1;
        }
    }

    /// System 4: advance every attended construction site by one tick, finish
    /// sites that reach their target, cancel sites that have been unable to
    /// finish for [`STALLED_SITE_TICKS`] consecutive ticks, and clear the
    /// orders of workers whose site just finished.
    ///
    /// Runs before orders (6) and movement (9), so a site that finishes this
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
                self.site_stall[slot] = 0;
            } else if self.finish_site(slot, b) {
                self.site_stall[slot] = 0;
                self.finished.push(self.entities.id_at(slot).expect("live"));
            } else {
                // A completion that could not evacuate every body it covers is
                // simply not applied: progress stays one tick short of its
                // target, the site stays walkable, and the attempt is repeated
                // next tick — but only for a bounded number of ticks. Past
                // that the obstruction is not traffic, it is geometry, and
                // retrying forever is a silent freeze rather than an outcome.
                self.site_stall[slot] += 1;
                if self.site_stall[slot] >= STALLED_SITE_TICKS {
                    self.site_stall[slot] = 0;
                    self.stalled_cancels
                        .push(self.entities.id_at(slot).expect("live"));
                }
            }
        }
        // Drained here, not inside the pass: cancelling despawns the site and
        // rewrites orders, and the pass above is still walking `live_scratch`.
        if !self.stalled_cancels.is_empty() {
            for i in 0..self.stalled_cancels.len() {
                let id = self.stalled_cancels[i];
                let cancelled = self.cancel_construction(id);
                debug_assert!(cancelled, "a stalled site is unfinished, so cancellable");
            }
            self.stalled_cancels.clear();
            // `live_scratch` was collected at the top of the tick and every
            // later system reads it without an aliveness check. A slot this
            // pass just despawned has to leave it, in place, before system 5
            // asks the store what kind of entity stands there.
            let store = &self.entities;
            self.live_scratch.retain(|&slot| store.alive(slot));
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

    /// Distance from `p` to the outside of whatever `target_slot` occupies:
    /// a unit's **body circle**, a building's or node's footprint rectangle.
    ///
    /// [`Self::approach_done`] can use the footprint alone because everything
    /// it measures against is static, and the centre mask already holds a
    /// mover one body radius clear of it. A follower's target is usually
    /// another unit, whose footprint is one cell but whose body is radius
    /// [`RTS_UNIT_BODY_RADIUS_CELLS`]: two such bodies can never close to
    /// within a one-cell rectangle's reach, so measuring the footprint would
    /// leave a follower pressing into its own target forever.
    fn follow_gap(&self, p: [f32; 2], target_slot: usize) -> f32 {
        let c = self.entities.position(target_slot);
        match self.entities.kind(target_slot) {
            EntityKind::Unit(k) => (dist2(p, c).sqrt() - k.body_radius_cells()).max(0.0),
            other => rect_distance(p, c, other.footprint_cells()),
        }
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
    ///    holds at `build_ticks - 1` and stays walkable. The caller counts
    ///    consecutive discards and cancels the site outright once they reach
    ///    [`STALLED_SITE_TICKS`], so "holds" is bounded, never forever;
    /// 4. only a complete plan is committed — and then, in the same tick, the
    ///    building is marked finished, stamped into the static masks, and its
    ///    supply granted.
    ///
    /// There is deliberately no "finish anyway and shove the leftovers inside
    /// the new walls" relaxation. Dropping the footprint from the destination
    /// search only ever yields centres the stamp is about to make illegal:
    /// post-stamp legality *is* `!center_blocked && clear of the footprint`,
    /// which is exactly what the strict search already tests, so within one
    /// connected region a relaxed search can only return a body position that
    /// penetrates static geometry — forbidden since T4 — or one merged with
    /// another body — forbidden by ADR 021. A bounded cancel-and-refund is
    /// the only escape that keeps both.
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

    /// System 5: advance every finished building's production queue by one
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
                // Dispatches on target kind: Cell → move, Entity(node) → gather,
                // Entity(unit/building) → follow. Ignores the return; a blocked
                // or stale rally is a no-op, the unit stays idle.
                match rally {
                    RallyTarget::Cell(cell) => {
                        self.order_move(id, cell);
                    }
                    RallyTarget::Entity(eid) => {
                        let target_slot = self.entities.slot(eid);
                        match target_slot.map(|s| self.entities.kind(s)) {
                            Some(EntityKind::Node(_)) => {
                                self.order_gather(id, eid);
                            }
                            Some(EntityKind::Unit(_) | EntityKind::Building(_)) => {
                                self.order_follow(id, eid);
                            }
                            None => {}
                        }
                    }
                }
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

    /// System 10: recompute `Supply::used` from scratch — live units plus every
    /// live production queue's reservations — rather than maintaining it
    /// incrementally, so it can never drift.
    fn supply_recount(&mut self) {
        let mut used = 0u32;
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            // Enemies never enter Supply::used — supply is a player economy.
            if self.entities.owner(slot) != OWNER_PLAYER {
                continue;
            }
            if let EntityKind::Unit(k) = self.entities.kind(slot) {
                used += supply_cost(k);
            }
        }
        used += self.reserved_supply();
        self.supply.set_used(used);
    }

    /// System 6: advance every gathering worker's round trip one step.
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

    /// Re-derive the enemy faction's objective from the live world: the
    /// approach cell of the live player building nearest
    /// [`Self::enemy_objective_origin`] (ascending scan with a strict `<`,
    /// so a tie goes to the lower slot), or `None` when no player building
    /// is left.
    ///
    /// The approach cell, not the building's own cell: a finished
    /// building's footprint is blocked in the inflated centre mask, so a
    /// pooled field to its own cell cannot be built — the objective must be
    /// a cell a body can stand on, exactly as a hauler's drop-off leg
    /// targets one.
    fn recompute_enemy_objective(&mut self) {
        self.enemy_objective_dirty = false;
        let mut best: Option<(f32, usize)> = None;
        for slot in 0..self.entities.slot_count() {
            if !self.entities.alive(slot)
                || self.entities.owner(slot) != OWNER_PLAYER
                || !matches!(self.entities.kind(slot), EntityKind::Building(_))
            {
                continue;
            }
            let d = dist2(self.entities.position(slot), self.enemy_objective_origin);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, slot));
            }
        }
        let objective = best.map(|(_, slot)| {
            let id = self.entities.id_at(slot).expect("live building");
            entity_approach_cell(&self.static_nav, &self.entities, id, UnitKind::Ghoul).0
        });
        self.enemy_objective = objective;
    }

    /// System 7: enemy AI. Every idle enemy unit is sent marching at the
    /// faction objective, and one already marching somewhere stale is
    /// re-aimed. Runs immediately before combat so a fresh order can still
    /// fire this tick.
    fn enemy_ai(&mut self) {
        if self.enemy_objective_dirty {
            self.recompute_enemy_objective();
        }
        let Some(obj) = self.enemy_objective else {
            // Nothing left to march on: marchers stop. A forced `Attack`
            // (test seam) keeps its target; combat clears it on death.
            for i in 0..self.live_scratch.len() {
                let slot = self.live_scratch[i];
                if self.entities.alive(slot)
                    && self.entities.owner(slot) == OWNER_ENEMY
                    && matches!(self.orders.get(slot), Order::AttackMove { .. })
                {
                    self.orders.clear(slot);
                }
            }
            return;
        };
        // One pooled field for the whole faction, acquired at most once per
        // tick — hundreds of enemies, one field.
        let mut field: Option<FieldRef> = None;
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            if !self.entities.alive(slot)
                || self.entities.owner(slot) != OWNER_ENEMY
                || !matches!(self.entities.kind(slot), EntityKind::Unit(_))
            {
                continue;
            }
            let stale = match self.orders.get(slot) {
                Order::Idle => true,
                Order::AttackMove { goal, .. } => goal.anchor != obj,
                _ => false,
            };
            if !stale {
                continue;
            }
            let f = match field {
                Some(f) => f,
                None => match self.nav.acquire(obj) {
                    Ok(f) => {
                        field = Some(f);
                        f
                    }
                    // No field to the objective right now: leave the
                    // faction idle and retry next tick, rather than order
                    // half of it.
                    Err(_) => return,
                },
            };
            self.orders.set(
                slot,
                Order::AttackMove {
                    goal: FormationGoal::at(obj),
                    field: f,
                },
            );
        }
    }

    /// The nearest live hostile of `slot`'s owner whose surface is within
    /// `range` cells — units by centre distance minus body radius,
    /// buildings by footprint distance, nodes never.
    ///
    /// Ties are broken by scan order with a strict `<`, so the first
    /// candidate at the winning distance keeps the shot. That order is
    /// `live_scratch`: ascending slot, except that a unit spawned earlier in
    /// this same tick (a wave Ghoul, a produced unit) was appended and is
    /// therefore visited last whatever slot it recycled. Fixed either way —
    /// which is the property that matters, since the buffer is built the same
    /// way on every run, so two replays of one scene break a tie identically.
    fn nearest_hostile_in_range(&self, slot: usize, range: f32) -> Option<EntityId> {
        let p = self.entities.position(slot);
        let own = self.entities.owner(slot);
        let mut best: Option<(f32, usize)> = None;
        for i in 0..self.live_scratch.len() {
            let t = self.live_scratch[i];
            if t == slot || !self.entities.alive(t) {
                continue;
            }
            let owner = self.entities.owner(t);
            if owner == own || owner == OWNER_NEUTRAL {
                continue;
            }
            let Some(d) = surface_distance(&self.entities, p, t) else {
                continue;
            };
            if d <= range && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, t));
            }
        }
        best.map(|(_, t)| self.entities.id_at(t).expect("live target"))
    }

    /// Nearest live enemy unit within `range` of a building firer at `slot`.
    ///
    /// Effective distance = `rect_distance(unit_pos, building_pos, edge) −
    /// unit_body_radius` — range from the building's footprint face to the
    /// unit's hull. Buildings only target enemy *units*; the enemy faction
    /// cannot own buildings (T2's schema spawns only Ghouls), so a building–
    /// building path would be dead code.
    fn building_nearest_hostile(&self, slot: usize, edge: u32, range: f32) -> Option<EntityId> {
        use super::orders::rect_distance;
        let p = self.entities.position(slot);
        let own = self.entities.owner(slot);
        let mut best: Option<(f32, usize)> = None;
        for i in 0..self.live_scratch.len() {
            let t = self.live_scratch[i];
            if t == slot || !self.entities.alive(t) {
                continue;
            }
            let owner = self.entities.owner(t);
            if owner == own || owner == OWNER_NEUTRAL {
                continue;
            }
            let EntityKind::Unit(k) = self.entities.kind(t) else {
                continue;
            };
            let d = rect_distance(self.entities.position(t), p, edge) - k.body_radius_cells();
            if d <= range && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, t));
            }
        }
        best.map(|(_, t)| self.entities.id_at(t).expect("live target"))
    }

    /// Whether `target` is live, hostile to `slot`'s owner and within
    /// `range` of it, by the same surface rule the acquire scan uses.
    fn target_in_range(&self, slot: usize, target: EntityId, range: f32) -> bool {
        let Some(t) = self.entities.slot(target) else {
            return false;
        };
        let owner = self.entities.owner(t);
        if owner == self.entities.owner(slot) || owner == OWNER_NEUTRAL {
            return false;
        }
        surface_distance(&self.entities, self.entities.position(slot), t)
            .is_some_and(|d| d <= range)
    }

    /// System 8: instant-hit fire. For every live armed unit, in
    /// [`Self::live_scratch`] order (see [`Self::nearest_hostile_in_range`]
    /// for exactly what that order is and why it is deterministic): tick the
    /// cooldown down, pick a target under the firing rules, and — at
    /// cooldown 0 — apply the damage and reset the cooldown, so the firing
    /// period is exactly `cooldown_ticks` and a fresh spawn (cooldown 0)
    /// fires the first tick it has a target.
    ///
    /// Firing rules: `Idle` and `AttackMove` auto-acquire the nearest
    /// hostile in range and fire in place; `Attack` fires only at its own
    /// target and walks while out of range; a dead `Attack` target clears
    /// the order to `Idle` (the enemy AI re-marches an enemy next tick) and
    /// the unit defends itself under the `Idle` rule the same tick.
    /// `Move`, `Gather` and `Build` never fire.
    ///
    /// Deaths resolve immediately through `apply_damage`, so a later slot
    /// never shoots a corpse. [`Self::combat_hold`] records who has a live
    /// target in range; the movement system holds those units in place.
    fn combat(&mut self) {
        self.combat_hold.fill(false);
        let mut despawned = false;
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            if !self.entities.alive(slot) {
                continue;
            }
            let w = match self.entities.kind(slot) {
                EntityKind::Unit(kind) => weapon(kind),
                // A finished building may be armed; a site never scans.
                EntityKind::Building(b) if self.entities.progress_target(slot) == 0 => {
                    building_weapon(b)
                }
                _ => None,
            };
            let Some(w) = w else {
                continue;
            };
            let cd = self.entities.cooldown(slot);
            if cd > 0 {
                self.entities.set_cooldown(slot, cd - 1);
            }
            let target = match self.entities.kind(slot) {
                EntityKind::Unit(_) => match self.orders.get(slot) {
                    Order::Idle | Order::AttackMove { .. } => {
                        self.nearest_hostile_in_range(slot, w.range_cells)
                    }
                    Order::Attack { target, .. } => {
                        if self.entities.contains(target) {
                            self.target_in_range(slot, target, w.range_cells)
                                .then_some(target)
                        } else {
                            self.orders.clear(slot);
                            self.nearest_hostile_in_range(slot, w.range_cells)
                        }
                    }
                    Order::Move { .. }
                    | Order::Gather { .. }
                    | Order::Build { .. }
                    | Order::Follow { .. } => continue,
                },
                // Buildings have no orders; scan enemies unconditionally.
                EntityKind::Building(b) => {
                    self.building_nearest_hostile(slot, b.footprint_cells(), w.range_cells)
                }
                EntityKind::Node(_) => continue,
            };
            let Some(target) = target else {
                continue;
            };
            self.combat_hold[slot] = true;
            if self.entities.cooldown(slot) != 0 {
                continue;
            }
            let target_owner = {
                let t = self.entities.slot(target).expect("live target");
                self.entities.owner(t)
            };
            let _ = self.apply_damage(target, w.damage);
            if self.first_combat_tick.is_none() {
                self.first_combat_tick = Some(self.tick_index as u32);
            }
            if !self.entities.contains(target) {
                despawned = true;
                if target_owner == OWNER_ENEMY {
                    self.kills += 1;
                } else if target_owner == OWNER_PLAYER {
                    self.losses += 1;
                }
            }
            self.entities.set_cooldown(slot, w.cooldown_ticks);
        }
        if despawned {
            // [`Self::apply_damage`]'s contract: a slot it despawned stays
            // in `live_scratch`, and the store's accessors assert liveness.
            // The supply recount later this tick reads that buffer without a
            // liveness check, so re-collect rather than leave it holding
            // corpses. Allocation-free: the buffer is reserved to
            // [`MAX_ENTITIES`] at load and `collect_live` only clears and
            // refills it.
            self.entities.collect_live(&mut self.live_scratch);
        }
    }

    /// System 9: walk every unit under a `Move` order, or a `Gather` order
    /// mid-transit (`ToNode` or `Returning`), one step down its field —
    /// **without ever merging two unit bodies**.
    ///
    /// Three phases, in this order:
    ///
    /// 1. collect every live unit's slot and body position
    ///    ([`Self::collect_unit_bodies`]);
    /// 2. record this tick's collision policy over every unit pair
    ///    ([`Self::mark_active_gather_pairs`]) and advance every pair that is
    ///    *leaving* an exemption ([`Self::separate_exiting_pairs`]);
    /// 3. **testkit builds only, and only when a test hook or a spent gather
    ///    exit armed it**: repair any penetration the world was handed that
    ///    the policy does *not* allow ([`Self::repair_body_overlaps`]) — no
    ///    normal path can produce one, only
    ///    [`Self::force_position_for_test`], a raw store mutation and a gather
    ///    exit that ran out of bound can, so the shipping tick skips straight
    ///    from phase 2 to phase 4;
    /// 4. propose and commit one candidate step per unit, sequentially, in an
    ///    order rotated by the tick index.
    ///
    /// **Why sequential, and why that is a proof.** The final phase accepts a
    /// candidate only when its whole swept segment clears every *other*
    /// unit's current body — final position for a unit already processed this
    /// tick, last tick's position for one not yet processed. So by induction
    /// on the traversal: the set of committed bodies starts free of every
    /// *policy-forbidden* overlap (no normal path can hand this system one,
    /// phase 2 ends the exits that could, and phase 3 repairs the ones a test
    /// hook forces), and each accepted candidate is likewise clear of every
    /// member of that set at the moment it joins it, including the ones that
    /// will move later — because they have not moved yet and their own
    /// candidates will in turn be tested against this one. A rejected
    /// candidate simply does not move, which cannot create an
    /// overlap either.
    ///
    /// The one mutation that touches a body other than the mover's is
    /// [`Self::try_push_chain`], and it preserves the same induction: it
    /// commits nothing unless the displaced body's own new position is clear
    /// of the mover's candidate and of every other body (swept, so it cannot
    /// tunnel), which is exactly the property the step above assumes of the
    /// set it joins. Therefore no completed tick leaves two bodies merged
    /// **unless the pair is exempt** — see [`Self::pair_ignores_collision`],
    /// which every one of those gates consults, so the exemption is either
    /// honoured by all of them or by none.
    ///
    /// Exempt means one of exactly two things, and both are bounded: an
    /// *active* gather pair ([`Self::mark_active_gather_pairs`]), which loses
    /// the exemption the tick its order does, and a pair inside its bounded
    /// exit ([`Self::separate_exiting_pairs`]), which loses it after at most
    /// [`GATHER_SEPARATION_TICKS`] attempts whatever happens. Both passes run
    /// before the generic repair on purpose: an exit that finishes early is
    /// hard again *within the same tick*, so the repair sees legal geometry
    /// and normal movement sees a hard pair that cannot re-merge on the very
    /// tick it separated.
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
        // Cleared here rather than inside a phase, so it always describes the
        // last tick and never an older one in *every* build — the shipping
        // build has no repair pass, and a spent gather exit is the only thing
        // left that can set it there.
        self.last_tick_error = None;
        self.collect_unit_bodies();
        self.mark_active_gather_pairs();
        self.separate_exiting_pairs();
        if self.last_tick_error.is_some() {
            // An exit the fallback could not land. Same rule as below: the
            // pair is hard again (its byte is cleared), so it is counted by
            // `body_overlap_count` and reported rather than moved on top of.
            // `relocate_stuck_pair` has already armed the repair in a build
            // that has one, so the next tick retries the pair.
            return;
        }
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
            let id = self.entities.id_at(slot).expect("live unit");
            // Before anything reads this slot's pair bytes: a slot recycled
            // since the table last saw it must not hand its old exemptions to
            // whoever lives there now.
            self.gather_pairs.sync_slot(id);
            self.unit_scratch.push(slot);
            self.candidate_pos.push(self.entities.position(slot));
            self.pushed.push(false);
        }
    }

    /// Phase 2: write this tick's collision policy into the pair table — one
    /// byte per unordered pair of live units.
    ///
    /// Provenance, not a reaction to geometry: **every** active gather pair is
    /// marked, overlapping or not, so the exemption is a fact about what the
    /// pair is doing rather than about where it happens to be standing.
    ///
    /// A pair that is *not* active is deliberately left alone here rather than
    /// cleared. Its byte is what the pair carried out of the last tick, and
    /// that byte is the exit's whole memory: [`Self::separate_exiting_pairs`]
    /// runs immediately after and is the one place that reads it, advances it
    /// and — always within this tick — either clears it or hands the pair to
    /// the fallback. So no pair leaves this phase pair holding a marker it did
    /// not earn this tick.
    fn mark_active_gather_pairs(&mut self) {
        let n = self.unit_scratch.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.pair_is_active_gather(i, j) {
                    self.gather_pairs.set_state(
                        self.unit_scratch[i],
                        self.unit_scratch[j],
                        GATHER_PAIR_ACTIVE,
                    );
                }
            }
        }
    }

    /// Whether the entity in store slot `slot` is a worker currently under a
    /// gather order — any phase, any owner.
    fn is_gathering_worker_slot(&self, slot: usize) -> bool {
        matches!(self.entities.kind(slot), EntityKind::Unit(UnitKind::Worker))
            && matches!(self.orders.get(slot), Order::Gather { .. })
    }

    /// Whether the two [`Self::unit_scratch`] bodies `i` and `j` form an active
    /// gather pair: both workers, both gathering. Eligibility is deliberately
    /// blind to owner and to which of the nine `ToNode`/`Mining`/`Returning`
    /// phase combinations the two are in — a resource cluster is crowded by
    /// whoever is working it.
    fn pair_is_active_gather(&self, i: usize, j: usize) -> bool {
        self.is_gathering_worker_slot(self.unit_scratch[i])
            && self.is_gathering_worker_slot(self.unit_scratch[j])
    }

    /// Whether the pair `(i, j)` is exempt from *mutual dynamic* collision
    /// this tick.
    ///
    /// The single gate every pairwise body check in the movement system asks,
    /// so an exemption cannot be honoured by one check and ignored by the
    /// next — which is how a pair gets marked legal by the sweep and then torn
    /// apart by the repair pass on the following tick. Reads the table
    /// [`Self::mark_active_gather_pairs`] and [`Self::separate_exiting_pairs`]
    /// wrote, never the orders directly, so the policy has exactly one author
    /// per tick.
    ///
    /// Two bytes exempt and they mean different things
    /// ([`pair_byte_exempts`]): active gather provenance, and an exit still
    /// inside its [`GATHER_SEPARATION_TICKS`] bound. Both are mutual, and
    /// neither survives a tick that did not re-earn it.
    ///
    /// Static geometry is not a pair and is never exempt.
    fn pair_ignores_collision(&self, i: usize, j: usize) -> bool {
        pair_byte_exempts(
            self.gather_pairs
                .state(self.unit_scratch[i], self.unit_scratch[j]),
        )
    }

    /// Phase 2b: walk every live pair once and advance the ones that are
    /// *leaving* a gather exemption behind.
    ///
    /// An exemption ends the moment the order that earned it does, but a pair
    /// that has been standing inside itself cannot become legal geometry in
    /// one frame without teleporting — the very thing this pass exists to
    /// avoid. So the exit is spread over at most [`GATHER_SEPARATION_TICKS`]
    /// attempts of at most [`GATHER_SEPARATION_STEP_CELLS`] each, one attempt
    /// per pair per tick, and the pair byte counts the attempts already spent.
    ///
    /// The bound is the whole point, so it is enforced from both ends: the
    /// count only ever rises, and the tick that spends the last attempt also
    /// runs the fallback, so no pair is ever observed between two ticks
    /// holding [`GATHER_SEPARATION_TICKS`]. A pair that neither separates nor
    /// relocates ends the tick at `0` — hard, counted by
    /// [`Self::body_overlap_count`], reported through [`TickError`] and handed
    /// to the next tick's generic repair. There is no state in which a pair
    /// keeps an exemption indefinitely.
    ///
    /// Order-independence: the walk is the same ascending live-slot order the
    /// pair hash frames, always with `i < j`, and each accepted move is
    /// committed immediately — so a later pair collision-tests against the
    /// world as earlier pairs left it, while *which* pairs act at all is read
    /// from [`Self::snapshot_pos`], the picture taken before the walk began.
    /// One pass, never iterated to a fixpoint.
    fn separate_exiting_pairs(&mut self) {
        let n = self.unit_scratch.len();
        if n < 2 {
            return;
        }
        self.snapshot_pos.clear();
        self.snapshot_pos.extend_from_slice(&self.candidate_pos);
        for i in 0..n {
            for j in (i + 1)..n {
                self.step_pair_transition(i, j, n);
            }
        }
    }

    /// One pair's whole transition for this tick.
    ///
    /// `pair_is_active_gather` rather than the byte is what says "active":
    /// [`Self::mark_active_gather_pairs`] has just written
    /// [`GATHER_PAIR_ACTIVE`] for exactly these pairs from exactly this
    /// predicate, and a byte carried out of the last tick is the same value —
    /// so only the predicate can tell an exemption being *earned* from one
    /// being *left*.
    fn step_pair_transition(&mut self, i: usize, j: usize, n: usize) {
        if self.pair_is_active_gather(i, j) {
            return;
        }
        let (a, b) = (self.unit_scratch[i], self.unit_scratch[j]);
        let mut state = self.gather_pairs.state(a, b);
        if state == 0 {
            return;
        }
        if !units_overlap(
            self.snapshot_pos[i],
            self.body_radius(i),
            self.snapshot_pos[j],
            self.body_radius(j),
        ) {
            // The exit is over: whatever the pair was carrying, it is legal
            // geometry now and goes back to being a hard pair this instant.
            self.gather_pairs.set_state(a, b, 0);
            return;
        }
        // `<` rather than `!=`, so the bound is enforced on the whole byte
        // range and not only on the one value the writers below can produce:
        // any count at or past it goes to the fallback, and none can climb
        // through it into a second lap of the counter.
        if state == GATHER_PAIR_ACTIVE || state < GATHER_SEPARATION_TICKS {
            if self.try_separation_attempt(i, j, n) {
                self.gather_pairs.set_state(a, b, 0);
                return;
            }
            state = if state == GATHER_PAIR_ACTIVE {
                1
            } else {
                state + 1
            };
        } else {
            state = GATHER_SEPARATION_TICKS;
        }
        self.gather_pairs.set_state(a, b, state);
        if state == GATHER_SEPARATION_TICKS {
            // The bound is spent. Never a further attempt, and never a tick
            // boundary at this value: the relocation below ends at `0` either
            // way.
            self.relocate_stuck_pair(i, j, n);
        }
    }

    /// Spend one separation attempt on the pair `(i, j)`.
    ///
    /// `true` when the pair is no longer penetrating afterwards — including
    /// the case where an earlier pair in this same pass already pulled them
    /// apart, which costs no move at all.
    ///
    /// Only one of the two bodies moves. The tick rotation picks which one
    /// gets the first proposal, exactly as it does for a walk, so an exit is
    /// not paid for by the same body every tick; the partner is tried only
    /// when the first has nowhere legal to go.
    fn try_separation_attempt(&mut self, i: usize, j: usize, n: usize) -> bool {
        let (first, second) = self.rotated_pair_priority(i, j, n);
        if !self.try_separation_move(first, second, i, j) {
            self.try_separation_move(second, first, i, j);
        }
        !units_overlap(
            self.candidate_pos[i],
            self.body_radius(i),
            self.candidate_pos[j],
            self.body_radius(j),
        )
    }

    /// The pair's two bodies, first-proposal first, by the same tick-rotated
    /// rank [`Self::movement`] walks units in.
    fn rotated_pair_priority(&self, i: usize, j: usize, n: usize) -> (usize, usize) {
        let start = (self.tick_index % n as u64) as usize;
        let rank = |k: usize| (k + n - start) % n;
        if rank(i) <= rank(j) { (i, j) } else { (j, i) }
    }

    /// Move `mover` directly away from `partner` by at most
    /// [`GATHER_SEPARATION_STEP_CELLS`], never past contact.
    ///
    /// `true` when the pair needs no move from this body at all or the move
    /// was committed; `false` when the candidate was refused, which is what
    /// makes trying the partner next safe — nothing has been mutated.
    ///
    /// The step is clamped to the penetration depth, so the best an attempt
    /// can do is put the two bodies exactly in contact. Contact is legal, so
    /// no epsilon is needed to make it legal; an attempt that lands a hair
    /// short simply leaves the pair one more attempt inside its bound.
    fn try_separation_move(
        &mut self,
        mover: usize,
        partner: usize,
        pair_i: usize,
        pair_j: usize,
    ) -> bool {
        let from = self.candidate_pos[mover];
        let q = self.candidate_pos[partner];
        let rm = self.body_radius(mover);
        let sum = rm + self.body_radius(partner);
        let dx = from[0] - q[0];
        let dy = from[1] - q[1];
        let d2 = dx * dx + dy * dy;
        let (dir, depth) = if d2 < CONCENTRIC_CELLS * CONCENTRIC_CELLS {
            // No "away" to move along. The pair's own stable axis instead,
            // signed so the lower slot always goes one way and the higher the
            // other — which is also the direction the general case picks on
            // every attempt after this one.
            let normal =
                concentric_pair_normal(self.unit_scratch[pair_i], self.unit_scratch[pair_j]);
            let sign = if mover == pair_i { -1.0 } else { 1.0 };
            ([normal[0] * sign, normal[1] * sign], sum)
        } else {
            let d = d2.sqrt();
            ([dx / d, dy / d], sum - d)
        };
        if depth <= 0.0 {
            return true;
        }
        let step = depth.min(GATHER_SEPARATION_STEP_CELLS);
        let to = [from[0] + dir[0] * step, from[1] + dir[1] * step];
        if !self.displacement_is_legal_statically(from, to, rm)
            || self.sweep_hits_a_hard_body(mover, from, to)
            || self.crowds_another_transition(mover, partner, to)
        {
            return false;
        }
        self.entities.set_position(self.unit_scratch[mover], to);
        self.candidate_pos[mover] = to;
        true
    }

    /// Whether `mover`'s swept separation step runs into a body the policy
    /// does *not* exempt it from.
    ///
    /// A separation attempt shoves nobody: it is the pair's own business, and
    /// a third body is entitled to stand where it stands. Swept rather than
    /// endpoint-tested for the same reason every other body check is.
    fn sweep_hits_a_hard_body(&self, mover: usize, from: [f32; 2], to: [f32; 2]) -> bool {
        let r = self.body_radius(mover);
        (0..self.candidate_pos.len()).any(|k| {
            k != mover
                && !self.pair_ignores_collision(mover, k)
                && moving_circle_hits_point(from, to, r, self.candidate_pos[k], self.body_radius(k))
        })
    }

    /// Whether the candidate `to` would move `mover` *closer* to a body it is
    /// separating from some other pair.
    ///
    /// A unit can be exiting two gathers at once, and its two escapes can
    /// point at each other. Refusing the step that undoes another pair's
    /// progress is what keeps a cluster's exits monotone instead of letting
    /// two bounds trade the same overlap back and forth until both expire.
    fn crowds_another_transition(&self, mover: usize, partner: usize, to: [f32; 2]) -> bool {
        let from = self.candidate_pos[mover];
        let m = self.unit_scratch[mover];
        (0..self.unit_scratch.len()).any(|k| {
            k != mover
                && k != partner
                && pair_byte_is_transition(self.gather_pairs.state(m, self.unit_scratch[k]))
                && dist2(to, self.candidate_pos[k]) < dist2(from, self.candidate_pos[k])
        })
    }

    /// The end of a spent bound: relocate one of the two bodies to the nearest
    /// free legal centre in its own connected region.
    ///
    /// Tried on the rotated-priority body first, then the partner. The search
    /// is the same [`nearest_free_body_center`] every other relocation in this
    /// world uses, with the mover itself ignored and every other live body —
    /// exempt or not — occupied, so the destination is clear of the whole
    /// world rather than only of the pair.
    ///
    /// Neither body having anywhere to go is a *reported* failure, not a
    /// silent exemption: the pair goes hard, which makes it a violation the
    /// oracle counts for as long as it lasts. It is also the one non-test way
    /// to hand the world a penetration, so it arms
    /// [`Self::repair_body_overlaps`] exactly as the two test hooks do and the
    /// next tick retries the pair. A shipping build compiles no such pass; the
    /// failure is still reported this tick and still counted by
    /// [`Self::body_overlap_count`] afterwards.
    fn relocate_stuck_pair(&mut self, i: usize, j: usize, n: usize) {
        let (first, second) = self.rotated_pair_priority(i, j, n);
        for m in [first, second] {
            let Some(p) = nearest_free_body_center(
                &self.static_nav,
                &self.candidate_pos,
                Some(m),
                None,
                self.candidate_pos[m],
            ) else {
                continue;
            };
            self.entities.set_position(self.unit_scratch[m], p);
            self.candidate_pos[m] = p;
            self.clear_settled_transitions(m);
            return;
        }
        self.gather_pairs
            .set_state(self.unit_scratch[i], self.unit_scratch[j], 0);
        self.last_tick_error = Some(TickError::UnrepairableOverlap);
        #[cfg(feature = "testkit")]
        {
            self.repair_armed = true;
        }
    }

    /// After `m` has been relocated: end every *transition* of its that the
    /// move settled.
    ///
    /// Transitions only. A [`GATHER_PAIR_ACTIVE`] byte on one of `m`'s pairs
    /// is this tick's provenance for a gather `m` is still doing with some
    /// third worker, and every active pair is required to carry one — dropping
    /// it would take mandatory provenance out of the state hash to say nothing
    /// about the collision the pair is still entitled to.
    fn clear_settled_transitions(&mut self, m: usize) {
        let rm = self.body_radius(m);
        let slot_m = self.unit_scratch[m];
        for k in 0..self.unit_scratch.len() {
            if k == m {
                continue;
            }
            let slot_k = self.unit_scratch[k];
            if !pair_byte_is_transition(self.gather_pairs.state(slot_m, slot_k))
                || units_overlap(
                    self.candidate_pos[m],
                    rm,
                    self.candidate_pos[k],
                    self.body_radius(k),
                )
            {
                continue;
            }
            self.gather_pairs.set_state(slot_m, slot_k, 0);
        }
    }

    /// Whether bodies `i` and `j` are merged *and* the policy forbids it.
    fn pair_penetrates(&self, i: usize, j: usize) -> bool {
        !self.pair_ignores_collision(i, j)
            && units_overlap(
                self.candidate_pos[i],
                self.body_radius(i),
                self.candidate_pos[j],
                self.body_radius(j),
            )
    }

    /// Phase 3, **testkit only**: move any unit that starts the tick merged
    /// into another body it may not be merged with to the nearest legal free
    /// cell centre, in the same rotated order the step phase walks.
    ///
    /// Only an invalid state reaches this, and no normal tick can build one:
    /// every in-game path that places a unit (seeding, movement, production,
    /// the push off a finished building) already respects bodies, so the
    /// shipping tick compiles this pass out entirely rather than scanning
    /// every pair of live bodies for a penetration it would not find. Exactly
    /// three things arm it — [`Self::force_position_for_test`],
    /// [`Self::entities_mut`] and [`Self::relocate_stuck_pair`] failing a
    /// gather exit's spent bound — and nothing else does. When a penetration
    /// *is* found, the first penetrating unit in the rotated order is the one
    /// relocated — its partner is then no longer penetrating and is left
    /// alone, so a pair costs one relocation, not two.
    ///
    /// A unit with nowhere legal to go stashes [`TickError::UnrepairableOverlap`]
    /// rather than letting the tick complete with a merged pair unreported.
    /// The caller then leaves [`Self::repair_armed`] set, so the pass retries
    /// on the next tick.
    ///
    /// This runs *after* [`Self::separate_exiting_pairs`], so a pair still
    /// inside its exit bound is not torn apart here — and one that has just
    /// spent the last of it is, because that pass has already cleared its byte
    /// back to hard.
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

    /// Whether unit `i`'s current body penetrates any other unit's in a way
    /// the collision policy forbids.
    fn body_penetrates_any(&self, i: usize) -> bool {
        (0..self.candidate_pos.len()).any(|j| j != i && self.pair_penetrates(i, j))
    }

    /// What the swept segment `from -> to` of unit `i`'s body runs into.
    ///
    /// Unit `i` is skipped against itself, and against any body the policy
    /// exempts it from ([`Self::pair_ignores_collision`]); every other unit
    /// counts, whatever its owner, kind or order — an idle, mining or
    /// site-attending unit is a body exactly like a walking one.
    fn body_sweep_hit(&self, i: usize, from: [f32; 2], to: [f32; 2]) -> BodySweep {
        let r = self.body_radius(i);
        let mut hit = [0usize; MAX_PUSHED_BODIES];
        let mut count = 0usize;
        for (j, &q) in self.candidate_pos.iter().enumerate() {
            if j == i
                || self.pair_ignores_collision(i, j)
                || !moving_circle_hits_point(from, to, r, q, self.body_radius(j))
            {
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
    ///   candidate whole and nothing moves at all. Each of those three
    ///   pairwise tests goes through [`Self::pair_ignores_collision`], so an
    ///   exempt pair neither drags a nested push it does not need nor gets the
    ///   whole chain falsely rejected — while static legality, which is not a
    ///   pair, still holds for every link.
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
            // The mover is never displaced by its own push — unless the two
            // are an exempt pair, which may end the tick merged.
            if !self.pair_ignores_collision(i, d.body) && units_overlap(candidate, ri, d.to, r) {
                return false;
            }
            for k in 0..self.candidate_pos.len() {
                if k == i
                    || k == d.body
                    || self.pair_ignores_collision(d.body, k)
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
                if !self.pair_ignores_collision(moved[a].body, moved[b].body)
                    && units_overlap(
                        moved[a].to,
                        self.body_radius(moved[a].body),
                        moved[b].to,
                        self.body_radius(moved[b].body),
                    )
                {
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
            Order::Attack { target, field } => {
                let Some(t_slot) = self.entities.slot(target) else {
                    // Only armed units get their stale Attack cleared by the
                    // combat system; an unarmed one under the test seam stops here.
                    self.orders.clear(slot);
                    return;
                };
                if self.combat_hold[slot] {
                    // In range: combat is firing, the walk pauses.
                    return;
                }
                let cell = match self.entities.kind(t_slot) {
                    EntityKind::Unit(_) => node_cell(self.entities.position(t_slot)),
                    EntityKind::Building(_) => {
                        entity_approach_cell(&self.static_nav, &self.entities, target, kind).0
                    }
                    // A node is never a combat target.
                    EntityKind::Node(_) => {
                        self.orders.clear(slot);
                        return;
                    }
                };
                (FormationGoal::at(cell), field)
            }
            Order::AttackMove { goal, field } => {
                if self.combat_hold[slot] {
                    // In range: hold and let combat fire; descent resumes when
                    // the target dies.
                    return;
                }
                (goal, field)
            }
            Order::Follow {
                target,
                goal,
                field,
            } => {
                let Some(t_slot) = self.entities.slot(target) else {
                    // A dead or despawned target ends the order on the tick it
                    // is noticed — the same discipline a mined-out node gets.
                    self.orders.clear(slot);
                    return;
                };
                // Re-path only once the target has walked more than
                // FOLLOW_REPATH_CELLS from the goal this follower last pathed
                // to. Between those moments the cached field is descended as
                // it stands: a follower never rebuilds a field per tick.
                let drift = dist2(self.entities.position(t_slot), goal.anchor_center()).sqrt();
                let (goal, field) = if drift > FOLLOW_REPATH_CELLS {
                    let (approach, _) =
                        entity_approach_cell(&self.static_nav, &self.entities, target, kind);
                    match self.nav.acquire(approach) {
                        Ok(fresh) => {
                            let fresh_goal = FormationGoal::at(approach);
                            self.orders.set(
                                slot,
                                Order::Follow {
                                    target,
                                    goal: fresh_goal,
                                    field: fresh,
                                },
                            );
                            (fresh_goal, fresh)
                        }
                        // No field can be built to the target any more. Stop,
                        // rather than keep an order alive nothing can finish.
                        Err(_) => {
                            self.orders.clear(slot);
                            return;
                        }
                    }
                } else {
                    (goal, field)
                };
                // Hold at arm's length, measured to the target's shape rather
                // than its cell — see `follow_gap`. The order is not cleared:
                // a follower parks and resumes when its target walks off.
                let anchor_gap = self.follow_gap(goal.anchor_center(), t_slot);
                if self.follow_gap(self.entities.position(slot), t_slot)
                    <= adaptive_reach(kind, anchor_gap)
                {
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
    /// progress_target, amount, carry kind, carry amount, hp), then every live
    /// slot's order, then the gather-collision pair table
    /// ([`GatherCollisionState::hash_into`] — self-framing, so it pins which
    /// unit each pair byte belongs to), then the selection, production queues
    /// and rally points, then the camera centre, then the pending placement
    /// ghost (one tag byte plus the kind byte), then resources and supply,
    /// then the combat counters (kills, losses, first-combat tick as a tag
    /// byte plus fixed-width payload). `f32` goes in as raw bits, matching
    /// `Simulation::state_hash`. The per-slot cooldown column needs no
    /// mention here — [`EntityStore::hash_into`] already carries it.
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
        self.gather_pairs.hash_into(&mut h, &self.entities);
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
        h.update(self.kills.to_le_bytes());
        h.update(self.losses.to_le_bytes());
        match self.first_combat_tick {
            None => {
                h.update([0u8]);
                h.update(0u32.to_le_bytes());
            }
            Some(t) => {
                h.update([1u8]);
                h.update(t.to_le_bytes());
            }
        }
        h.update((self.move_marker_len as u32).to_le_bytes());
        for i in 0..self.move_marker_len {
            let (cell, ticks) = self.move_markers[i];
            h.update(cell.x.to_le_bytes());
            h.update(cell.y.to_le_bytes());
            h.update(ticks.to_le_bytes());
        }
        h.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rts::entity::RTS_UNIT_BODY_RADIUS_CELLS;
    use crate::scenario::{RtsSpec, ScenarioSpec};

    const W: u32 = 40;
    const H: u32 = 40;

    /// A minimal valid RTS scene with two worker spawn cells far enough apart
    /// that the seed keeps both where they are.
    fn two_worker_scene() -> RtsWorld {
        let spec = ScenarioSpec {
            version: "rts_prototype_v1".to_string(),
            width: 64,
            height: 64,
            cell_size_px: 4,
            sprite_size_px: 48,
            hard_agent_count: 0,
            stretch_agent_count: 0,
            seed: 1,
            destination: Cell { x: 0, y: 0 },
            spawn_cells: vec![Cell { x: 6, y: 50 }, Cell { x: 30, y: 50 }],
            atlas_count: 4,
            direction_count: 8,
            frame_count: 4,
            collision_radius_q8: 0,
            separation_strength_q8: 0,
            separation_phases: 1,
            mass_class_count: 1,
            separation_threads: 1,
            obstacle_cells: vec![],
            rts: Some(RtsSpec {
                start_crystal: 300,
                start_gas: 100,
                start_supply_cap: 10,
                hq_cell: Cell { x: 40, y: 40 },
                crystal_nodes: vec![Cell { x: 1, y: 62 }],
                gas_nodes: vec![Cell { x: 1, y: 61 }],
                enemies: None,
                buildings: vec![],
                start_units: vec![],
            }),
        };
        RtsWorld::from_scenario(scenario::Scenario::from_spec(spec).expect("valid spec"))
            .expect("rts world")
    }

    fn live_slots_of<F: Fn(EntityKind) -> bool>(world: &RtsWorld, want: F) -> Vec<usize> {
        (0..world.entities.slot_count())
            .filter(|&s| world.entities.alive(s) && want(world.entities.kind(s)))
            .collect()
    }

    fn crystal_node(world: &RtsWorld) -> EntityId {
        let slot = live_slots_of(world, |k| {
            matches!(k, EntityKind::Node(ResourceKind::Crystal))
        })[0];
        world.entities.id_at(slot).expect("live node")
    }

    /// Park every worker under a gather order that stands still.
    fn set_all_mining(world: &mut RtsWorld, slots: &[usize]) {
        let node = crystal_node(world);
        for &slot in slots {
            world.orders.set(
                slot,
                Order::Gather {
                    node,
                    // A phase that stands still, so a pair cannot drift into
                    // an overlap and make a mark look like a reaction to one.
                    phase: GatherPhase::Mining { ticks_left: 10_000 },
                },
            );
        }
    }

    /// The exemption is provenance, not a reaction to geometry: a pair that is
    /// gathering is marked whether or not it is anywhere near overlapping.
    ///
    /// Asserted on the table rather than on behaviour, because behaviour is
    /// exactly what cannot distinguish "marked" from "never had to be marked"
    /// while the two bodies are 24 cells apart.
    #[test]
    fn active_pair_provenance_is_marked_before_overlap() {
        let mut world = two_worker_scene();
        let workers = live_slots_of(&world, |k| k == EntityKind::Unit(UnitKind::Worker));
        assert_eq!(workers.len(), 2);
        set_all_mining(&mut world, &workers);
        world.tick();

        let (a, b) = (workers[0], workers[1]);
        assert!(
            !units_overlap(
                world.entities.position(a),
                RTS_UNIT_BODY_RADIUS_CELLS,
                world.entities.position(b),
                RTS_UNIT_BODY_RADIUS_CELLS,
            ),
            "the two bodies must be nowhere near each other for this to prove \
             anything"
        );
        assert_eq!(
            world.gather_pairs.state(a, b),
            GATHER_PAIR_ACTIVE,
            "an active gather pair must be marked regardless of overlap"
        );
        assert_eq!(world.body_overlap_count(), 0);
    }

    /// ...and the marker is dropped the moment the pair stops qualifying, so
    /// the exemption cannot outlive the orders that earned it.
    #[test]
    fn a_pair_that_stops_gathering_loses_its_marker() {
        let mut world = two_worker_scene();
        let workers = live_slots_of(&world, |k| k == EntityKind::Unit(UnitKind::Worker));
        set_all_mining(&mut world, &workers);
        world.tick();
        assert_eq!(
            world.gather_pairs.state(workers[0], workers[1]),
            GATHER_PAIR_ACTIVE
        );

        world.orders.clear(workers[1]);
        world.tick();

        assert_eq!(
            world.gather_pairs.state(workers[0], workers[1]),
            0,
            "the marker must be cleared as soon as the pair stops qualifying"
        );
    }

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
