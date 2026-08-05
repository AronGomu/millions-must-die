//! Versioned benchmark JSON report + absolute verdict.

use serde::{Deserialize, Serialize};

use crate::alloc_guard::ALLOC_VISIBILITY_NOTE;

use super::policy::BenchPolicy;
use super::stats::{TrialAggregate, TrialPercentiles};

/// Schema id written into every report.
pub const REPORT_SCHEMA_VERSION: &str = "benchmark-report-v2";

/// Process exit codes (CLI contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum BenchExitCode {
    Pass = 0,
    Fail = 1,
    Inconclusive = 2,
    Error = 3,
}

impl BenchExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// Overall / per-count verdict label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictStatus {
    Pass,
    Fail,
    Inconclusive,
    /// Non-blocking count recorded without absolute gate.
    Recorded,
    Error,
}

impl VerdictStatus {
    pub fn to_exit_code(self) -> BenchExitCode {
        match self {
            VerdictStatus::Pass | VerdictStatus::Recorded => BenchExitCode::Pass,
            VerdictStatus::Fail => BenchExitCode::Fail,
            VerdictStatus::Inconclusive => BenchExitCode::Inconclusive,
            VerdictStatus::Error => BenchExitCode::Error,
        }
    }
}

/// Scenario + atlas identity for one benchmark workload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadIdentity {
    pub scenario_version: String,
    pub scenario_sha256: String,
    pub atlas_manifest_sha256: String,
}

impl WorkloadIdentity {
    pub fn new(
        scenario_version: impl Into<String>,
        scenario_sha256: impl Into<String>,
        atlas_manifest_sha256: impl Into<String>,
    ) -> Self {
        Self {
            scenario_version: scenario_version.into(),
            scenario_sha256: scenario_sha256.into(),
            atlas_manifest_sha256: atlas_manifest_sha256.into(),
        }
    }
}

/// Source / scenario / shader / backend pins embedded in report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportManifests {
    pub scenario_version: String,
    pub scenario_sha256: String,
    pub atlas_manifest_sha256: String,
    pub backend: String,
    pub adapter: String,
    pub shader_manifest_version: u32,
    pub engine_version: String,
    pub policy_id: String,
    pub frames_in_flight: usize,
    /// Honest label: fence latency is queue proxy, not GPU execution time.
    pub gpu_queue_latency_note: String,
    /// Honest label: only Rust global allocator is counted.
    pub project_alloc_visibility_note: String,
}

impl ReportManifests {
    pub fn new(
        workload: WorkloadIdentity,
        backend: impl Into<String>,
        adapter: impl Into<String>,
        shader_manifest_version: u32,
        engine_version: impl Into<String>,
        policy: &BenchPolicy,
    ) -> Self {
        Self {
            scenario_version: workload.scenario_version,
            scenario_sha256: workload.scenario_sha256,
            atlas_manifest_sha256: workload.atlas_manifest_sha256,
            backend: backend.into(),
            adapter: adapter.into(),
            shader_manifest_version,
            engine_version: engine_version.into(),
            policy_id: policy.policy_id.to_string(),
            frames_in_flight: policy.frames_in_flight,
            gpu_queue_latency_note:
                "async submit-to-fence completion proxy; not true GPU execution time".into(),
            project_alloc_visibility_note: ALLOC_VISIBILITY_NOTE.into(),
        }
    }
}

/// One trial inside a scale point.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrialReport {
    pub index: u32,
    pub sample_count: usize,
    pub p95_frame_service_ms: f64,
    pub p99_frame_service_ms: f64,
    pub median_sim_ms: f64,
    pub median_upload_ms: f64,
    pub median_gpu_queue_latency_ms: f64,
}

/// One agent-count scale point.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScaleResult {
    pub agent_count: u32,
    pub blocking: bool,
    pub stretch: bool,
    pub trials: Vec<TrialReport>,
    pub median_p95_frame_service_ms: f64,
    pub median_p99_frame_service_ms: f64,
    pub nmad_p95: f64,
    pub nmad_p99: f64,
    pub final_drain_ms: f64,
    pub submitted_frames: u64,
    pub completed_frames: u64,
    pub max_in_flight: usize,
    /// Sum of project Rust global allocations across measured trial frames.
    pub project_rust_alloc_count: u64,
    pub verdict: VerdictStatus,
    pub verdict_reason: String,
}

/// Absolute gate thresholds recorded for audit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AbsoluteGate {
    pub agent_count: u32,
    pub median_p95_limit_ms: f64,
    pub median_p99_limit_ms: f64,
    pub nmad_limit: f64,
    /// Relative gates disabled until T26.
    pub relative_gates_enabled: bool,
}

/// Top-level versioned report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkReport {
    pub schema_version: String,
    pub manifests: ReportManifests,
    pub absolute_gate: AbsoluteGate,
    pub scale_results: Vec<ScaleResult>,
    /// Verdict from blocking 50k only.
    pub verdict: VerdictStatus,
    pub verdict_reason: String,
}

impl BenchmarkReport {
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn exit_code(&self) -> BenchExitCode {
        self.verdict.to_exit_code()
    }
}

/// Build trial report row from samples.
pub fn trial_report(
    index: u32,
    tp: &TrialPercentiles,
    median_sim_ms: f64,
    median_upload_ms: f64,
    median_gpu_queue_latency_ms: f64,
) -> TrialReport {
    TrialReport {
        index,
        sample_count: tp.sample_count,
        p95_frame_service_ms: tp.p95_ms,
        p99_frame_service_ms: tp.p99_ms,
        median_sim_ms,
        median_upload_ms,
        median_gpu_queue_latency_ms,
    }
}

/// Absolute verdict for one scale point.
pub fn scale_verdict(
    policy: &BenchPolicy,
    count: u32,
    agg: &TrialAggregate,
    submitted: u64,
    completed: u64,
    max_in_flight: usize,
    project_rust_alloc_count: u64,
) -> (VerdictStatus, String) {
    if submitted != completed {
        return (
            VerdictStatus::Error,
            format!("drain mismatch submitted={submitted} completed={completed}"),
        );
    }
    if max_in_flight > policy.frames_in_flight {
        return (
            VerdictStatus::Error,
            format!(
                "frames in flight {} exceeded cap {}",
                max_in_flight, policy.frames_in_flight
            ),
        );
    }
    if project_rust_alloc_count > 0 {
        return (
            VerdictStatus::Fail,
            format!(
                "project Rust frame allocations = {project_rust_alloc_count} (limit 0); {ALLOC_VISIBILITY_NOTE}"
            ),
        );
    }

    if !policy.is_blocking_count(count) {
        // Non-blocking: still surface noise as recorded (never fails process alone).
        let reason = if agg.noisy {
            format!(
                "non-blocking count {count}; noisy nmad_p95={:.4} nmad_p99={:.4}",
                agg.nmad_p95, agg.nmad_p99
            )
        } else {
            format!(
                "non-blocking count {count}; median_p95={:.3} median_p99={:.3}",
                agg.median_p95_ms, agg.median_p99_ms
            )
        };
        return (VerdictStatus::Recorded, reason);
    }

    // Blocking 50k absolute gate.
    if agg.noisy {
        return (
            VerdictStatus::Inconclusive,
            format!(
                "50k normalized MAD over 3% (nmad_p95={:.4} nmad_p99={:.4})",
                agg.nmad_p95, agg.nmad_p99
            ),
        );
    }
    if agg.median_p95_ms > policy.p95_limit_ms() {
        return (
            VerdictStatus::Fail,
            format!(
                "50k median p95 {:.3} ms > {:.2} ms",
                agg.median_p95_ms,
                policy.p95_limit_ms()
            ),
        );
    }
    if agg.median_p99_ms > policy.p99_limit_ms() {
        return (
            VerdictStatus::Fail,
            format!(
                "50k median p99 {:.3} ms > {:.2} ms",
                agg.median_p99_ms,
                policy.p99_limit_ms()
            ),
        );
    }
    (
        VerdictStatus::Pass,
        format!(
            "50k median p95 {:.3} ms <= {:.2}; p99 {:.3} ms <= {:.2}",
            agg.median_p95_ms,
            policy.p95_limit_ms(),
            agg.median_p99_ms,
            policy.p99_limit_ms()
        ),
    )
}

/// Compose full report; overall verdict = blocking scale result only.
pub fn build_report(
    policy: &BenchPolicy,
    manifests: ReportManifests,
    scale_results: Vec<ScaleResult>,
) -> BenchmarkReport {
    let (verdict, verdict_reason) = scale_results
        .iter()
        .find(|s| s.blocking)
        .map(|s| (s.verdict, s.verdict_reason.clone()))
        .unwrap_or((
            VerdictStatus::Error,
            "missing blocking 50k scale result".into(),
        ));

    BenchmarkReport {
        schema_version: REPORT_SCHEMA_VERSION.into(),
        manifests,
        absolute_gate: AbsoluteGate {
            agent_count: policy.gate_count,
            median_p95_limit_ms: policy.p95_limit_ms(),
            median_p99_limit_ms: policy.p99_limit_ms(),
            nmad_limit: policy.nmad_limit(),
            relative_gates_enabled: false,
        },
        scale_results,
        verdict,
        verdict_reason,
    }
}

/// Build a ScaleResult from aggregates + trial rows.
// Report row mirrors the JSON schema field-for-field; grouping into a struct
// would only rename the same nine values (mechanical clippy allowance).
#[allow(clippy::too_many_arguments)]
pub fn build_scale_result(
    policy: &BenchPolicy,
    agent_count: u32,
    trials: Vec<TrialReport>,
    agg: &TrialAggregate,
    final_drain_ms: f64,
    submitted_frames: u64,
    completed_frames: u64,
    max_in_flight: usize,
    project_rust_alloc_count: u64,
) -> ScaleResult {
    let (verdict, verdict_reason) = scale_verdict(
        policy,
        agent_count,
        agg,
        submitted_frames,
        completed_frames,
        max_in_flight,
        project_rust_alloc_count,
    );
    ScaleResult {
        agent_count,
        blocking: policy.is_blocking_count(agent_count),
        stretch: policy.is_stretch_count(agent_count),
        trials,
        median_p95_frame_service_ms: agg.median_p95_ms,
        median_p99_frame_service_ms: agg.median_p99_ms,
        nmad_p95: agg.nmad_p95,
        nmad_p99: agg.nmad_p99,
        final_drain_ms,
        submitted_frames,
        completed_frames,
        max_in_flight,
        project_rust_alloc_count,
        verdict,
        verdict_reason,
    }
}
