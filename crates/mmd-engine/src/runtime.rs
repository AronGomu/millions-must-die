//! Interactive frame composition: scenario → field → sim → instances.

use std::path::Path;
use std::time::Instant;

use thiserror::Error;

use crate::nav::flow_field::FlowField;
use crate::render::{ATLAS_COUNT, DrawGroup, SpriteInstance, frame_uv_rect};
use crate::scenario::{Scenario, ScenarioError};
use crate::sim::Simulation;

/// Stable logical key bindings (OS scancodes mapped in app).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoundKey {
    Escape,
    F1,
    Space,
}

/// Input actions consumed by [`Runtime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum InputAction {
    Quit = 0,
    ToggleOverlay = 1,
    TogglePause = 2,
}

/// Map bound key → action. Contract locked by `input_actions_are_stable`.
pub fn action_for_key(key: BoundKey) -> InputAction {
    match key {
        BoundKey::Escape => InputAction::Quit,
        BoundKey::F1 => InputAction::ToggleOverlay,
        BoundKey::Space => InputAction::TogglePause,
    }
}

/// Per-frame CPU timings (milliseconds).
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameStats {
    /// Sim step only (0 when paused).
    pub sim_ms: f64,
    /// SoA → instance pack time.
    pub upload_ms: f64,
    /// Full `tick_and_render` wall time.
    pub total_ms: f64,
}

/// Result of one runtime frame.
#[derive(Debug, Clone)]
pub struct FrameOutput {
    pub tick_index: u64,
    pub agent_count: usize,
    pub paused: bool,
    pub overlay_visible: bool,
    pub groups: [DrawGroup; ATLAS_COUNT],
    pub stats: FrameStats,
    pub state_hash: [u8; 32],
}

/// Runtime load failures.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Scenario(#[from] ScenarioError),
    #[error("agent count {got} exceeds stretch cap {cap}")]
    AgentCount { got: u32, cap: u32 },
    #[error("agent count must be > 0")]
    ZeroAgents,
}

/// Scenario + flow field + SoA sim + instance builder.
#[derive(Debug)]
pub struct Runtime {
    scenario_version: String,
    cell_size_px: f32,
    sprite_size_px: f32,
    sim: Simulation,
    paused: bool,
    overlay_visible: bool,
    last_stats: FrameStats,
}

impl Runtime {
    /// Load verified scenario; build field + sim.
    ///
    /// `agent_count_override` when `Some` replaces hard count (inspection CLI).
    /// Gate scene remains the locked default (`None` → hard_agent_count).
    pub fn load(
        scenario_path: impl AsRef<Path>,
        agent_count_override: Option<u32>,
    ) -> Result<Self, RuntimeError> {
        let scenario = Scenario::load_verified(scenario_path.as_ref())?;
        Self::from_scenario(scenario, agent_count_override)
    }

    /// Build from already-validated scenario.
    pub fn from_scenario(
        scenario: Scenario,
        agent_count_override: Option<u32>,
    ) -> Result<Self, RuntimeError> {
        let count = agent_count_override.unwrap_or(scenario.hard_agent_count());
        if count == 0 {
            return Err(RuntimeError::ZeroAgents);
        }
        let cap = scenario.stretch_agent_count();
        if count > cap {
            return Err(RuntimeError::AgentCount {
                got: count,
                cap,
            });
        }

        let field = FlowField::from_scenario(&scenario);
        let sim = if count == scenario.hard_agent_count() {
            Simulation::new(&scenario, &field)
        } else {
            Simulation::new_custom(
                &field,
                scenario.destination(),
                scenario.spawn_cells(),
                count as usize,
                scenario.atlas_count() as u8,
                scenario.direction_count() as u8,
                scenario.frame_count() as u8,
            )
        };

        Ok(Self {
            scenario_version: scenario.version().to_string(),
            cell_size_px: scenario.cell_size_px() as f32,
            sprite_size_px: scenario.sprite_size_px() as f32,
            sim,
            paused: false,
            overlay_visible: false,
            last_stats: FrameStats::default(),
        })
    }

    pub fn scenario_version(&self) -> &str {
        &self.scenario_version
    }

    pub fn agent_count(&self) -> usize {
        self.sim.agent_count()
    }

    pub fn tick_index(&self) -> u64 {
        self.sim.tick_index()
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn overlay_visible(&self) -> bool {
        self.overlay_visible
    }

    pub fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    pub fn state_hash(&self) -> [u8; 32] {
        self.sim.state_hash()
    }

    /// Apply one input action. `Quit` is observed by caller (no local side effect).
    pub fn apply_action(&mut self, action: InputAction) {
        match action {
            InputAction::Quit => {}
            InputAction::ToggleOverlay => self.overlay_visible = !self.overlay_visible,
            InputAction::TogglePause => self.paused = !self.paused,
        }
    }

    /// One frame: optional sim tick + rebuild draw groups.
    pub fn tick_and_render(&mut self) -> FrameOutput {
        let t0 = Instant::now();

        let sim_ms = if self.paused {
            0.0
        } else {
            let s = Instant::now();
            self.sim.tick();
            s.elapsed().as_secs_f64() * 1000.0
        };

        let u0 = Instant::now();
        let groups = build_instance_groups(
            self.sim.agents(),
            self.cell_size_px,
            self.sprite_size_px,
        );
        let upload_ms = u0.elapsed().as_secs_f64() * 1000.0;
        let total_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let stats = FrameStats {
            sim_ms,
            upload_ms,
            total_ms,
        };
        self.last_stats = stats;

        FrameOutput {
            tick_index: self.sim.tick_index(),
            agent_count: self.sim.agent_count(),
            paused: self.paused,
            overlay_visible: self.overlay_visible,
            groups,
            stats,
            state_hash: self.sim.state_hash(),
        }
    }
}

/// Convert SoA agent view → 4 atlas draw groups (pixel space, top-left origin).
pub fn build_instance_groups(
    agents: crate::sim::AgentsView<'_>,
    cell_size_px: f32,
    sprite_size_px: f32,
) -> [DrawGroup; ATLAS_COUNT] {
    let half = sprite_size_px * 0.5;
    let size = [sprite_size_px, sprite_size_px];
    let n = agents.x.len();
    let mut buckets: [Vec<SpriteInstance>; ATLAS_COUNT] =
        std::array::from_fn(|_| Vec::with_capacity(n / ATLAS_COUNT + 1));

    for i in 0..n {
        let atlas = agents.atlas[i] as usize % ATLAS_COUNT;
        let px = agents.x[i] * cell_size_px - half;
        let py = agents.y[i] * cell_size_px - half;
        let uv = frame_uv_rect(u32::from(agents.dir[i]), u32::from(agents.frame[i]));
        buckets[atlas].push(SpriteInstance::new(
            [px, py],
            size,
            uv,
            SpriteInstance::WHITE,
        ));
    }

    std::array::from_fn(|i| DrawGroup {
        atlas_id: i as u32,
        instances: std::mem::take(&mut buckets[i]),
    })
}
