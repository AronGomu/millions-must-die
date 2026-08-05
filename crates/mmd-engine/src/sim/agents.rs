//! SoA agent storage + construction.

use sha2::{Digest, Sha256};

use crate::nav::flow_field::{COST_OBSTACLE, FlowField};
use crate::scenario::{Cell, Scenario};

use super::tick;

/// Position quantum: 1/256 cell.
pub const POS_QUANTUM_PER_CELL: i32 = 256;

/// Quantize cell-space position to integer 1/256 units.
#[inline]
pub fn quantize_cell(v: f32) -> i32 {
    (v * POS_QUANTUM_PER_CELL as f32).round() as i32
}

/// Read-only SoA slices for render / tests.
#[derive(Debug, Clone, Copy)]
pub struct AgentsView<'a> {
    pub x: &'a [f32],
    pub y: &'a [f32],
    pub atlas: &'a [u8],
    pub dir: &'a [u8],
    pub frame: &'a [u8],
}

/// Fixed-timestep horde simulation.
#[derive(Debug, Clone)]
pub struct Simulation {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) dest_cx: u32,
    pub(super) dest_cy: u32,
    pub(super) dest_x: f32,
    pub(super) dest_y: f32,
    pub(super) field_vx: Vec<f32>,
    pub(super) field_vy: Vec<f32>,
    pub(super) blocked: Vec<bool>,
    pub(super) spawn_x: Vec<f32>,
    pub(super) spawn_y: Vec<f32>,
    /// Population at construction. Kept explicitly so the recycle counters stay
    /// correct if the population ever becomes variable (deaths, dynamic spawn).
    pub(super) initial_agent_count: u64,
    pub(super) recycle_cursor: u64,
    pub(super) tick_index: u64,
    #[allow(dead_code)]
    pub(super) atlas_count: u8,
    #[allow(dead_code)]
    pub(super) dir_count: u8,
    pub(super) frame_count: u8,
    pub(super) x: Vec<f32>,
    pub(super) y: Vec<f32>,
    pub(super) atlas: Vec<u8>,
    pub(super) dir: Vec<u8>,
    pub(super) frame: Vec<u8>,
}

impl Simulation {
    /// Build full hard-count sim from verified scenario + field.
    pub fn new(scenario: &Scenario, field: &FlowField) -> Self {
        Self::new_custom(
            field,
            scenario.destination(),
            scenario.spawn_cells(),
            scenario.hard_agent_count() as usize,
            scenario.atlas_count() as u8,
            scenario.direction_count() as u8,
            scenario.frame_count() as u8,
        )
    }

    /// Construct with explicit counts (unit tests + stretch).
    pub fn new_custom(
        field: &FlowField,
        destination: Cell,
        spawn_cells: &[Cell],
        agent_count: usize,
        atlas_count: u8,
        dir_count: u8,
        frame_count: u8,
    ) -> Self {
        assert!(!spawn_cells.is_empty(), "spawn list empty");
        assert!(agent_count > 0, "agent_count zero");
        assert!(atlas_count > 0 && dir_count > 0 && frame_count > 0);

        let width = field.width();
        let height = field.height();
        let n_cells = (width as usize)
            .checked_mul(height as usize)
            .expect("grid size");

        let mut field_vx = Vec::with_capacity(n_cells);
        let mut field_vy = Vec::with_capacity(n_cells);
        let mut blocked = Vec::with_capacity(n_cells);
        for y in 0..height {
            for x in 0..width {
                let (vx, vy) = field.vector_at(x, y);
                field_vx.push(vx);
                field_vy.push(vy);
                blocked.push(field.cost_at(x, y) == COST_OBSTACLE);
            }
        }

        let mut spawn_x = Vec::with_capacity(spawn_cells.len());
        let mut spawn_y = Vec::with_capacity(spawn_cells.len());
        for s in spawn_cells {
            spawn_x.push(s.x as f32 + 0.5);
            spawn_y.push(s.y as f32 + 0.5);
        }

        let mut x = Vec::with_capacity(agent_count);
        let mut y = Vec::with_capacity(agent_count);
        let mut atlas = Vec::with_capacity(agent_count);
        let mut dir = Vec::with_capacity(agent_count);
        let mut frame = Vec::with_capacity(agent_count);

        let n_spawn = spawn_cells.len();
        let ac = atlas_count as usize;
        let dc = dir_count as usize;
        let fc = frame_count as usize;

        for i in 0..agent_count {
            let si = i % n_spawn;
            x.push(spawn_x[si]);
            y.push(spawn_y[si]);

            // Independent moduli → exact even counts when n divides 4/8/4.
            let a = (i % ac) as u8;
            let d = (i % dc) as u8;
            let f = ((i / ac) % fc) as u8;
            atlas.push(a);
            dir.push(d);
            frame.push(f);
        }

        Self {
            width,
            height,
            dest_cx: destination.x,
            dest_cy: destination.y,
            dest_x: destination.x as f32 + 0.5,
            dest_y: destination.y as f32 + 0.5,
            field_vx,
            field_vy,
            blocked,
            spawn_x,
            spawn_y,
            initial_agent_count: agent_count as u64,
            recycle_cursor: agent_count as u64,
            tick_index: 0,
            atlas_count,
            dir_count,
            frame_count,
            x,
            y,
            atlas,
            dir,
            frame,
        }
    }

    pub fn agent_count(&self) -> usize {
        self.x.len()
    }

    pub fn tick_index(&self) -> u64 {
        self.tick_index
    }

    /// Arrivals recycled back to a spawn cell since construction.
    ///
    /// The recycle cursor starts at the initial population (that seeding
    /// consumed one spawn slot per agent) and advances once per arrival, so the
    /// difference is the recycle count. Measured against the *initial* count,
    /// not the live one, so it stays correct if the population ever varies.
    pub fn recycle_count(&self) -> u64 {
        self.recycle_cursor - self.initial_agent_count
    }

    /// Total spawn events issued: the initial seeding plus every recycle.
    /// Balances against [`Self::agent_count`] + [`Self::recycle_count`].
    pub fn spawn_count(&self) -> u64 {
        self.recycle_cursor
    }

    pub fn agents(&self) -> AgentsView<'_> {
        AgentsView {
            x: &self.x,
            y: &self.y,
            atlas: &self.atlas,
            dir: &self.dir,
            frame: &self.frame,
        }
    }

    /// Test helper: overwrite one agent position.
    pub fn set_position(&mut self, index: usize, px: f32, py: f32) {
        self.x[index] = px;
        self.y[index] = py;
    }

    /// Test helper: overwrite one cell descent vector.
    pub fn set_vector_for_test(&mut self, cx: u32, cy: u32, vx: f32, vy: f32) {
        let i = (cx + cy * self.width) as usize;
        self.field_vx[i] = vx;
        self.field_vy[i] = vy;
    }

    /// One fixed 1/60 s step for all agents.
    pub fn tick(&mut self) {
        tick::step(self);
    }

    /// Exact same-platform state digest (raw f32 bits + discrete channels).
    pub fn state_hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.tick_index.to_le_bytes());
        h.update((self.x.len() as u64).to_le_bytes());
        for v in &self.x {
            h.update(v.to_bits().to_le_bytes());
        }
        for v in &self.y {
            h.update(v.to_bits().to_le_bytes());
        }
        h.update(&self.atlas);
        h.update(&self.dir);
        h.update(&self.frame);
        h.finalize().into()
    }

    /// Cross-platform digest: positions quantized to 1/256 cell; dir/frame/atlas exact.
    pub fn quantized_state_hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.tick_index.to_le_bytes());
        h.update((self.x.len() as u64).to_le_bytes());
        for v in &self.x {
            h.update(quantize_cell(*v).to_le_bytes());
        }
        for v in &self.y {
            h.update(quantize_cell(*v).to_le_bytes());
        }
        h.update(&self.atlas);
        h.update(&self.dir);
        h.update(&self.frame);
        h.finalize().into()
    }
}
