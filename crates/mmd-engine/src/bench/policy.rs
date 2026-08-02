//! Locked production bench policy + injectable short test policy.

use std::time::Duration;

use super::stats::{GATE_P95_MS, GATE_P99_MS, NMAD_LIMIT};

/// Scale curve agent counts (normative).
pub const SCALE_COUNTS: [u32; 4] = [1_000, 10_000, 50_000, 100_000];

/// Only this count controls absolute pass/fail verdict.
pub const GATE_AGENT_COUNT: u32 = 50_000;

/// Named stretch (recorded, never blocks alone).
pub const STRETCH_AGENT_COUNT: u32 = 100_000;

/// Env var enabling short smoke policy for CLI path.
pub const TEST_POLICY_ENV: &str = "MMD_BENCH_TEST_POLICY";

/// Full locked measurement policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchPolicy {
    /// Agent counts to measure in order.
    pub scale_counts: Vec<u32>,
    /// Count whose absolute gate controls process verdict.
    pub gate_count: u32,
    /// Named stretch count (must appear in scale_counts).
    pub stretch_count: u32,
    pub warmup: Duration,
    pub trial_count: u32,
    pub trial_duration: Duration,
    /// Min frames per warmup/trial phase (0 = duration-only; test policy uses >0).
    pub min_frames_warmup: u32,
    pub min_frames_trial: u32,
    pub frames_in_flight: usize,
    /// Policy id string embedded in report.
    pub policy_id: &'static str,
}

impl BenchPolicy {
    /// Production locked policy (hours-long full curve).
    pub fn production() -> Self {
        Self {
            scale_counts: SCALE_COUNTS.to_vec(),
            gate_count: GATE_AGENT_COUNT,
            stretch_count: STRETCH_AGENT_COUNT,
            warmup: Duration::from_secs(10),
            trial_count: 7,
            trial_duration: Duration::from_secs(60),
            min_frames_warmup: 0,
            min_frames_trial: 0,
            frames_in_flight: 2,
            policy_id: "production-v1",
        }
    }

    /// Injectable short policy for unit/CLI smoke only.
    ///
    /// Keeps same scale curve + gate semantics; shrinks wall time.
    /// Min frame floors guarantee samples even when one frame > duration.
    pub fn test_short() -> Self {
        Self {
            scale_counts: SCALE_COUNTS.to_vec(),
            gate_count: GATE_AGENT_COUNT,
            stretch_count: STRETCH_AGENT_COUNT,
            warmup: Duration::from_millis(1),
            trial_count: 7,
            trial_duration: Duration::from_millis(1),
            min_frames_warmup: 2,
            min_frames_trial: 8,
            frames_in_flight: 2,
            policy_id: "test-short-v1",
        }
    }

    /// Resolve CLI/env override. `force_test` from `--test-policy`.
    pub fn resolve(force_test: bool) -> Self {
        if force_test || std::env::var_os(TEST_POLICY_ENV).is_some() {
            Self::test_short()
        } else {
            Self::production()
        }
    }

    pub fn is_blocking_count(&self, count: u32) -> bool {
        count == self.gate_count
    }

    pub fn is_stretch_count(&self, count: u32) -> bool {
        count == self.stretch_count
    }

    pub fn p95_limit_ms(&self) -> f64 {
        GATE_P95_MS
    }

    pub fn p99_limit_ms(&self) -> f64 {
        GATE_P99_MS
    }

    pub fn nmad_limit(&self) -> f64 {
        NMAD_LIMIT
    }
}
