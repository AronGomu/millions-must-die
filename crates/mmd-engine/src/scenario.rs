//! Versioned scenario contract: parse, hash-verify, validate geometry.

use std::fs;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Locked phase-0 scenario version id.
pub const TECHNICAL_PROTOTYPE_V1: &str = "technical_prototype_v1";

/// Version-id prefix reserved for small test fixtures (T29 harness).
///
/// A fixture is a real, fully validated scenario — it obeys every structural
/// rule the gate scene obeys (nonzero seed, non-empty spawns, in-bounds unique
/// obstacles, free destination, free and reachable spawns) and the same
/// tracked-hash contract via [`Scenario::load_verified`]. What it does *not*
/// inherit are the values frozen for the phase-0 workload specifically: the
/// 480×270 grid, the locked agent counts, and the exact-20% obstacle ratio.
/// Those stay locked for [`TECHNICAL_PROTOTYPE_V1`] alone.
pub const FIXTURE_VERSION_PREFIX: &str = "fixture_";

/// Version id for the collision demo family: the gate scene's screen geometry
/// and destination, with a free population and a free body radius.
///
/// It exists because [`TECHNICAL_PROTOTYPE_V1`] freezes its workload and the
/// exact-20% obstacle ratio, and the `fixture_` caps (65 536 cells) cannot
/// express a full-screen scene.
pub const COLLISION_SCENE_V1: &str = "collision_scene_v1";

/// The absolute simultaneous-entity ceiling of this engine.
///
/// Nothing above this count is run, tested or benchmarked. It is not a
/// per-family tuning knob: every scenario family is checked against it in one
/// place ([`check_population`]), so a family added later inherits the ceiling
/// instead of having to remember it. The game reads as a horde through body
/// scale and density, not through population.
pub const MAX_LIVE_AGENTS: u32 = 5_000;

/// Fixture grids must stay small enough that a full system test is cheap.
pub const FIXTURE_MAX_CELLS: u32 = 65_536;
/// Fixture agent counts must stay in the tens/hundreds, not the tens of thousands.
pub const FIXTURE_MAX_AGENTS: u32 = 4_096;

/// Q8 fixed-point scale for collision data: 256 units = one cell, or the
/// scalar value 1.0. Integer because [`Scenario`] derives `Eq`; exact in `f32`
/// because the divisor is a power of two.
pub const COLLISION_Q8: u32 = 256;
/// Largest body radius a scenario may declare: 8 cells (32 px at 4 px/cell).
pub const MAX_COLLISION_RADIUS_Q8: u32 = 2_048;
/// Largest separation weight a scenario may declare: 10.0.
pub const MAX_SEPARATION_STRENGTH_Q8: u32 = 2_560;
/// Largest number of ticks the separation pass may be spread over.
/// `1` is the identity: every agent, every tick.
pub const MAX_SEPARATION_PHASES: u32 = 16;
/// Largest number of distinct push-priority classes. `1` is the identity:
/// every agent pushes and is pushed equally.
pub const MAX_MASS_CLASSES: u32 = 8;
/// Largest worker-thread count for the separation pass. `1` is the
/// identity: the pass runs inline on the calling thread, no pool exists.
pub const MAX_SEPARATION_THREADS: u32 = 16;

const V1_WIDTH: u32 = 480;
const V1_HEIGHT: u32 = 270;
const V1_CELL_PX: u32 = 4;
const V1_SPRITE_PX: u32 = 48;
const V1_HARD_AGENTS: u32 = 5_000;
const V1_STRETCH_AGENTS: u32 = 5_000;
const V1_ATLASES: u32 = 4;
const V1_DIRS: u32 = 8;
const V1_FRAMES: u32 = 4;
/// Exactly half [`V1_SPRITE_PX`]: 6 cells of 4 px, so two touching bodies are
/// one full sprite width apart and their art meets edge to edge.
const V1_COLLISION_RADIUS_Q8: u32 = 1_536;
const V1_SEPARATION_STRENGTH_Q8: u32 = 256;
const V1_SEPARATION_PHASES: u32 = 1;
const V1_MASS_CLASSES: u32 = 1;
const V1_SEPARATION_THREADS: u32 = 1;
const V1_DEST_X: u32 = 240;
const V1_DEST_Y: u32 = 135;

/// The phase-1 RTS prototype scene family: a horde-free base-building map.
pub const RTS_PROTOTYPE_V1: &str = "rts_prototype_v1";

/// Locked non-dimension geometry for [`RTS_PROTOTYPE_V1`]. Width/height are
/// bounded, not exact-locked; see [`RTS_MAX_MAP_EDGE`].
const RTS_CELL_PX: u32 = 4;
const RTS_SPRITE_PX: u32 = 48;

/// Largest width or height an RTS-family map may declare, in cells.
pub const RTS_MAX_MAP_EDGE: u32 = 512;
/// Largest cell count (`width * height`) an RTS-family map may declare.
pub const RTS_MAX_MAP_CELLS: u32 = 262_144;

/// Edge of one *visible build square*, in true coordinate cells.
///
/// Units move on the cell grid as floats and ignore this entirely; it
/// exists so buildings line up with each other and with what the player
/// is shown. Every building footprint is a whole number of squares, and
/// every building's min corner is a multiple of this.
pub const BUILD_SQUARE_CELLS: u32 = 8;

/// Footprint edge of the HQ, in cells — 3 × 3 squares. The validator needs it
/// to prove the HQ site is buildable; the build system reuses the same
/// constant.
pub const HQ_FOOTPRINT_CELLS: u32 = 24;
/// Footprint edge of the Depot, in cells — 1 × 1 square.
pub const DEPOT_FOOTPRINT_CELLS: u32 = 8;
/// Footprint edge of the Barracks, in cells — 2 × 2 squares.
pub const BARRACKS_FOOTPRINT_CELLS: u32 = 16;
/// Footprint edge of the Turret, in cells — 1 × 1 square.
pub const TURRET_FOOTPRINT_CELLS: u32 = 8;

/// Largest starting stock a scene may grant, per resource. Generous, but not
/// "the whole slice is already paid for".
pub const MAX_START_RESOURCE: u32 = 2_000;
/// Absolute supply ceiling — the 500-population design pillar.
pub const MAX_SUPPLY_CAP: u32 = 500;
/// Most resource nodes a scene may declare, per kind.
pub const MAX_RESOURCE_NODES: usize = 64;
/// Most enemies a scene may script in total: pre-placed plus every wave.
///
/// Chosen under the 2 048-entity store with room left for the 500-supply
/// player army, its buildings and the scene's nodes; the wave spawner's
/// bounded deferral absorbs a store that is momentarily fuller.
pub const MAX_ENEMIES: u32 = 1_200;

/// Grid cell coordinate (cell space, not pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Cell {
    pub x: u32,
    pub y: u32,
}

/// The RTS block of a scenario. Present exactly on [`RTS_PROTOTYPE_V1`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RtsSpec {
    /// Starting Crystal stock.
    pub start_crystal: u32,
    /// Starting Gas stock.
    pub start_gas: u32,
    /// Starting supply ceiling, before any Depot is built.
    pub start_supply_cap: u32,
    /// **Minimum corner** (smallest x, smallest y) of the starting HQ's
    /// `HQ_FOOTPRINT_CELLS` x `HQ_FOOTPRINT_CELLS` footprint. Not its centre:
    /// a footprint anchored on a centre has no integer answer for an even edge,
    /// and the build system stamps obstacles from the min corner.
    pub hq_cell: Cell,
    /// Crystal node cells. At least one.
    pub crystal_nodes: Vec<Cell>,
    /// Gas node cells. At least one.
    pub gas_nodes: Vec<Cell>,
    /// Scripted enemy content. Absent on every scene shipped before combat.
    ///
    /// `#[serde(default)]` is load-bearing: it is what keeps every tracked
    /// RTS `.ron` byte-identical, and therefore its `.sha256` sidecar valid,
    /// across this change.
    #[serde(default)]
    pub enemies: Option<EnemySpec>,
}

/// Scripted enemy content of an RTS scene: pre-placed Ghouls, wave
/// origins and a finite timed wave list. Validated by
/// [`validate_rts_block`]; capped by [`MAX_ENEMIES`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EnemySpec {
    /// Ghoul positions seeded at world construction (tick 0).
    pub pre_placed: Vec<Cell>,
    /// Wave origins, indexed by [`WaveSpec::spawn_point`].
    pub spawn_points: Vec<Cell>,
    /// Timed waves, sorted non-decreasing by `at_tick`.
    pub waves: Vec<WaveSpec>,
}

/// One timed enemy wave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct WaveSpec {
    /// Tick the wave fires on, compared against the post-increment tick
    /// counter: the first tick after construction is tick 1.
    pub at_tick: u32,
    /// Ghouls in the wave. At least 1.
    pub count: u32,
    /// Index into [`EnemySpec::spawn_points`].
    pub spawn_point: u8,
}

/// Immutable validated scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    version: String,
    width: u32,
    height: u32,
    cell_size_px: u32,
    sprite_size_px: u32,
    hard_agent_count: u32,
    stretch_agent_count: u32,
    seed: u64,
    destination: Cell,
    spawn_cells: Vec<Cell>,
    atlas_count: u32,
    direction_count: u32,
    frame_count: u32,
    /// Body radius, in 1/256 cell.
    collision_radius_q8: u32,
    /// Separation weight, in 1/256 (256 = 1.0).
    separation_strength_q8: u32,
    /// Ticks the separation pass is spread over. 1 = every agent, every tick.
    separation_phases: u32,
    /// Distinct push-priority classes. 1 = every agent equal.
    mass_class_count: u32,
    /// Worker threads for the separation pass. 1 = inline, no pool.
    separation_threads: u32,
    /// Sorted unique obstacle cell indices (`x + y * width`).
    obstacle_cells: Vec<u32>,
    /// The RTS block, absent on every phase-0 family.
    ///
    /// `#[serde(default)]` is load-bearing: it is what lets every tracked phase-0
    /// `.ron` keep its exact bytes, and therefore its `.sha256` sidecar, across
    /// this change. A required field would invalidate five committed scenes and
    /// four fixtures at once.
    rts: Option<RtsSpec>,
}

/// Unvalidated scenario description — the wire/in-memory form of a scene.
///
/// This is what a `.ron` file deserializes into, and what
/// [`Scenario::from_spec`] validates. Tests that need a synthetic grid build a
/// spec directly instead of formatting RON, so in-memory and on-disk scenarios
/// pass through exactly one validator.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ScenarioSpec {
    pub version: String,
    pub width: u32,
    pub height: u32,
    pub cell_size_px: u32,
    pub sprite_size_px: u32,
    pub hard_agent_count: u32,
    pub stretch_agent_count: u32,
    pub seed: u64,
    pub destination: Cell,
    pub spawn_cells: Vec<Cell>,
    pub atlas_count: u32,
    pub direction_count: u32,
    pub frame_count: u32,
    /// Body radius, in 1/256 cell.
    pub collision_radius_q8: u32,
    /// Separation weight, in 1/256 (256 = 1.0).
    pub separation_strength_q8: u32,
    /// Ticks the separation pass is spread over. 1 = every agent, every tick.
    pub separation_phases: u32,
    /// Distinct push-priority classes. 1 = every agent equal.
    pub mass_class_count: u32,
    /// Worker threads for the separation pass. 1 = inline, no pool.
    pub separation_threads: u32,
    pub obstacle_cells: Vec<u32>,
    /// The RTS block, absent on every phase-0 family.
    ///
    /// `#[serde(default)]` is load-bearing: it is what lets every tracked phase-0
    /// `.ron` keep its exact bytes, and therefore its `.sha256` sidecar, across
    /// this change. A required field would invalidate five committed scenes and
    /// four fixtures at once.
    #[serde(default)]
    pub rts: Option<RtsSpec>,
}

/// Scenario load / validation failures.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ScenarioError {
    #[error("io error: {0}")]
    Io(String),
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("parse error: {0}")]
    Parse(String),
    #[error("unsupported scenario version: {0}")]
    UnsupportedVersion(String),
    #[error("invalid dimension: {0}")]
    InvalidDimension(String),
    #[error("invalid obstacle ratio: obstacles={obstacles}, cells={cells}")]
    InvalidObstacleRatio { obstacles: u32, cells: u32 },
    #[error("invalid obstacle cell index: {0}")]
    InvalidObstacleIndex(u32),
    #[error("duplicate obstacle cell index: {0}")]
    DuplicateObstacle(u32),
    #[error("destination is blocked or out of bounds")]
    DestinationBlocked,
    #[error("spawn cell invalid or blocked: ({x}, {y})")]
    InvalidSpawn { x: u32, y: u32 },
    #[error("unreachable spawn: ({x}, {y})")]
    UnreachableSpawn { x: u32, y: u32 },
    #[error("empty spawn list")]
    EmptySpawns,
    #[error("seed must be nonzero")]
    InvalidSeed,
    #[error("invalid collision config: {0}")]
    InvalidCollision(String),
    #[error("invalid rts block: {0}")]
    InvalidRts(String),
}

impl Scenario {
    /// Load scenario from path; verify sidecar `.sha256` then validate.
    pub fn load_verified(path: impl AsRef<Path>) -> Result<Self, ScenarioError> {
        let path = path.as_ref();
        // Both reads name their own path: a load touches two files, and
        // "No such file or directory" without one is not actionable — the
        // caller cannot tell a missing scenario from a missing sidecar.
        let bytes =
            fs::read(path).map_err(|e| ScenarioError::Io(format!("{}: {e}", path.display())))?;
        let sha_path = path.with_extension("sha256");
        let expected = fs::read_to_string(&sha_path)
            .map_err(|e| ScenarioError::Io(format!("{}: {e}", sha_path.display())))?;
        Self::from_verified_bytes(&bytes, expected.trim())
    }

    /// Verify SHA-256 hex of `bytes`, parse RON, validate contract.
    pub fn from_verified_bytes(bytes: &[u8], expected_hex: &str) -> Result<Self, ScenarioError> {
        let actual = sha256_hex(bytes);
        if actual != expected_hex {
            return Err(ScenarioError::HashMismatch {
                expected: expected_hex.to_string(),
                actual,
            });
        }
        Self::parse_and_validate(bytes)
    }

    /// Parse + validate without hash (tests / trusted in-memory docs).
    pub fn parse_and_validate(bytes: &[u8]) -> Result<Self, ScenarioError> {
        let doc: ScenarioSpec =
            ron::de::from_bytes(bytes).map_err(|e| ScenarioError::Parse(e.to_string()))?;
        Self::from_spec(doc)
    }

    /// Validate an in-memory spec. Same rules as a parsed file, minus the hash.
    pub fn from_spec(doc: ScenarioSpec) -> Result<Self, ScenarioError> {
        let cells = doc
            .width
            .checked_mul(doc.height)
            .ok_or_else(|| ScenarioError::InvalidDimension("width*height overflow".into()))?;

        validate_version_and_dims(&doc, cells)?;
        validate_counts(&doc)?;
        validate_collision(&doc)?;
        if doc.seed == 0 {
            return Err(ScenarioError::InvalidSeed);
        }
        if doc.spawn_cells.is_empty() {
            return Err(ScenarioError::EmptySpawns);
        }

        let obstacles = normalize_obstacles(&doc.obstacle_cells, cells)?;
        if doc.version == TECHNICAL_PROTOTYPE_V1 {
            // Exact 20%: obstacles * 5 == cells. Locked workload property of the
            // gate scene only — fixtures choose their own obstacle layout.
            if obstacles
                .len()
                .checked_mul(5)
                .is_none_or(|n| n != cells as usize)
            {
                return Err(ScenarioError::InvalidObstacleRatio {
                    obstacles: obstacles.len() as u32,
                    cells,
                });
            }
        } else if obstacles.len() >= cells as usize {
            // Every scenario needs somewhere to stand. `normalize_obstacles`
            // already rejected out-of-range and duplicate indices, so this can
            // only mean "every cell is blocked".
            return Err(ScenarioError::InvalidObstacleRatio {
                obstacles: obstacles.len() as u32,
                cells,
            });
        }

        let blocked = obstacle_set(&obstacles, cells as usize);
        let dest_idx = cell_index(doc.destination, doc.width, doc.height)
            .ok_or(ScenarioError::DestinationBlocked)?;
        if blocked[dest_idx] {
            return Err(ScenarioError::DestinationBlocked);
        }

        let mut spawn_idxs = Vec::with_capacity(doc.spawn_cells.len());
        for sp in &doc.spawn_cells {
            let idx = cell_index(*sp, doc.width, doc.height)
                .ok_or(ScenarioError::InvalidSpawn { x: sp.x, y: sp.y })?;
            if blocked[idx] {
                return Err(ScenarioError::InvalidSpawn { x: sp.x, y: sp.y });
            }
            spawn_idxs.push(idx);
        }

        let reachable = flood_reachable(dest_idx, doc.width, doc.height, &blocked);
        for (sp, idx) in doc.spawn_cells.iter().zip(spawn_idxs.iter()) {
            if !reachable[*idx] {
                return Err(ScenarioError::UnreachableSpawn { x: sp.x, y: sp.y });
            }
        }

        validate_rts_block(&doc, &blocked, &reachable)?;

        Ok(Self {
            version: doc.version,
            width: doc.width,
            height: doc.height,
            cell_size_px: doc.cell_size_px,
            sprite_size_px: doc.sprite_size_px,
            hard_agent_count: doc.hard_agent_count,
            stretch_agent_count: doc.stretch_agent_count,
            seed: doc.seed,
            destination: doc.destination,
            spawn_cells: doc.spawn_cells,
            atlas_count: doc.atlas_count,
            direction_count: doc.direction_count,
            frame_count: doc.frame_count,
            collision_radius_q8: doc.collision_radius_q8,
            separation_strength_q8: doc.separation_strength_q8,
            separation_phases: doc.separation_phases,
            mass_class_count: doc.mass_class_count,
            separation_threads: doc.separation_threads,
            obstacle_cells: obstacles,
            rts: doc.rts,
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn cell_size_px(&self) -> u32 {
        self.cell_size_px
    }
    pub fn sprite_size_px(&self) -> u32 {
        self.sprite_size_px
    }
    pub fn hard_agent_count(&self) -> u32 {
        self.hard_agent_count
    }
    pub fn stretch_agent_count(&self) -> u32 {
        self.stretch_agent_count
    }
    pub fn seed(&self) -> u64 {
        self.seed
    }
    pub fn destination(&self) -> Cell {
        self.destination
    }
    pub fn spawn_cells(&self) -> &[Cell] {
        &self.spawn_cells
    }
    pub fn atlas_count(&self) -> u32 {
        self.atlas_count
    }
    pub fn direction_count(&self) -> u32 {
        self.direction_count
    }
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    pub fn collision_radius_q8(&self) -> u32 {
        self.collision_radius_q8
    }

    /// Body radius in cells. Exact: the Q8 divisor is a power of two.
    pub fn collision_radius_cells(&self) -> f32 {
        self.collision_radius_q8 as f32 / COLLISION_Q8 as f32
    }

    pub fn separation_strength_q8(&self) -> u32 {
        self.separation_strength_q8
    }

    /// Separation weight relative to the unit flow vector. Exact, as above.
    pub fn separation_strength(&self) -> f32 {
        self.separation_strength_q8 as f32 / COLLISION_Q8 as f32
    }

    pub fn separation_phases(&self) -> u32 {
        self.separation_phases
    }
    pub fn mass_class_count(&self) -> u32 {
        self.mass_class_count
    }
    pub fn separation_threads(&self) -> u32 {
        self.separation_threads
    }

    pub fn obstacle_count(&self) -> u32 {
        self.obstacle_cells.len() as u32
    }
    pub fn obstacle_cells(&self) -> &[u32] {
        &self.obstacle_cells
    }

    /// True if cell index is an obstacle.
    pub fn is_obstacle_index(&self, index: u32) -> bool {
        self.obstacle_cells.binary_search(&index).is_ok()
    }

    /// The RTS block, or `None` on every phase-0 family.
    pub fn rts(&self) -> Option<&RtsSpec> {
        self.rts.as_ref()
    }
}

/// The one site that enforces [`MAX_LIVE_AGENTS`].
///
/// Deliberately not per-family: it is called once, from the family dispatcher,
/// *before* the dispatch, so a family added later inherits the ceiling rather
/// than having to remember it. The message names the field, the value and the
/// cap so an author can tell a ceiling breach from a mistyped locked constant.
fn check_population(doc: &ScenarioSpec) -> Result<(), ScenarioError> {
    for (got, name) in [
        (doc.hard_agent_count, "hard_agent_count"),
        (doc.stretch_agent_count, "stretch_agent_count"),
    ] {
        if got > MAX_LIVE_AGENTS {
            return Err(ScenarioError::InvalidDimension(format!(
                "{name} {got} exceeds MAX_LIVE_AGENTS {MAX_LIVE_AGENTS}"
            )));
        }
    }
    Ok(())
}

fn validate_version_and_dims(doc: &ScenarioSpec, cells: u32) -> Result<(), ScenarioError> {
    let is_fixture = doc.version.starts_with(FIXTURE_VERSION_PREFIX);
    if !is_fixture
        && doc.version != COLLISION_SCENE_V1
        && doc.version != TECHNICAL_PROTOTYPE_V1
        && doc.version != RTS_PROTOTYPE_V1
    {
        return Err(ScenarioError::UnsupportedVersion(doc.version.clone()));
    }
    // The ceiling binds every recognised family, checked once before dispatch.
    check_population(doc)?;
    if is_fixture {
        return validate_fixture_dims(doc, cells);
    }
    if doc.version == COLLISION_SCENE_V1 {
        return validate_collision_scene_dims(doc);
    }
    if doc.version == RTS_PROTOTYPE_V1 {
        return validate_rts_scene_dims(doc);
    }
    let checks = [
        (doc.width, V1_WIDTH, "width"),
        (doc.height, V1_HEIGHT, "height"),
        (doc.cell_size_px, V1_CELL_PX, "cell_size_px"),
        (doc.sprite_size_px, V1_SPRITE_PX, "sprite_size_px"),
        (doc.destination.x, V1_DEST_X, "destination.x"),
        (doc.destination.y, V1_DEST_Y, "destination.y"),
        (
            doc.collision_radius_q8,
            V1_COLLISION_RADIUS_Q8,
            "collision_radius_q8",
        ),
        (
            doc.separation_strength_q8,
            V1_SEPARATION_STRENGTH_Q8,
            "separation_strength_q8",
        ),
        (
            doc.separation_phases,
            V1_SEPARATION_PHASES,
            "separation_phases",
        ),
        (doc.mass_class_count, V1_MASS_CLASSES, "mass_class_count"),
        (
            doc.separation_threads,
            V1_SEPARATION_THREADS,
            "separation_threads",
        ),
    ];
    for (got, want, name) in checks {
        if got != want {
            return Err(ScenarioError::InvalidDimension(format!(
                "{name}: got {got}, want {want}"
            )));
        }
    }
    Ok(())
}

fn validate_counts(doc: &ScenarioSpec) -> Result<(), ScenarioError> {
    // Sprite-sheet geometry is a renderer contract, not a workload knob: every
    // scenario family — fixtures included — must address the same 4 atlases of
    // 8 directions × 4 frames the atlas generator produces.
    let renderer = [
        (doc.atlas_count, V1_ATLASES, "atlas_count"),
        (doc.direction_count, V1_DIRS, "direction_count"),
        (doc.frame_count, V1_FRAMES, "frame_count"),
    ];
    // Agent counts are the frozen phase-0 workload; fixtures pick their own
    // (bounded by `validate_fixture_dims`).
    let workload = [
        (doc.hard_agent_count, V1_HARD_AGENTS, "hard_agent_count"),
        (
            doc.stretch_agent_count,
            V1_STRETCH_AGENTS,
            "stretch_agent_count",
        ),
    ];

    // Families that pick their own population: the small fixtures, the
    // collision demo scenes whose whole purpose is a different agent count,
    // and the RTS family, which is horde-free by construction and enforces
    // its own (zero) population lock in `validate_rts_scene_dims`.
    let free_workload = doc.version.starts_with(FIXTURE_VERSION_PREFIX)
        || doc.version == COLLISION_SCENE_V1
        || doc.version == RTS_PROTOTYPE_V1;
    let checks = renderer
        .iter()
        .chain(workload.iter().filter(|_| !free_workload));
    for &(got, want, name) in checks {
        if got != want {
            return Err(ScenarioError::InvalidDimension(format!(
                "{name}: got {got}, want {want}"
            )));
        }
    }
    Ok(())
}

/// Body radius and separation weight are bounded, and a weight without a body
/// is an authoring mistake rather than a silent no-op.
fn validate_collision(doc: &ScenarioSpec) -> Result<(), ScenarioError> {
    if doc.collision_radius_q8 > MAX_COLLISION_RADIUS_Q8 {
        return Err(ScenarioError::InvalidCollision(format!(
            "collision_radius_q8: got {}, max {MAX_COLLISION_RADIUS_Q8}",
            doc.collision_radius_q8
        )));
    }
    if doc.separation_strength_q8 > MAX_SEPARATION_STRENGTH_Q8 {
        return Err(ScenarioError::InvalidCollision(format!(
            "separation_strength_q8: got {}, max {MAX_SEPARATION_STRENGTH_Q8}",
            doc.separation_strength_q8
        )));
    }
    if doc.collision_radius_q8 == 0 && doc.separation_strength_q8 != 0 {
        return Err(ScenarioError::InvalidCollision(
            "separation_strength_q8 is set but collision_radius_q8 is 0; a \
             weight without a body pushes nothing"
                .into(),
        ));
    }
    for (got, max, name) in [
        (
            doc.separation_phases,
            MAX_SEPARATION_PHASES,
            "separation_phases",
        ),
        (doc.mass_class_count, MAX_MASS_CLASSES, "mass_class_count"),
        (
            doc.separation_threads,
            MAX_SEPARATION_THREADS,
            "separation_threads",
        ),
    ] {
        if got == 0 {
            return Err(ScenarioError::InvalidCollision(format!(
                "{name} must be >= 1; 1 is the identity tuning"
            )));
        }
        if got > max {
            return Err(ScenarioError::InvalidCollision(format!(
                "{name}: got {got}, max {max}"
            )));
        }
    }
    // A knob on a pass that never runs is an authoring mistake, not a no-op:
    // it reads as tuned and changes nothing.
    if doc.collision_radius_q8 == 0
        && (doc.separation_phases != 1 || doc.mass_class_count != 1 || doc.separation_threads != 1)
    {
        return Err(ScenarioError::InvalidCollision(
            "a bodyless scenario must leave separation_phases, \
             mass_class_count and separation_threads at 1; there is no \
             separation pass to tune"
                .into(),
        ));
    }
    Ok(())
}

/// Structural bounds for the `fixture_*` family: real geometry, free shape,
/// but capped so a fixture cannot quietly become another full horde workload.
fn validate_fixture_dims(doc: &ScenarioSpec, cells: u32) -> Result<(), ScenarioError> {
    let positive = [
        (doc.width, "width"),
        (doc.height, "height"),
        (doc.cell_size_px, "cell_size_px"),
        (doc.sprite_size_px, "sprite_size_px"),
        (doc.hard_agent_count, "hard_agent_count"),
    ];
    for (got, name) in positive {
        if got == 0 {
            return Err(ScenarioError::InvalidDimension(format!(
                "{name} must be > 0"
            )));
        }
    }

    if cells > FIXTURE_MAX_CELLS {
        return Err(ScenarioError::InvalidDimension(format!(
            "fixture grid {cells} cells exceeds cap {FIXTURE_MAX_CELLS}"
        )));
    }
    if doc.hard_agent_count > FIXTURE_MAX_AGENTS {
        return Err(ScenarioError::InvalidDimension(format!(
            "fixture hard_agent_count {} exceeds cap {FIXTURE_MAX_AGENTS}",
            doc.hard_agent_count
        )));
    }
    if doc.stretch_agent_count < doc.hard_agent_count {
        return Err(ScenarioError::InvalidDimension(format!(
            "stretch_agent_count {} below hard_agent_count {}",
            doc.stretch_agent_count, doc.hard_agent_count
        )));
    }
    if doc.stretch_agent_count > FIXTURE_MAX_AGENTS {
        return Err(ScenarioError::InvalidDimension(format!(
            "fixture stretch_agent_count {} exceeds cap {FIXTURE_MAX_AGENTS}",
            doc.stretch_agent_count
        )));
    }
    Ok(())
}

/// Grid geometry is locked so the demo scenes stay comparable with the gate
/// scene; population and body radius are the point of the family and stay free
/// within caps.
///
/// `width`/`height`/`cell_size_px` describe the **cell grid**, not a mapping
/// onto the screen. Since the render layer projects 2:1 isometric, a cell is
/// drawn as a `2 * cell_size_px` × `cell_size_px` tile and the grid becomes a
/// diamond wider and shorter than `width * cell_size_px` by `height *
/// cell_size_px` — bigger than the view, in the tracked scenes' case. Where
/// that grid lands on screen is the camera's business
/// ([`IsoView`](crate::render::IsoView)), not this scene contract's.
fn validate_collision_scene_dims(doc: &ScenarioSpec) -> Result<(), ScenarioError> {
    let locked = [
        (doc.width, V1_WIDTH, "width"),
        (doc.height, V1_HEIGHT, "height"),
        (doc.cell_size_px, V1_CELL_PX, "cell_size_px"),
        (doc.sprite_size_px, V1_SPRITE_PX, "sprite_size_px"),
        (doc.destination.x, V1_DEST_X, "destination.x"),
        (doc.destination.y, V1_DEST_Y, "destination.y"),
    ];
    for (got, want, name) in locked {
        if got != want {
            return Err(ScenarioError::InvalidDimension(format!(
                "{name}: got {got}, want {want}"
            )));
        }
    }
    if doc.hard_agent_count == 0 {
        return Err(ScenarioError::InvalidDimension(
            "hard_agent_count must be > 0".into(),
        ));
    }
    if doc.stretch_agent_count < doc.hard_agent_count {
        return Err(ScenarioError::InvalidDimension(format!(
            "stretch_agent_count {} below hard_agent_count {}",
            doc.stretch_agent_count, doc.hard_agent_count
        )));
    }
    // The population ceiling is `MAX_LIVE_AGENTS`, applied to every family by
    // `check_population`. A second, larger collision-scene cap would let a demo
    // scene declare a horde the engine refuses to run.
    if doc.collision_radius_q8 == 0 {
        return Err(ScenarioError::InvalidCollision(
            "a collision scene must declare a nonzero collision_radius_q8".into(),
        ));
    }
    // A body with a zero weight is worse than a bodyless scene: it validates,
    // it looks tuned, and `CollisionParams::enabled()` still returns false, so
    // the separation pass never runs and the scene demonstrates nothing.
    if doc.separation_strength_q8 == 0 {
        return Err(ScenarioError::InvalidCollision(
            "a collision scene declaring a body must also declare a nonzero \
             separation_strength_q8; a body with no weight leaves the \
             separation pass switched off"
                .into(),
        ));
    }
    Ok(())
}

/// Geometry bound for the RTS prototype family.
///
/// Width and height are a range, not a lock: phase-1.1 maps grow up to
/// [`RTS_MAX_MAP_EDGE`] per side and [`RTS_MAX_MAP_CELLS`] total, so later
/// slices can ship bigger maps without touching this contract again. Cell
/// and sprite pixel size stay exact — they are the shared atlas contract.
/// The population fields are required to be **exactly zero**: this family is
/// horde-free by construction, and a nonzero count would silently seed a
/// flow-field crowd into a base-building scene. `validate_counts` skips the
/// phase-0 workload lock for this family precisely so this stricter rule can
/// replace it.
fn validate_rts_scene_dims(doc: &ScenarioSpec) -> Result<(), ScenarioError> {
    for (got, name) in [(doc.width, "width"), (doc.height, "height")] {
        if got == 0 || got > RTS_MAX_MAP_EDGE {
            return Err(ScenarioError::InvalidDimension(format!(
                "{name}: got {got}, want 1..={RTS_MAX_MAP_EDGE}"
            )));
        }
    }
    let cells = doc.width.checked_mul(doc.height).ok_or_else(|| {
        ScenarioError::InvalidDimension(format!(
            "width {} * height {} overflows",
            doc.width, doc.height
        ))
    })?;
    if cells > RTS_MAX_MAP_CELLS {
        return Err(ScenarioError::InvalidDimension(format!(
            "width {} * height {} = {cells} cells exceeds cap {RTS_MAX_MAP_CELLS}",
            doc.width, doc.height
        )));
    }
    for (got, want, name) in [
        (doc.cell_size_px, RTS_CELL_PX, "cell_size_px"),
        (doc.sprite_size_px, RTS_SPRITE_PX, "sprite_size_px"),
        (doc.hard_agent_count, 0, "hard_agent_count"),
        (doc.stretch_agent_count, 0, "stretch_agent_count"),
    ] {
        if got != want {
            return Err(ScenarioError::InvalidDimension(format!(
                "{name}: got {got}, want {want}"
            )));
        }
    }
    Ok(())
}

/// The RTS block is present exactly on the RTS family, and every cell it names
/// is a cell a base could actually use.
///
/// `hq_cell` is additionally checked for [`BUILD_SQUARE_CELLS`] alignment, and
/// that check runs *before* the footprint bounds check so a misaligned cell
/// reports as misaligned rather than as an overflow.
fn validate_rts_block(
    doc: &ScenarioSpec,
    blocked: &[bool],
    reachable: &[bool],
) -> Result<(), ScenarioError> {
    let is_rts = doc.version == RTS_PROTOTYPE_V1;
    if is_rts != doc.rts.is_some() {
        return Err(ScenarioError::InvalidRts(format!(
            "version {:?}: rts block must be present iff version == {RTS_PROTOTYPE_V1:?}",
            doc.version
        )));
    }
    let Some(rts) = doc.rts.as_ref() else {
        return Ok(());
    };

    if rts.start_crystal > MAX_START_RESOURCE {
        return Err(ScenarioError::InvalidRts(format!(
            "start_crystal: got {}, max {MAX_START_RESOURCE}",
            rts.start_crystal
        )));
    }
    if rts.start_gas > MAX_START_RESOURCE {
        return Err(ScenarioError::InvalidRts(format!(
            "start_gas: got {}, max {MAX_START_RESOURCE}",
            rts.start_gas
        )));
    }

    if rts.start_supply_cap == 0 || rts.start_supply_cap > MAX_SUPPLY_CAP {
        return Err(ScenarioError::InvalidRts(format!(
            "start_supply_cap: got {}, want 1..={MAX_SUPPLY_CAP}",
            rts.start_supply_cap
        )));
    }

    if rts.crystal_nodes.is_empty() {
        return Err(ScenarioError::InvalidRts(
            "crystal_nodes must not be empty".into(),
        ));
    }
    if rts.crystal_nodes.len() > MAX_RESOURCE_NODES {
        return Err(ScenarioError::InvalidRts(format!(
            "crystal_nodes: {} exceeds cap {MAX_RESOURCE_NODES}",
            rts.crystal_nodes.len()
        )));
    }
    if rts.gas_nodes.is_empty() {
        return Err(ScenarioError::InvalidRts(
            "gas_nodes must not be empty".into(),
        ));
    }
    if rts.gas_nodes.len() > MAX_RESOURCE_NODES {
        return Err(ScenarioError::InvalidRts(format!(
            "gas_nodes: {} exceeds cap {MAX_RESOURCE_NODES}",
            rts.gas_nodes.len()
        )));
    }

    let all_nodes: Vec<&Cell> = rts
        .crystal_nodes
        .iter()
        .chain(rts.gas_nodes.iter())
        .collect();
    for cell in &all_nodes {
        let idx = cell_index(**cell, doc.width, doc.height).ok_or_else(|| {
            ScenarioError::InvalidRts(format!("node ({}, {}) is out of bounds", cell.x, cell.y))
        })?;
        if blocked[idx] {
            return Err(ScenarioError::InvalidRts(format!(
                "node ({}, {}) is blocked",
                cell.x, cell.y
            )));
        }
        if !reachable[idx] {
            return Err(ScenarioError::InvalidRts(format!(
                "node ({}, {}) is unreachable from the destination",
                cell.x, cell.y
            )));
        }
    }

    let mut seen: Vec<Cell> = Vec::with_capacity(all_nodes.len());
    for cell in &all_nodes {
        if seen.contains(*cell) {
            return Err(ScenarioError::InvalidRts(format!(
                "cell ({}, {}) appears in both crystal_nodes and gas_nodes",
                cell.x, cell.y
            )));
        }
        seen.push(**cell);
    }

    // Alignment is checked *before* bounds so a misaligned cell reports as
    // misaligned rather than as a footprint overflow.
    if rts.hq_cell.x % BUILD_SQUARE_CELLS != 0 || rts.hq_cell.y % BUILD_SQUARE_CELLS != 0 {
        return Err(ScenarioError::InvalidRts(format!(
            "hq_cell ({}, {}): not aligned to the {BUILD_SQUARE_CELLS}-cell build square",
            rts.hq_cell.x, rts.hq_cell.y
        )));
    }

    let hq_min_x = rts.hq_cell.x;
    let hq_min_y = rts.hq_cell.y;
    let hq_max_x = hq_min_x.checked_add(HQ_FOOTPRINT_CELLS);
    let hq_max_y = hq_min_y.checked_add(HQ_FOOTPRINT_CELLS);
    let (Some(hq_max_x), Some(hq_max_y)) = (hq_max_x, hq_max_y) else {
        return Err(ScenarioError::InvalidRts(format!(
            "hq_cell ({hq_min_x}, {hq_min_y}): footprint overflows"
        )));
    };
    if hq_max_x > doc.width || hq_max_y > doc.height {
        return Err(ScenarioError::InvalidRts(format!(
            "hq_cell ({hq_min_x}, {hq_min_y}): {HQ_FOOTPRINT_CELLS}x{HQ_FOOTPRINT_CELLS} footprint is out of bounds on a {}x{} grid",
            doc.width, doc.height
        )));
    }
    for y in hq_min_y..hq_max_y {
        for x in hq_min_x..hq_max_x {
            let idx = (x + y * doc.width) as usize;
            if blocked[idx] {
                return Err(ScenarioError::InvalidRts(format!(
                    "hq footprint cell ({x}, {y}) is blocked"
                )));
            }
        }
    }

    let in_hq_footprint =
        |c: Cell| c.x >= hq_min_x && c.x < hq_max_x && c.y >= hq_min_y && c.y < hq_max_y;
    for cell in &all_nodes {
        if in_hq_footprint(**cell) {
            return Err(ScenarioError::InvalidRts(format!(
                "node ({}, {}) lies inside the HQ footprint",
                cell.x, cell.y
            )));
        }
    }
    for sp in &doc.spawn_cells {
        if in_hq_footprint(*sp) {
            return Err(ScenarioError::InvalidRts(format!(
                "spawn ({}, {}) lies inside the HQ footprint",
                sp.x, sp.y
            )));
        }
    }

    // Enemy block: every cell it names must be ground an enemy could
    // actually stand on and walk out of, and the whole scripted invasion
    // must stay under the entity budget.
    if let Some(enemies) = rts.enemies.as_ref() {
        let enemy_cell_ok = |cell: &Cell, what: &str| -> Result<(), ScenarioError> {
            let idx = cell_index(*cell, doc.width, doc.height).ok_or_else(|| {
                ScenarioError::InvalidRts(format!(
                    "{what} ({}, {}) is out of bounds",
                    cell.x, cell.y
                ))
            })?;
            if blocked[idx] {
                return Err(ScenarioError::InvalidRts(format!(
                    "{what} ({}, {}) is blocked",
                    cell.x, cell.y
                )));
            }
            if !reachable[idx] {
                return Err(ScenarioError::InvalidRts(format!(
                    "{what} ({}, {}) is unreachable from the destination",
                    cell.x, cell.y
                )));
            }
            if in_hq_footprint(*cell) {
                return Err(ScenarioError::InvalidRts(format!(
                    "{what} ({}, {}) lies inside the HQ footprint",
                    cell.x, cell.y
                )));
            }
            if all_nodes.iter().any(|n| **n == *cell) {
                return Err(ScenarioError::InvalidRts(format!(
                    "{what} ({}, {}) sits on a resource node",
                    cell.x, cell.y
                )));
            }
            Ok(())
        };
        for c in &enemies.pre_placed {
            enemy_cell_ok(c, "enemy pre_placed cell")?;
        }
        for c in &enemies.spawn_points {
            enemy_cell_ok(c, "enemy spawn_point cell")?;
        }

        let mut total = enemies.pre_placed.len() as u64;
        let mut prev_tick: Option<u32> = None;
        for (i, w) in enemies.waves.iter().enumerate() {
            if w.count == 0 {
                return Err(ScenarioError::InvalidRts(format!(
                    "wave {i}: count must be >= 1"
                )));
            }
            if (w.spawn_point as usize) >= enemies.spawn_points.len() {
                return Err(ScenarioError::InvalidRts(format!(
                    "wave {i}: spawn_point {} out of range ({} spawn points)",
                    w.spawn_point,
                    enemies.spawn_points.len()
                )));
            }
            if prev_tick.is_some_and(|p| w.at_tick < p) {
                return Err(ScenarioError::InvalidRts(format!(
                    "wave {i}: at_tick {} is not sorted non-decreasing",
                    w.at_tick
                )));
            }
            prev_tick = Some(w.at_tick);
            total += u64::from(w.count);
        }
        if total > u64::from(MAX_ENEMIES) {
            return Err(ScenarioError::InvalidRts(format!(
                "enemy total {total} exceeds cap {MAX_ENEMIES}"
            )));
        }
    }

    Ok(())
}

fn normalize_obstacles(raw: &[u32], cells: u32) -> Result<Vec<u32>, ScenarioError> {
    let mut out = raw.to_vec();
    out.sort_unstable();
    let mut prev: Option<u32> = None;
    for &idx in &out {
        if idx >= cells {
            return Err(ScenarioError::InvalidObstacleIndex(idx));
        }
        if prev == Some(idx) {
            return Err(ScenarioError::DuplicateObstacle(idx));
        }
        prev = Some(idx);
    }
    Ok(out)
}

fn cell_index(cell: Cell, width: u32, height: u32) -> Option<usize> {
    if cell.x >= width || cell.y >= height {
        return None;
    }
    Some((cell.x + cell.y * width) as usize)
}

fn obstacle_set(obstacles: &[u32], cells: usize) -> Vec<bool> {
    let mut blocked = vec![false; cells];
    for &idx in obstacles {
        blocked[idx as usize] = true;
    }
    blocked
}

/// 4-connected flood fill from destination over free cells.
fn flood_reachable(origin: usize, width: u32, height: u32, blocked: &[bool]) -> Vec<bool> {
    let w = width as usize;
    let h = height as usize;
    let mut seen = vec![false; w * h];
    let mut stack = vec![origin];
    seen[origin] = true;
    while let Some(i) = stack.pop() {
        let x = i % w;
        let y = i / w;
        let neighbors = [
            (x.wrapping_sub(1), y, x > 0),
            (x + 1, y, x + 1 < w),
            (x, y.wrapping_sub(1), y > 0),
            (x, y + 1, y + 1 < h),
        ];
        for (nx, ny, ok) in neighbors {
            if !ok {
                continue;
            }
            let ni = nx + ny * w;
            if seen[ni] || blocked[ni] {
                continue;
            }
            seen[ni] = true;
            stack.push(ni);
        }
    }
    seen
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_math_20_percent() {
        assert_eq!(V1_WIDTH * V1_HEIGHT / 5, 25_920);
    }
}
