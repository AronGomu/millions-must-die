//! T27 release-proof manifest + validator (Linux-verified scope).
//!
//! Encodes the Hardware deferral policy honestly: the only claimable phase-0
//! statuses today are `linux-verified-deferred-hw` (Linux gates pass; native
//! Windows/macOS matrix deferred) or `failed`. `full-pass` stays rejected
//! while any lane is deferred. Deferred lanes must be explicitly recorded —
//! silently absent lanes reject the proof.

use std::path::Path;

use mmd_engine::bench::{BenchPolicy, BenchmarkReport, VerdictStatus};
use serde::{Deserialize, Serialize};

/// Schema id for `lab/releases/*.json`.
pub const RELEASE_PROOF_SCHEMA: &str = "release-proof-v1";
/// Honest deferral-scoped pass status (the ceiling under the 2026-08-05 policy).
pub const STATUS_LINUX_VERIFIED: &str = "linux-verified-deferred-hw";
/// Honest phase failure record (absolute gate missed).
pub const STATUS_FAILED: &str = "failed";
/// Full 3-OS proof; unavailable while any lane is deferred-hw.
pub const STATUS_FULL_PASS: &str = "full-pass";
/// Exact claim sentence the results doc and proof must agree on.
pub const LINUX_VERIFIED_CLAIM: &str = "Linux-verified; native cross-platform matrix deferred";

/// Lane states.
pub const LANE_VERIFIED: &str = "verified";
pub const LANE_DEFERRED_HW: &str = "deferred-hw";
pub const LANE_FAILED: &str = "failed";

/// Profiler capture states.
pub const CAPTURE_CAPTURED: &str = "captured";
pub const CAPTURE_PROCEDURE_DOCUMENTED: &str = "procedure-documented";
pub const CAPTURE_DEFERRED_HW: &str = "deferred-hw";

/// Every lane the phase-0 contract defines must appear in the proof,
/// verified or explicitly deferred — never silently absent.
pub const REQUIRED_LANES: [&str; 3] = ["linux-vulkan", "windows-d3d12", "macos-metal"];

/// One agent-count row copied from the bench report (summary only; raw
/// trials stay in the referenced evidence file).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReleaseScaleResult {
    pub agent_count: u32,
    pub blocking: bool,
    pub median_p95_frame_service_ms: f64,
    pub median_p99_frame_service_ms: f64,
    /// Noise evidence: a noisy 50k run must stay visible in the proof.
    pub nmad_p95: f64,
    pub nmad_p99: f64,
    pub verdict: VerdictStatus,
}

/// Absolute gate thresholds + verdict recorded for audit (no silent changes).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReleaseAbsoluteGate {
    pub agent_count: u32,
    pub median_p95_limit_ms: f64,
    pub median_p99_limit_ms: f64,
    pub nmad_limit: f64,
    pub verdict: VerdictStatus,
    pub reason: String,
}

/// Relative gate state. Must stay disabled until reviewed real-hardware
/// baselines exist (T25/T26 rule).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelativeGates {
    pub enabled: bool,
    pub reason: String,
}

/// Backend profiler evidence for one lane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfilerCapture {
    /// captured | procedure-documented | deferred-hw
    pub state: String,
    /// Doc/evidence pointer; never empty (deferred lanes point at the policy).
    pub reference: String,
}

/// One platform lane, verified or explicitly deferred.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LaneProof {
    pub lane_id: String,
    pub platform: String,
    pub backend: String,
    /// verified | deferred-hw | failed
    pub state: String,
    /// Why the lane is in this state (e.g. deferral policy pointer).
    pub reason: String,
    pub profiler_capture: ProfilerCapture,
}

/// Pointer to the committed raw bench report backing the numbers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchEvidenceRef {
    pub path: String,
    pub sha256: String,
    pub policy_id: String,
    pub backend: String,
    pub adapter: String,
}

/// Top-level release proof (`release-proof-v1`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReleaseProof {
    pub schema_version: String,
    pub release_id: String,
    /// linux-verified-deferred-hw | failed | full-pass
    pub status: String,
    pub claim: String,
    /// Commit the bench binary + evidence were produced from.
    pub candidate_commit: String,
    pub scale_results: Vec<ReleaseScaleResult>,
    pub absolute_gate: ReleaseAbsoluteGate,
    pub relative_gates: RelativeGates,
    pub lanes: Vec<LaneProof>,
    pub bench_evidence: BenchEvidenceRef,
    /// Ordinary reports keep 90 days; the release proof keeps forever.
    pub retention: String,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl ReleaseProof {
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Validator outcome for a structurally honest proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseVerdict {
    /// Linux gates pass; Windows/macOS lanes explicitly deferred.
    LinuxVerifiedDeferredHw,
    /// Absolute gate missed and the proof records the failure honestly.
    PhaseFailed,
}

/// Freeze a release proof from a real bench report.
///
/// Status derives from the report verdict — a failed 50k gate freezes an
/// honest `failed` proof, never a massaged pass.
pub fn freeze_release_proof(
    report: &BenchmarkReport,
    release_id: &str,
    candidate_commit: &str,
    bench_evidence_path: &str,
    bench_evidence_sha256: &str,
    notes: Vec<String>,
) -> ReleaseProof {
    let status = if report.verdict == VerdictStatus::Pass {
        STATUS_LINUX_VERIFIED
    } else {
        STATUS_FAILED
    };
    let claim = if status == STATUS_LINUX_VERIFIED {
        LINUX_VERIFIED_CLAIM.to_string()
    } else {
        format!("phase 0 failed on Linux: {}", report.verdict_reason)
    };
    ReleaseProof {
        schema_version: RELEASE_PROOF_SCHEMA.into(),
        release_id: release_id.into(),
        status: status.into(),
        claim,
        // Lowercase: the JSON schema pins ^[0-9a-f]{7,40}$.
        candidate_commit: candidate_commit.to_ascii_lowercase(),
        scale_results: report
            .scale_results
            .iter()
            .map(|s| ReleaseScaleResult {
                agent_count: s.agent_count,
                blocking: s.blocking,
                median_p95_frame_service_ms: s.median_p95_frame_service_ms,
                median_p99_frame_service_ms: s.median_p99_frame_service_ms,
                nmad_p95: s.nmad_p95,
                nmad_p99: s.nmad_p99,
                verdict: s.verdict,
            })
            .collect(),
        absolute_gate: ReleaseAbsoluteGate {
            agent_count: report.absolute_gate.agent_count,
            median_p95_limit_ms: report.absolute_gate.median_p95_limit_ms,
            median_p99_limit_ms: report.absolute_gate.median_p99_limit_ms,
            nmad_limit: report.absolute_gate.nmad_limit,
            verdict: report.verdict,
            reason: report.verdict_reason.clone(),
        },
        relative_gates: RelativeGates {
            enabled: false,
            reason: "no enabled baselines; synthetic pilot values stay disabled pending \
                     real-hardware pilot + owner review (T25/T26 rule)"
                .into(),
        },
        lanes: default_deferral_lanes(),
        bench_evidence: BenchEvidenceRef {
            path: bench_evidence_path.into(),
            sha256: bench_evidence_sha256.into(),
            policy_id: report.manifests.policy_id.clone(),
            backend: report.manifests.backend.clone(),
            adapter: report.manifests.adapter.clone(),
        },
        retention: "forever".into(),
        notes,
    }
}

/// Phase-0 lane set under the 2026-08-05 Hardware deferral policy.
pub fn default_deferral_lanes() -> Vec<LaneProof> {
    vec![
        LaneProof {
            lane_id: "linux-vulkan".into(),
            platform: "linux-x86_64".into(),
            backend: "vulkan".into(),
            state: LANE_VERIFIED.into(),
            reason: "real Vulkan bench on Linux dev GPU (not the RX 6400 ref target)".into(),
            profiler_capture: ProfilerCapture {
                state: CAPTURE_PROCEDURE_DOCUMENTED.into(),
                reference: "docs/lab/gpu-profiling.md".into(),
            },
        },
        LaneProof {
            lane_id: "windows-d3d12".into(),
            platform: "windows-x86_64".into(),
            backend: "d3d12".into(),
            state: LANE_DEFERRED_HW.into(),
            reason: "Hardware deferral policy 2026-08-05: no physical Windows ref PC".into(),
            profiler_capture: ProfilerCapture {
                state: CAPTURE_DEFERRED_HW.into(),
                reference: ".tmp/IMPLEMENTATION_PLAN_technical_prototype.md#hardware-deferral-policy-2026-08-05-user-directed".into(),
            },
        },
        LaneProof {
            lane_id: "macos-metal".into(),
            platform: "macos-arm64".into(),
            backend: "metal".into(),
            state: LANE_DEFERRED_HW.into(),
            reason: "Hardware deferral policy 2026-08-05: no M4 Mac / MDM lab".into(),
            profiler_capture: ProfilerCapture {
                state: CAPTURE_DEFERRED_HW.into(),
                reference: ".tmp/IMPLEMENTATION_PLAN_technical_prototype.md#hardware-deferral-policy-2026-08-05-user-directed".into(),
            },
        },
    ]
}

/// Validate a release proof against the exact gate commit + locked policy.
///
/// Rejections return every violated rule (not just the first).
pub fn validate_release_proof(
    proof: &ReleaseProof,
    expected_commit: &str,
    policy: &BenchPolicy,
) -> Result<ReleaseVerdict, Vec<String>> {
    let mut errs: Vec<String> = Vec::new();

    if proof.schema_version != RELEASE_PROOF_SCHEMA {
        errs.push(format!(
            "schema_version {} != {RELEASE_PROOF_SCHEMA}",
            proof.schema_version
        ));
    }

    // Exact-hash binding: the proof must name the gate candidate commit.
    if proof.candidate_commit.is_empty()
        || !proof.candidate_commit.eq_ignore_ascii_case(expected_commit)
    {
        errs.push(format!(
            "candidate commit {} does not match gate commit {expected_commit}",
            proof.candidate_commit
        ));
    }

    // Complete scale curve — every locked count must be recorded exactly once
    // (duplicate rows would let a forged first-match row drive the verdict).
    for count in &policy.scale_counts {
        match proof
            .scale_results
            .iter()
            .filter(|s| s.agent_count == *count)
            .count()
        {
            0 => errs.push(format!("missing scale count {count}")),
            1 => {}
            n => errs.push(format!("duplicate scale count {count} ({n} rows)")),
        }
    }

    // Keep-forever guarantee + non-empty claim are enforced, not just schema'd.
    if proof.retention != "forever" {
        errs.push(format!(
            "retention {:?} must be \"forever\" for a release proof",
            proof.retention
        ));
    }
    if proof.claim.trim().is_empty() {
        errs.push("claim must not be empty".into());
    }

    // Locked thresholds — no silent loosening/tightening.
    if proof.absolute_gate.agent_count != policy.gate_count {
        errs.push(format!(
            "gate count {} != locked {}",
            proof.absolute_gate.agent_count, policy.gate_count
        ));
    }
    if proof.absolute_gate.median_p95_limit_ms != policy.p95_limit_ms()
        || proof.absolute_gate.median_p99_limit_ms != policy.p99_limit_ms()
        || proof.absolute_gate.nmad_limit != policy.nmad_limit()
    {
        errs.push(format!(
            "threshold changed: proof p95<={} p99<={} nmad<={} vs locked p95<={} p99<={} nmad<={}",
            proof.absolute_gate.median_p95_limit_ms,
            proof.absolute_gate.median_p99_limit_ms,
            proof.absolute_gate.nmad_limit,
            policy.p95_limit_ms(),
            policy.p99_limit_ms(),
            policy.nmad_limit()
        ));
    }

    // Evidence must come from the locked production policy — a short/test
    // policy report matching by hash is still a forged-easy-pass.
    if proof.bench_evidence.policy_id != "production-v1" {
        errs.push(format!(
            "bench evidence policy {} is not production-v1",
            proof.bench_evidence.policy_id
        ));
    }

    // Relative gates stay disabled until reviewed real-hardware baselines exist.
    if proof.relative_gates.enabled {
        errs.push("relative gates enabled but no reviewed real-hardware baselines exist".into());
    }

    // Every contract lane present, verified or explicitly deferred — with a
    // profiler capture record (deferred lanes point at the deferral policy).
    for required in REQUIRED_LANES {
        match proof.lanes.iter().find(|l| l.lane_id == required) {
            None => errs.push(format!(
                "lane {required} absent: deferred lanes must be explicitly recorded, \
                 never silently missing"
            )),
            Some(lane) => {
                match lane.state.as_str() {
                    LANE_VERIFIED | LANE_DEFERRED_HW | LANE_FAILED => {}
                    other => errs.push(format!("lane {required} has unknown state {other}")),
                }
                let cap = &lane.profiler_capture;
                let state_ok = matches!(
                    cap.state.as_str(),
                    CAPTURE_CAPTURED | CAPTURE_PROCEDURE_DOCUMENTED | CAPTURE_DEFERRED_HW
                );
                if !state_ok || cap.reference.trim().is_empty() {
                    errs.push(format!(
                        "lane {required} profiler capture missing state/reference \
                         (state={:?} reference={:?})",
                        cap.state, cap.reference
                    ));
                }
                if lane.state == LANE_DEFERRED_HW && cap.state == CAPTURE_CAPTURED {
                    errs.push(format!(
                        "lane {required} claims a capture while deferred-hw"
                    ));
                }
                if lane.state == LANE_VERIFIED && cap.state == CAPTURE_DEFERRED_HW {
                    errs.push(format!(
                        "lane {required} claims verified while its profiler capture \
                         is deferred-hw"
                    ));
                }
            }
        }
    }

    // 50k absolute gate honesty: recompute the miss from recorded numbers.
    // Only decisive Pass/Fail evidence can freeze a phase status; noisy or
    // errored 50k runs demand a rerun, never a verdict.
    let gate_row = proof
        .scale_results
        .iter()
        .find(|s| s.agent_count == policy.gate_count);
    let indecisive = matches!(
        proof.absolute_gate.verdict,
        VerdictStatus::Inconclusive | VerdictStatus::Error | VerdictStatus::Recorded
    ) || matches!(
        gate_row.map(|r| r.verdict),
        Some(VerdictStatus::Inconclusive)
            | Some(VerdictStatus::Error)
            | Some(VerdictStatus::Recorded)
    ) || gate_row.is_some_and(|r| {
        // Recompute noise from recorded numbers — a forged-quiet verdict
        // label cannot hide a noisy 50k run.
        r.nmad_p95 > policy.nmad_limit() || r.nmad_p99 > policy.nmad_limit()
    });
    if indecisive {
        errs.push(
            "50k gate evidence is inconclusive/errored; rerun on a quiet host — \
             neither a verified nor a failed phase status may be frozen from it"
                .into(),
        );
    }
    let gate_missed = match gate_row {
        None => true, // missing gate row already rejected above; treat as miss
        Some(row) => {
            row.median_p95_frame_service_ms > policy.p95_limit_ms()
                || row.median_p99_frame_service_ms > policy.p99_limit_ms()
                || row.verdict != VerdictStatus::Pass
                || proof.absolute_gate.verdict != VerdictStatus::Pass
        }
    };

    let status_ok = matches!(
        proof.status.as_str(),
        STATUS_LINUX_VERIFIED | STATUS_FAILED | STATUS_FULL_PASS
    );
    if !status_ok {
        errs.push(format!("unknown status {}", proof.status));
    }

    if proof.status == STATUS_FULL_PASS {
        // Unconditional under release-proof-v1: no evidence model exists yet
        // that could bind Windows/macOS lane "verified" states to real runs,
        // so the full-confidence 3-OS claim is flatly unavailable.
        errs.push(
            "status full-pass rejected: unavailable under the Hardware deferral \
             policy (2026-08-05); release-proof-v1 cannot bind native \
             Windows/macOS lane evidence"
                .into(),
        );
    }

    if !indecisive {
        if gate_missed && proof.status != STATUS_FAILED {
            errs.push(format!(
                "50k absolute gate missed but status is {}; the phase failure must be \
                 recorded honestly as status failed",
                proof.status
            ));
        }
        if !gate_missed && proof.status == STATUS_FAILED {
            errs.push("status failed but recorded 50k numbers pass the absolute gate".into());
        }
    }
    if proof.status == STATUS_LINUX_VERIFIED && !proof.claim.contains(LINUX_VERIFIED_CLAIM) {
        errs.push(format!(
            "claim must state {LINUX_VERIFIED_CLAIM:?} verbatim (got {:?})",
            proof.claim
        ));
    }

    if !errs.is_empty() {
        return Err(errs);
    }
    if proof.status == STATUS_FAILED {
        Ok(ReleaseVerdict::PhaseFailed)
    } else {
        Ok(ReleaseVerdict::LinuxVerifiedDeferredHw)
    }
}

/// Cross-check the committed bench report file against the proof reference.
pub fn verify_bench_evidence(
    proof: &ReleaseProof,
    report_path: &Path,
    report_sha256: &str,
    report: &BenchmarkReport,
) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();
    if !proof
        .bench_evidence
        .sha256
        .eq_ignore_ascii_case(report_sha256)
    {
        errs.push(format!(
            "bench evidence sha256 mismatch: proof {} file {} ({})",
            proof.bench_evidence.sha256,
            report_sha256,
            report_path.display()
        ));
    }
    if report.schema_version != "benchmark-report-v2" {
        errs.push(format!(
            "bench report schema {} != benchmark-report-v2",
            report.schema_version
        ));
    }
    if report.manifests.policy_id != "production-v1" {
        errs.push(format!(
            "bench report policy {} is not production-v1 (short/test policy evidence rejected)",
            report.manifests.policy_id
        ));
    }
    if report.manifests.policy_id != proof.bench_evidence.policy_id {
        errs.push(format!(
            "bench evidence policy mismatch: proof {} report {}",
            proof.bench_evidence.policy_id, report.manifests.policy_id
        ));
    }
    // Report verdict must agree with the frozen status — no massaged proofs.
    let status_consistent = match report.verdict {
        VerdictStatus::Pass => proof.status == STATUS_LINUX_VERIFIED,
        VerdictStatus::Fail => proof.status == STATUS_FAILED,
        _ => false,
    };
    if !status_consistent {
        errs.push(format!(
            "proof status {} inconsistent with bench report verdict {:?}",
            proof.status, report.verdict
        ));
    }
    for want in &proof.scale_results {
        match report
            .scale_results
            .iter()
            .find(|s| s.agent_count == want.agent_count)
        {
            None => errs.push(format!(
                "bench report missing scale count {}",
                want.agent_count
            )),
            Some(got) => {
                let eps = 1e-9;
                if (got.median_p95_frame_service_ms - want.median_p95_frame_service_ms).abs() > eps
                    || (got.median_p99_frame_service_ms - want.median_p99_frame_service_ms).abs()
                        > eps
                    || (got.nmad_p95 - want.nmad_p95).abs() > eps
                    || (got.nmad_p99 - want.nmad_p99).abs() > eps
                    || got.verdict != want.verdict
                {
                    errs.push(format!(
                        "scale {} stats diverge from raw report",
                        want.agent_count
                    ));
                }
            }
        }
    }
    if errs.is_empty() { Ok(()) } else { Err(errs) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmd_engine::bench::{GATE_AGENT_COUNT, SCALE_COUNTS};

    fn passing_scale(count: u32) -> ReleaseScaleResult {
        ReleaseScaleResult {
            agent_count: count,
            blocking: count == GATE_AGENT_COUNT,
            median_p95_frame_service_ms: 4.0,
            median_p99_frame_service_ms: 6.0,
            nmad_p95: 0.001,
            nmad_p99: 0.001,
            verdict: VerdictStatus::Pass,
        }
    }

    fn sample_proof() -> ReleaseProof {
        let policy = BenchPolicy::production();
        ReleaseProof {
            schema_version: RELEASE_PROOF_SCHEMA.into(),
            release_id: "technical-prototype-v1".into(),
            status: STATUS_LINUX_VERIFIED.into(),
            claim: LINUX_VERIFIED_CLAIM.into(),
            candidate_commit: "f2def9e".into(),
            scale_results: SCALE_COUNTS.iter().map(|c| passing_scale(*c)).collect(),
            absolute_gate: ReleaseAbsoluteGate {
                agent_count: GATE_AGENT_COUNT,
                median_p95_limit_ms: policy.p95_limit_ms(),
                median_p99_limit_ms: policy.p99_limit_ms(),
                nmad_limit: policy.nmad_limit(),
                verdict: VerdictStatus::Pass,
                reason: "50k p95/p99 under limits".into(),
            },
            relative_gates: RelativeGates {
                enabled: false,
                reason: "no enabled baselines".into(),
            },
            lanes: default_deferral_lanes(),
            bench_evidence: BenchEvidenceRef {
                path: "lab/releases/evidence/linux-vulkan-bench-production-v1.json".into(),
                sha256: "ab".repeat(32),
                policy_id: "production-v1".into(),
                backend: "vulkan".into(),
                adapter: "test-adapter".into(),
            },
            retention: "forever".into(),
            notes: Vec::new(),
        }
    }

    fn errors_of(proof: &ReleaseProof, commit: &str) -> Vec<String> {
        validate_release_proof(proof, commit, &BenchPolicy::production())
            .expect_err("proof must be rejected")
    }

    #[test]
    fn valid_linux_scoped_proof_passes() {
        let proof = sample_proof();
        let v = validate_release_proof(&proof, "f2def9e", &BenchPolicy::production())
            .expect("valid proof");
        assert_eq!(v, ReleaseVerdict::LinuxVerifiedDeferredHw);
    }

    #[test]
    fn release_requires_all_scale_counts() {
        let mut proof = sample_proof();
        proof.scale_results.retain(|s| s.agent_count != 10_000);
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("scale count 10000")),
            "expected missing 10k rejection, got {errs:?}"
        );
    }

    #[test]
    fn release_requires_profiler_refs() {
        // Empty reference on a deferred lane → reject.
        let mut proof = sample_proof();
        proof
            .lanes
            .iter_mut()
            .find(|l| l.lane_id == "windows-d3d12")
            .unwrap()
            .profiler_capture
            .reference
            .clear();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter()
                .any(|e| e.contains("windows-d3d12") && e.contains("profiler")),
            "expected profiler ref rejection, got {errs:?}"
        );

        // Silently absent deferred lane → reject (must be explicitly recorded).
        let mut proof = sample_proof();
        proof.lanes.retain(|l| l.lane_id != "macos-metal");
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter()
                .any(|e| e.contains("macos-metal") && e.contains("explicit")),
            "expected explicit-deferral rejection, got {errs:?}"
        );
    }

    #[test]
    fn release_hash_matches_gate() {
        let proof = sample_proof();
        let errs = errors_of(&proof, "0123abcd");
        assert!(
            errs.iter().any(|e| e.contains("commit")),
            "expected commit mismatch rejection, got {errs:?}"
        );
    }

    #[test]
    fn failed_50k_records_phase_failure() {
        // Absolute miss + status still claiming linux-verified → dishonest, reject.
        let mut proof = sample_proof();
        for s in &mut proof.scale_results {
            if s.agent_count == GATE_AGENT_COUNT {
                s.median_p95_frame_service_ms = 20.0;
                s.verdict = VerdictStatus::Fail;
            }
        }
        proof.absolute_gate.verdict = VerdictStatus::Fail;
        proof.absolute_gate.reason = "50k median p95 20.000 ms > 16.67 ms".into();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("recorded honestly")),
            "expected honesty rejection, got {errs:?}"
        );

        // Same miss recorded as status=failed → honest phase failure record.
        proof.status = STATUS_FAILED.into();
        proof.claim = "phase 0 failed on Linux: 50k median p95 20.000 ms > 16.67 ms".into();
        let v = validate_release_proof(&proof, "f2def9e", &BenchPolicy::production())
            .expect("honest failure record accepted");
        assert_eq!(v, ReleaseVerdict::PhaseFailed);
    }

    #[test]
    fn full_pass_rejected_unconditionally() {
        // Even a proof forging every lane as verified cannot claim full-pass:
        // release-proof-v1 has no native Windows/macOS evidence model.
        let mut proof = sample_proof();
        proof.status = STATUS_FULL_PASS.into();
        for lane in &mut proof.lanes {
            lane.state = LANE_VERIFIED.into();
            lane.profiler_capture.state = CAPTURE_CAPTURED.into();
            lane.profiler_capture.reference = "forged-ref".into();
        }
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("full-pass")),
            "expected unconditional full-pass rejection, got {errs:?}"
        );
    }

    #[test]
    fn enabled_relative_gates_rejected() {
        let mut proof = sample_proof();
        proof.relative_gates.enabled = true;
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("relative")),
            "expected relative-gate rejection, got {errs:?}"
        );
    }

    #[test]
    fn inconclusive_50k_rejects_any_frozen_status() {
        // Noisy 50k evidence may freeze neither a verified nor a failed phase.
        // Each indecisive signal source must reject independently.
        for status in [STATUS_LINUX_VERIFIED, STATUS_FAILED] {
            // Source 1: only the recorded absolute gate verdict is indecisive.
            let mut proof = sample_proof();
            proof.status = status.into();
            proof.absolute_gate.verdict = VerdictStatus::Inconclusive;
            proof.absolute_gate.reason = "50k normalized MAD over 3%".into();
            let errs = errors_of(&proof, "f2def9e");
            assert!(
                errs.iter().any(|e| e.contains("inconclusive")),
                "expected gate-verdict inconclusive rejection for status {status}, got {errs:?}"
            );

            // Source 2: only the 50k scale row is indecisive (Error variant).
            let mut proof = sample_proof();
            proof.status = status.into();
            for s in &mut proof.scale_results {
                if s.agent_count == GATE_AGENT_COUNT {
                    s.verdict = VerdictStatus::Error;
                }
            }
            let errs = errors_of(&proof, "f2def9e");
            assert!(
                errs.iter().any(|e| e.contains("inconclusive")),
                "expected 50k-row error rejection for status {status}, got {errs:?}"
            );
        }
    }

    #[test]
    fn forged_quiet_noise_label_rejected() {
        // Recorded nmad above the locked limit rejects even when every
        // verdict label claims pass.
        let mut proof = sample_proof();
        for s in &mut proof.scale_results {
            if s.agent_count == GATE_AGENT_COUNT {
                s.nmad_p95 = 0.10; // > 0.03, verdict left as Pass (forged label)
            }
        }
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("inconclusive")),
            "expected recomputed-noise rejection, got {errs:?}"
        );
    }

    #[test]
    fn verified_lane_with_deferred_capture_rejected() {
        let mut proof = sample_proof();
        let linux = proof
            .lanes
            .iter_mut()
            .find(|l| l.lane_id == "linux-vulkan")
            .unwrap();
        linux.profiler_capture.state = CAPTURE_DEFERRED_HW.into();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter()
                .any(|e| e.contains("verified while its profiler capture")),
            "expected verified/deferred-capture contradiction rejection, got {errs:?}"
        );
    }

    #[test]
    fn duplicate_gate_rows_rejected() {
        let mut proof = sample_proof();
        let mut dup = passing_scale(GATE_AGENT_COUNT);
        dup.median_p95_frame_service_ms = 20.0; // hidden failing twin
        proof.scale_results.push(dup);
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter()
                .any(|e| e.contains("duplicate scale count 50000")),
            "expected duplicate-row rejection, got {errs:?}"
        );
    }

    #[test]
    fn non_forever_retention_rejected() {
        let mut proof = sample_proof();
        proof.retention = "90-days".into();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("forever")),
            "expected retention rejection, got {errs:?}"
        );
    }

    #[test]
    fn threshold_and_gate_count_locks() {
        // Each locked limit must reject independently.
        type GateMutator = fn(&mut ReleaseAbsoluteGate);
        let cases: [(&str, GateMutator); 3] = [
            ("p95", |g| g.median_p95_limit_ms = 20.0),
            ("p99", |g| g.median_p99_limit_ms = 30.0),
            ("nmad", |g| g.nmad_limit = 0.5),
        ];
        for (name, mutate) in cases {
            let mut proof = sample_proof();
            mutate(&mut proof.absolute_gate);
            let errs = errors_of(&proof, "f2def9e");
            assert!(
                errs.iter().any(|e| e.contains("threshold")),
                "expected {name} threshold rejection, got {errs:?}"
            );
        }
        let mut proof = sample_proof();
        proof.absolute_gate.agent_count = 10_000;
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("gate count")),
            "expected gate-count rejection, got {errs:?}"
        );
    }

    #[test]
    fn status_failed_with_passing_numbers_rejected() {
        // Declaring failure while the recorded numbers pass is also dishonest.
        let mut proof = sample_proof();
        proof.status = STATUS_FAILED.into();
        proof.claim = "phase 0 failed on Linux".into();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("pass the absolute gate")),
            "expected inverse-honesty rejection, got {errs:?}"
        );
    }

    #[test]
    fn claim_sentence_must_be_verbatim() {
        let mut proof = sample_proof();
        proof.claim = "Linux verified".into();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("verbatim")),
            "expected verbatim-claim rejection, got {errs:?}"
        );
    }

    #[test]
    fn deferred_lane_claiming_capture_rejected() {
        let mut proof = sample_proof();
        proof
            .lanes
            .iter_mut()
            .find(|l| l.lane_id == "windows-d3d12")
            .unwrap()
            .profiler_capture
            .state = CAPTURE_CAPTURED.into();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter()
                .any(|e| e.contains("claims a capture while deferred-hw")),
            "expected deferred-capture contradiction rejection, got {errs:?}"
        );
    }

    #[test]
    fn unknown_enum_strings_rejected() {
        let mut proof = sample_proof();
        proof.lanes[0].state = "wip".into();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("unknown state wip")),
            "expected unknown lane state rejection, got {errs:?}"
        );

        let mut proof = sample_proof();
        proof.status = "shipped".into();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("unknown status shipped")),
            "expected unknown status rejection, got {errs:?}"
        );
    }

    #[test]
    fn schema_version_mismatch_rejected() {
        let mut proof = sample_proof();
        proof.schema_version = "release-proof-v0".into();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("schema_version")),
            "expected schema_version rejection, got {errs:?}"
        );
    }

    #[test]
    fn empty_commit_rejected_even_with_empty_expectation() {
        let mut proof = sample_proof();
        proof.candidate_commit = String::new();
        let errs = errors_of(&proof, "");
        assert!(
            errs.iter().any(|e| e.contains("commit")),
            "expected empty-commit rejection, got {errs:?}"
        );
    }

    #[test]
    fn test_policy_evidence_rejected() {
        // A short/test-policy bench file is a forged-easy-pass even when
        // its hash matches the proof.
        let mut proof = sample_proof();
        proof.bench_evidence.policy_id = "test-short-v1".into();
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("production-v1")),
            "expected policy rejection, got {errs:?}"
        );
    }

    #[test]
    fn changed_thresholds_rejected() {
        let mut proof = sample_proof();
        proof.absolute_gate.median_p95_limit_ms = 20.0; // silent loosening
        let errs = errors_of(&proof, "f2def9e");
        assert!(
            errs.iter().any(|e| e.contains("threshold")),
            "expected threshold-change rejection, got {errs:?}"
        );
    }

    #[test]
    fn freeze_from_failing_report_records_failed() {
        // Build a minimal failing report via serde (report types are data).
        let smoke = serde_json::json!({
            "schema_version": "benchmark-report-v2",
            "manifests": {
                "scenario_version": "technical_prototype_v1",
                "scenario_sha256": "00",
                "atlas_manifest_sha256": "00",
                "backend": "vulkan",
                "adapter": "test",
                "shader_manifest_version": 1,
                "engine_version": "0.1.0",
                "policy_id": "production-v1",
                "frames_in_flight": 2,
                "gpu_queue_latency_note": "n",
                "project_alloc_visibility_note": "n"
            },
            "absolute_gate": {
                "agent_count": 50000,
                "median_p95_limit_ms": 16.67,
                "median_p99_limit_ms": 25.0,
                "nmad_limit": 0.03,
                "relative_gates_enabled": false
            },
            "scale_results": [],
            "verdict": "fail",
            "verdict_reason": "50k median p95 20.000 ms > 16.67 ms"
        });
        let report: BenchmarkReport = serde_json::from_value(smoke).unwrap();
        let proof = freeze_release_proof(
            &report,
            "technical-prototype-v1",
            "f2def9e",
            "lab/releases/evidence/x.json",
            &"ab".repeat(32),
            Vec::new(),
        );
        assert_eq!(proof.status, STATUS_FAILED);
        assert!(proof.claim.contains("failed"));
    }
}
