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
/// 480×270 grid, the 50k/100k agent counts, and the exact-20% obstacle ratio.
/// Those stay locked for [`TECHNICAL_PROTOTYPE_V1`] alone.
pub const FIXTURE_VERSION_PREFIX: &str = "fixture_";

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

const V1_WIDTH: u32 = 480;
const V1_HEIGHT: u32 = 270;
const V1_CELL_PX: u32 = 4;
const V1_SPRITE_PX: u32 = 30;
const V1_HARD_AGENTS: u32 = 50_000;
const V1_STRETCH_AGENTS: u32 = 100_000;
const V1_ATLASES: u32 = 4;
const V1_DIRS: u32 = 8;
const V1_FRAMES: u32 = 4;
const V1_COLLISION_RADIUS_Q8: u32 = 102;
const V1_SEPARATION_STRENGTH_Q8: u32 = 256;
const V1_DEST_X: u32 = 240;
const V1_DEST_Y: u32 = 135;

/// Grid cell coordinate (cell space, not pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Cell {
    pub x: u32,
    pub y: u32,
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
    /// Sorted unique obstacle cell indices (`x + y * width`).
    obstacle_cells: Vec<u32>,
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
    pub obstacle_cells: Vec<u32>,
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
            obstacle_cells: obstacles,
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
}

fn validate_version_and_dims(doc: &ScenarioSpec, cells: u32) -> Result<(), ScenarioError> {
    if doc.version.starts_with(FIXTURE_VERSION_PREFIX) {
        return validate_fixture_dims(doc, cells);
    }
    if doc.version != TECHNICAL_PROTOTYPE_V1 {
        return Err(ScenarioError::UnsupportedVersion(doc.version.clone()));
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

    let is_fixture = doc.version.starts_with(FIXTURE_VERSION_PREFIX);
    let checks = renderer
        .iter()
        .chain(workload.iter().filter(|_| !is_fixture));
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
    Ok(())
}

/// Structural bounds for the `fixture_*` family: real geometry, free shape,
/// but capped so a fixture cannot quietly become another 50k workload.
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
