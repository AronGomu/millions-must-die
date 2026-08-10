//! Deterministic headless RTS world runner — the RTS sibling of [`super::Harness`].

use std::path::PathBuf;

use super::HarnessError;
use super::rng::SplitMix64;
use crate::rts::{EntityId, EntityKind, RtsWorld};
use crate::scenario::{Scenario, ScenarioSpec};

/// Where an [`RtsHarness`] gets its scenario.
#[derive(Debug, Clone)]
enum RtsSource {
    /// The tracked phase-1 scene. Hash-verified.
    Scene,
    /// An arbitrary scenario file. Hash-verified via its `.sha256` sidecar.
    Path(PathBuf),
    /// An in-memory spec, same validator, no hash.
    Spec(Box<ScenarioSpec>),
}

impl RtsSource {
    fn load(&self) -> Result<Scenario, crate::scenario::ScenarioError> {
        match self {
            Self::Scene => Scenario::load_verified(super::rts_scene_path()),
            Self::Path(path) => Scenario::load_verified(path),
            Self::Spec(spec) => Scenario::from_spec((**spec).clone()),
        }
    }
}

/// Fluent [`RtsHarness`] construction: source + seed.
#[derive(Debug, Clone)]
pub struct RtsHarnessBuilder {
    source: RtsSource,
    seed: u64,
}

impl RtsHarnessBuilder {
    /// Seed the harness's independent RNG streams (see [`RtsHarness::rng`]).
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    pub fn build(self) -> Result<RtsHarness, HarnessError> {
        let scenario = self.source.load()?;
        let world = RtsWorld::from_scenario(scenario)?;
        Ok(RtsHarness {
            world,
            seed: self.seed,
        })
    }
}

/// A seeded, clock-free RTS world runner — the RTS sibling of [`super::Harness`].
#[derive(Debug)]
pub struct RtsHarness {
    world: RtsWorld,
    seed: u64,
}

impl RtsHarness {
    /// The tracked phase-1 scene.
    pub fn scene() -> RtsHarnessBuilder {
        RtsHarnessBuilder {
            source: RtsSource::Scene,
            seed: 0,
        }
    }

    /// An arbitrary scenario file, hash-verified.
    pub fn path(path: impl Into<PathBuf>) -> RtsHarnessBuilder {
        RtsHarnessBuilder {
            source: RtsSource::Path(path.into()),
            seed: 0,
        }
    }

    /// An in-memory spec, same validator, no hash.
    pub fn spec(spec: ScenarioSpec) -> RtsHarnessBuilder {
        RtsHarnessBuilder {
            source: RtsSource::Spec(Box::new(spec)),
            seed: 0,
        }
    }

    /// Advance exactly `ticks` ticks.
    pub fn step_exact(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.world.tick();
        }
    }

    pub fn tick_index(&self) -> u64 {
        self.world.tick_index()
    }

    pub fn world(&self) -> &RtsWorld {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut RtsWorld {
        &mut self.world
    }

    pub fn state_hash(&self) -> [u8; 32] {
        self.world.state_hash()
    }

    pub fn state_hash_hex(&self) -> String {
        hex::encode(self.state_hash())
    }

    /// A fresh, independent seed stream for the named subsystem.
    pub fn rng(&self, label: &str) -> SplitMix64 {
        SplitMix64::new(self.seed).derive(label)
    }

    /// Live entities of one kind, ascending slot order.
    pub fn ids_of_kind(&self, kind: EntityKind) -> Vec<EntityId> {
        let mut slots = Vec::new();
        self.world.entities().collect_live(&mut slots);
        slots
            .into_iter()
            .filter(|&slot| self.world.entities().kind(slot) == kind)
            .filter_map(|slot| self.world.entities().id_at(slot))
            .collect()
    }
}
