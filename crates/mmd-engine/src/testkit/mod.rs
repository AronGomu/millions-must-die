//! Deterministic headless test harness (T29).
//!
//! One entry point every game-system test drives: build a runtime from a
//! scenario source + agent count + seed, step an exact number of ticks, and
//! read the resulting state.
//!
//! # Determinism contract
//!
//! * [`Harness::step`] advances through [`Runtime::tick_only`], which reads no
//!   clock. Nothing in the stepping path samples [`std::time::Instant`],
//!   spawns a thread, or touches a GPU device, so the same inputs produce the
//!   same [`Harness::state_hash`] on every run and in every process on a host.
//! * `seed == 0` means *canonical order* — byte-identical to constructing
//!   [`Runtime`] directly. A nonzero seed deterministically redistributes
//!   agents across the scenario's spawn cells; it never changes the locked
//!   atlas/direction/frame assignment contract.
//! * Fixtures are hash-verified like the gate scene, so a drifted asset fails
//!   loudly instead of silently changing every recorded hash.
//!
//! # Extending it
//!
//! Future phase-1 systems (camera, selection, workers) reach the live
//! [`Runtime`] through [`Harness::runtime_mut`] and take independent
//! reproducible randomness through [`Harness::rng`] + [`SplitMix64::derive`],
//! so adding a system needs no harness redesign.
//!
//! ```no_run
//! use mmd_engine::testkit::{FIXTURE_SMALL_V1, Harness};
//!
//! let mut h = Harness::fixture(FIXTURE_SMALL_V1).agents(64).seed(42).build().unwrap();
//! h.step_exact(500);
//! assert_eq!(h.alive_count(), 64);
//! ```

mod fixtures;
mod rng;

pub use fixtures::{
    ALL_FIXTURES, FIXTURE_CORRIDOR_V1, FIXTURE_DIR, FIXTURE_SMALL_V1, GATE_SCENARIO, fixture_path,
    gate_scenario_path,
};
pub use rng::SplitMix64;

use std::path::PathBuf;

use thiserror::Error;

use crate::nav::flow_field::FlowField;
use crate::render::{ATLAS_COUNT, DrawGroup};
use crate::runtime::{Runtime, RuntimeError};
use crate::scenario::{
    Cell, FIXTURE_MAX_AGENTS, Scenario, ScenarioError, ScenarioSpec, TECHNICAL_PROTOTYPE_V1,
};
use crate::sim::{AgentsView, Simulation};

/// Version id stamped on synthetic in-memory grids.
pub const INLINE_GRID_VERSION: &str = "fixture_inline_grid_v1";

/// Harness construction failures.
#[derive(Debug, Error)]
pub enum HarnessError {
    #[error(transparent)]
    Scenario(#[from] ScenarioError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

/// Where a harness gets its scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioSource {
    /// The real 50k phase-0 workload. Hash-verified.
    GateScene,
    /// A tracked fixture under [`FIXTURE_DIR`], by name. Hash-verified.
    Fixture(String),
    /// An arbitrary scenario file. Hash-verified via its `.sha256` sidecar.
    Path(PathBuf),
    /// A synthetic in-memory grid — no file, no hash, same validator.
    Inline(Box<ScenarioSpec>),
}

impl ScenarioSource {
    pub fn fixture(name: impl Into<String>) -> Self {
        Self::Fixture(name.into())
    }

    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path(path.into())
    }

    pub fn grid(spec: impl Into<ScenarioSpec>) -> Self {
        Self::Inline(Box::new(spec.into()))
    }

    /// Load and validate the scenario this source names.
    pub fn load(&self) -> Result<Scenario, ScenarioError> {
        match self {
            Self::GateScene => Scenario::load_verified(gate_scenario_path()),
            Self::Fixture(name) => Scenario::load_verified(fixture_path(name)),
            Self::Path(path) => Scenario::load_verified(path),
            Self::Inline(spec) => Scenario::from_spec((**spec).clone()),
        }
    }
}

/// Synthetic grid description for unit-scale tests.
///
/// Lets a surgical test (one agent, one hand-placed obstacle) use the same
/// [`Harness`] entry point as a full-scenario system test, instead of a
/// parallel construction path that could drift from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridSpec {
    pub width: u32,
    pub height: u32,
    pub destination: Cell,
    pub obstacle_cells: Vec<u32>,
    pub spawn_cells: Vec<Cell>,
    /// Scenario hard agent count. [`HarnessBuilder::agents`] overrides how many
    /// agents a run actually creates; this is the scenario's own default.
    pub agents: u32,
    pub cell_size_px: u32,
    pub sprite_size_px: u32,
    /// The *scenario's* seed field (must be nonzero) — unrelated to
    /// [`HarnessBuilder::seed`], which seeds the run.
    pub scenario_seed: u64,
}

impl GridSpec {
    /// Open grid, one spawn at the origin cell, one agent.
    pub fn new(width: u32, height: u32, destination: Cell) -> Self {
        Self {
            width,
            height,
            destination,
            obstacle_cells: Vec::new(),
            spawn_cells: vec![Cell { x: 0, y: 0 }],
            agents: 1,
            cell_size_px: 4,
            sprite_size_px: 30,
            scenario_seed: 1,
        }
    }

    pub fn with_obstacles(mut self, obstacle_cells: Vec<u32>) -> Self {
        self.obstacle_cells = obstacle_cells;
        self
    }

    pub fn with_spawns(mut self, spawn_cells: Vec<Cell>) -> Self {
        self.spawn_cells = spawn_cells;
        self
    }

    pub fn with_agents(mut self, agents: u32) -> Self {
        self.agents = agents;
        self
    }
}

impl From<GridSpec> for ScenarioSpec {
    fn from(g: GridSpec) -> Self {
        ScenarioSpec {
            version: INLINE_GRID_VERSION.to_string(),
            width: g.width,
            height: g.height,
            cell_size_px: g.cell_size_px,
            sprite_size_px: g.sprite_size_px,
            hard_agent_count: g.agents,
            stretch_agent_count: FIXTURE_MAX_AGENTS,
            seed: g.scenario_seed,
            destination: g.destination,
            spawn_cells: g.spawn_cells,
            // Renderer contract — fixed for every scenario family.
            atlas_count: 4,
            direction_count: 8,
            frame_count: 4,
            obstacle_cells: g.obstacle_cells,
        }
    }
}

/// Fluent harness construction: source + agent count + seed.
#[derive(Debug, Clone)]
pub struct HarnessBuilder {
    source: ScenarioSource,
    agents: Option<u32>,
    seed: u64,
}

impl HarnessBuilder {
    /// Override the scenario's agent count (defaults to its hard count).
    pub fn agents(mut self, agents: u32) -> Self {
        self.agents = Some(agents);
        self
    }

    /// Seed the run. `0` keeps the scenario's canonical spawn order.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    pub fn build(self) -> Result<Harness, HarnessError> {
        let scenario = self.source.load()?;
        let mut runtime = Runtime::from_scenario(scenario, self.agents)?;
        seed_spawn_placement(&mut runtime, self.seed);
        Ok(Harness {
            runtime,
            seed: self.seed,
            ticks_applied: 0,
        })
    }
}

/// Deterministically redistribute agents across the scenario's spawn cells.
///
/// Seed `0` is the identity: the runtime keeps the canonical round-robin
/// placement, so an unseeded harness is bit-identical to building [`Runtime`]
/// directly.
///
/// Only start positions move, and only ever to a validated spawn-cell centre.
/// The atlas/direction/frame assignment locked by the simulation contract is
/// untouched, as is the recycle cursor. `dir` is therefore left at its
/// construction value and may briefly not face the new spawn — the first tick
/// overwrites it from the field vector, exactly as on the unseeded path.
fn seed_spawn_placement(runtime: &mut Runtime, seed: u64) {
    if seed == 0 {
        return;
    }
    let spawns: Vec<(f32, f32)> = runtime
        .scenario()
        .spawn_cells()
        .iter()
        .map(|c| (c.x as f32 + 0.5, c.y as f32 + 0.5))
        .collect();
    if spawns.is_empty() {
        return;
    }

    let count = runtime.agent_count();
    let mut rng = SplitMix64::new(seed).derive("spawn-placement");
    for i in 0..count {
        let pick = rng.next_bounded(spawns.len() as u64) as usize;
        let (x, y) = spawns[pick];
        runtime.sim_mut().set_position(i, x, y);
    }
}

/// A seeded, headless, clock-free game-system runner.
#[derive(Debug)]
pub struct Harness {
    runtime: Runtime,
    seed: u64,
    ticks_applied: u64,
}

impl Harness {
    /// Build from any source.
    pub fn builder(source: ScenarioSource) -> HarnessBuilder {
        HarnessBuilder {
            source,
            agents: None,
            seed: 0,
        }
    }

    /// The real 50k phase-0 workload — for slower system-level tests.
    pub fn gate_scene() -> HarnessBuilder {
        Self::builder(ScenarioSource::GateScene)
    }

    /// A small tracked fixture — for fast tests.
    pub fn fixture(name: impl Into<String>) -> HarnessBuilder {
        Self::builder(ScenarioSource::fixture(name))
    }

    /// A synthetic in-memory grid — for unit-scale tests.
    pub fn grid(spec: impl Into<ScenarioSpec>) -> HarnessBuilder {
        Self::builder(ScenarioSource::grid(spec))
    }

    // --- run control -------------------------------------------------------

    /// Advance up to `ticks` ticks; returns how many were actually applied
    /// (fewer only when the harness is paused).
    pub fn step(&mut self, ticks: u64) -> u64 {
        let before = self.runtime.tick_index();
        let mut applied = 0;
        for _ in 0..ticks {
            if !self.runtime.tick_only() {
                break;
            }
            applied += 1;
        }
        // The returned count is observed, never predicted.
        debug_assert_eq!(self.runtime.tick_index(), before + applied);
        self.ticks_applied += applied;
        applied
    }

    /// Advance exactly `ticks` ticks, or panic. Use this whenever a test's
    /// meaning depends on the tick count, so a silently short run cannot be
    /// mistaken for a passing assertion.
    pub fn step_exact(&mut self, ticks: u64) {
        let applied = self.step(ticks);
        assert_eq!(
            applied,
            ticks,
            "requested {ticks} ticks but applied {applied} (paused={})",
            self.runtime.paused()
        );
    }

    /// Run one full frame including instance packing, and return its output.
    /// CPU-only: no device is required.
    ///
    /// Unlike [`Self::step`] this goes through [`Runtime::tick_and_render`],
    /// which samples [`std::time::Instant`] to fill `FrameStats`. Timings never
    /// reach the state hash, but prefer `step` wherever a hash is compared
    /// across timing-variable runs.
    pub fn render_frame(&mut self) -> crate::runtime::FrameOutput<'_> {
        let before = self.runtime.tick_index();
        let applied = u64::from(!self.runtime.paused());
        self.ticks_applied += applied;
        let out = self.runtime.tick_and_render();
        debug_assert_eq!(out.tick_index, before + applied);
        out
    }

    /// Pack the *current* state into draw groups without advancing the sim.
    pub fn render_frame_groups(&mut self) -> &[DrawGroup; ATLAS_COUNT] {
        self.runtime.pack_groups();
        self.runtime.draw_groups()
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.runtime.set_paused(paused);
    }

    pub fn paused(&self) -> bool {
        self.runtime.paused()
    }

    // --- state -------------------------------------------------------------

    /// Seed this harness was built with.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// A fresh seed stream for the named subsystem.
    ///
    /// The label is mandatory so two systems cannot accidentally share one
    /// stream: `rng("camera")` and `rng("selection")` are independent, and each
    /// is re-derivable from the harness seed alone.
    pub fn rng(&self, label: &str) -> SplitMix64 {
        SplitMix64::new(self.seed).derive(label)
    }

    /// Ticks the sim has advanced.
    pub fn tick_index(&self) -> u64 {
        self.runtime.tick_index()
    }

    /// Ticks this harness applied (equals [`Self::tick_index`] for a harness
    /// that was never externally stepped).
    pub fn ticks_applied(&self) -> u64 {
        self.ticks_applied
    }

    /// Live agent population.
    pub fn alive_count(&self) -> usize {
        self.runtime.agent_count()
    }

    /// Alias of [`Self::alive_count`] matching the runtime's vocabulary.
    pub fn agent_count(&self) -> usize {
        self.runtime.agent_count()
    }

    /// Arrivals recycled to a spawn cell so far.
    pub fn recycled_count(&self) -> u64 {
        self.runtime.sim().recycle_count()
    }

    /// Total spawn events: initial seeding plus every recycle.
    pub fn spawned_count(&self) -> u64 {
        self.runtime.sim().spawn_count()
    }

    /// Read-only SoA positions and animation channels.
    pub fn agents(&self) -> AgentsView<'_> {
        self.runtime.agents()
    }

    /// Exact same-host state digest.
    pub fn state_hash(&self) -> [u8; 32] {
        self.runtime.state_hash()
    }

    /// [`Self::state_hash`] as lowercase hex, for messages and cross-process
    /// comparison.
    pub fn state_hash_hex(&self) -> String {
        hex::encode(self.state_hash())
    }

    /// Cross-platform digest: positions quantized to 1/256 cell.
    pub fn quantized_state_hash(&self) -> [u8; 32] {
        self.runtime.sim().quantized_state_hash()
    }

    // --- composition -------------------------------------------------------

    pub fn scenario(&self) -> &Scenario {
        self.runtime.scenario()
    }

    pub fn flow_field(&self) -> &FlowField {
        self.runtime.flow_field()
    }

    pub fn sim(&self) -> &Simulation {
        self.runtime.sim()
    }

    /// Mutable sim access for tests that place an agent or override a field
    /// vector before stepping.
    pub fn sim_mut(&mut self) -> &mut Simulation {
        self.runtime.sim_mut()
    }

    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Escape hatch for systems the harness does not model yet.
    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    /// True when this harness is running the locked phase-0 workload.
    pub fn is_gate_scene(&self) -> bool {
        self.scenario().version() == TECHNICAL_PROTOTYPE_V1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_grid_needs_no_asset() {
        let spec = GridSpec::new(8, 8, Cell { x: 7, y: 7 }).with_agents(4);
        let h = Harness::grid(spec).build().expect("inline grid harness");
        assert_eq!(h.alive_count(), 4);
        assert!(!h.is_gate_scene());
    }

    #[test]
    fn inline_grid_is_bounded_by_the_fixture_caps() {
        // The synthetic source is not an escape hatch around the caps that keep
        // fixtures small.
        let spec = GridSpec::new(8, 8, Cell { x: 7, y: 7 }).with_agents(FIXTURE_MAX_AGENTS + 1);
        assert!(matches!(
            Harness::grid(spec).build(),
            Err(HarnessError::Scenario(ScenarioError::InvalidDimension(_)))
        ));
    }

    #[test]
    fn seed_zero_is_the_identity() {
        let spec = GridSpec::new(8, 8, Cell { x: 7, y: 7 })
            .with_spawns(vec![Cell { x: 0, y: 0 }, Cell { x: 0, y: 7 }])
            .with_agents(8);
        let a = Harness::grid(spec.clone()).seed(0).build().expect("a");
        let b = Harness::grid(spec).build().expect("b");
        assert_eq!(a.state_hash(), b.state_hash());
    }
}
