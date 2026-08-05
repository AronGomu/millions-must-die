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
///
/// `groups` borrows reusable pack buffers on [`Runtime`] — valid until next
/// [`Runtime::tick_and_render`].
#[derive(Debug, Clone, Copy)]
pub struct FrameOutput<'a> {
    pub tick_index: u64,
    pub agent_count: usize,
    pub paused: bool,
    pub overlay_visible: bool,
    pub groups: &'a [DrawGroup; ATLAS_COUNT],
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
    scenario: Scenario,
    cell_size_px: f32,
    sprite_size_px: f32,
    /// Retained so navigation state stays inspectable for tests and future
    /// systems (debug overlay, repathing) instead of being dropped after init.
    field: FlowField,
    sim: Simulation,
    paused: bool,
    overlay_visible: bool,
    last_stats: FrameStats,
    /// Reused atlas buckets (capacity reserved at load → post-warmup pack is zero-alloc).
    groups: [DrawGroup; ATLAS_COUNT],
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
            return Err(RuntimeError::AgentCount { got: count, cap });
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

        // Worst case: all agents land in one atlas → reserve full count per group.
        let n = count as usize;
        let groups = std::array::from_fn(|i| DrawGroup {
            atlas_id: i as u32,
            instances: Vec::with_capacity(n),
        });

        Ok(Self {
            cell_size_px: scenario.cell_size_px() as f32,
            sprite_size_px: scenario.sprite_size_px() as f32,
            scenario,
            field,
            sim,
            paused: false,
            overlay_visible: false,
            last_stats: FrameStats::default(),
            groups,
        })
    }

    pub fn scenario_version(&self) -> &str {
        self.scenario.version()
    }

    /// The verified scenario this runtime was built from.
    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    /// The flow field built from this scenario at load.
    ///
    /// Note: [`Simulation`] copies the field's vectors into its own SoA arrays
    /// at construction, so this is a *separate* copy. In particular
    /// [`Simulation::set_vector_for_test`] mutates the sim's copy only — after
    /// such an override the two views intentionally disagree, and this one
    /// still reflects the field as built.
    pub fn flow_field(&self) -> &FlowField {
        &self.field
    }

    /// Read-only SoA agent state (positions, atlas/dir/frame).
    pub fn agents(&self) -> crate::sim::AgentsView<'_> {
        self.sim.agents()
    }

    pub fn sim(&self) -> &Simulation {
        &self.sim
    }

    /// Mutable sim access for tests that need to place an agent or override a
    /// field vector before stepping.
    ///
    /// Feature-gated with the harness that consumes it: `set_position` /
    /// `set_vector_for_test` must not be reachable from a shipping build.
    #[cfg(feature = "testkit")]
    pub fn sim_mut(&mut self) -> &mut Simulation {
        &mut self.sim
    }

    /// Set the pause state directly (as opposed to toggling it via an input
    /// action).
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
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

    /// Borrow last packed draw groups (updated by [`Self::tick_and_render`]).
    pub fn draw_groups(&self) -> &[DrawGroup; ATLAS_COUNT] {
        &self.groups
    }

    /// Apply one input action. `Quit` is observed by caller (no local side effect).
    pub fn apply_action(&mut self, action: InputAction) {
        match action {
            InputAction::Quit => {}
            InputAction::ToggleOverlay => self.overlay_visible = !self.overlay_visible,
            InputAction::TogglePause => self.paused = !self.paused,
        }
    }

    /// Advance the simulation one tick without packing instances or reading a
    /// clock. Returns `false` when paused (no tick applied).
    ///
    /// This is the deterministic stepping path: [`Self::tick_and_render`]
    /// additionally samples [`Instant`] to fill [`FrameStats`], which is fine
    /// for the interactive loop but means a sim-only test would carry a
    /// wall-clock read it never uses.
    pub fn tick_only(&mut self) -> bool {
        if self.paused {
            return false;
        }
        self.sim.tick();
        true
    }

    /// Rebuild draw groups from the current sim state without advancing it.
    /// Reuses the pack buffers, so no allocation after load.
    pub fn pack_groups(&mut self) {
        pack_instance_groups(
            self.sim.agents(),
            self.cell_size_px,
            self.sprite_size_px,
            &mut self.groups,
        );
    }

    /// One frame: optional sim tick + rebuild draw groups into reused buffers.
    pub fn tick_and_render(&mut self) -> FrameOutput<'_> {
        let t0 = Instant::now();

        let sim_ms = if self.paused {
            0.0
        } else {
            let s = Instant::now();
            self.sim.tick();
            s.elapsed().as_secs_f64() * 1000.0
        };

        let u0 = Instant::now();
        self.pack_groups();
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
            groups: &self.groups,
            stats,
            state_hash: self.sim.state_hash(),
        }
    }
}

/// Pack SoA agents into existing atlas groups (clear + push; no realloc if capacity holds).
pub fn pack_instance_groups(
    agents: crate::sim::AgentsView<'_>,
    cell_size_px: f32,
    sprite_size_px: f32,
    out: &mut [DrawGroup; ATLAS_COUNT],
) {
    for (i, g) in out.iter_mut().enumerate() {
        g.atlas_id = i as u32;
        g.instances.clear();
    }

    let half = sprite_size_px * 0.5;
    let size = [sprite_size_px, sprite_size_px];
    let n = agents.x.len();

    for i in 0..n {
        let atlas = agents.atlas[i] as usize % ATLAS_COUNT;
        let px = agents.x[i] * cell_size_px - half;
        let py = agents.y[i] * cell_size_px - half;
        let uv = frame_uv_rect(u32::from(agents.dir[i]), u32::from(agents.frame[i]));
        out[atlas].instances.push(SpriteInstance::new(
            [px, py],
            size,
            uv,
            SpriteInstance::WHITE,
        ));
    }
}

/// Convert SoA agent view → 4 atlas draw groups (allocating; prefer [`pack_instance_groups`]).
pub fn build_instance_groups(
    agents: crate::sim::AgentsView<'_>,
    cell_size_px: f32,
    sprite_size_px: f32,
) -> [DrawGroup; ATLAS_COUNT] {
    let n = agents.x.len();
    let mut out = std::array::from_fn(|i| DrawGroup {
        atlas_id: i as u32,
        instances: Vec::with_capacity(n),
    });
    pack_instance_groups(agents, cell_size_px, sprite_size_px, &mut out);
    out
}
