//! macOS candidate gate lane (T23). Coordinator-side adapter, thin by design
//! — a deliberate mirror of the Ubuntu (T17) / Windows (T20) lanes, not a
//! cross-platform abstraction.
//!
//! Lane order (each step gates the next):
//! 1. Host attestation — identity only (contract match; never perf truth).
//!    Non-M4 / non-arm64 models and MoltenVK-style adapters are hard rejects.
//! 2. Recovery protocol must be `ReadyForCandidate` (external EACS/ADE/MDM lane).
//! 3. Exact archive delivery; remote hash verified — mismatch stops before exec.
//! 4. Collect raw evidence (samples / readback / manifests) via transport.
//! 5. Coordinator recomputes stats + golden diff from raw evidence; the
//!    candidate-reported verdict is never trusted.
//! 6. Post-run EACS reset is required at protocol level; a reset that cannot
//!    start quarantines the host (macOS-specific: T23 "failure quarantines").
//!
//! Fixture/fake-transport scope: the native Metal smoke/scale run and the
//! real EACS/ADE/MDM reset are deferred-hw; this lane drives the exact same
//! coordinator protocol against fixture evidence. The committed golden for
//! `macos-metal` remains an honest placeholder — the fixture lane binds to a
//! committed synthetic fixture golden until native capture retires it.
//!
//! Bounded risk (accepted, documented): attestation covers host/source
//! identity only. A hostile candidate can still deny service or forge its own
//! raw output wholesale — including the adapter/os strings the golden
//! [`HostBinding`] is built from (candidate evidence, not attestation). The
//! coordinator recomputes every gate from raw evidence and forces an external
//! post-run EACS reset, but cannot cryptographically attest candidate
//! behavior — manual review + reset model remains required (see plan "Risks /
//! stop rules").

use std::path::Path;

use mmd_engine::bench::{BenchPolicy, VerdictStatus};
use mmd_engine::render::{
    HostBinding, compare_readback, decode_readback_png, load_golden_image, load_golden_manifest,
};

use crate::archive::ArchiveBlob;
use crate::macos::{
    MacosAttestVerdict, MacosHostAttestation, MacosRunnerManifest, validate_macos_attestation,
};
use crate::macos_recovery::{MacosRecoveryEvent, MacosRecoveryState};
use crate::report::HostEvidence;
use crate::ssh::AgentTransport;
use crate::verify::{HostVerdict, verify_host, worse};

/// Inputs frozen before dispatch (trusted side; never candidate-supplied).
pub struct MacosGateParams<'a> {
    pub runner_manifest: &'a MacosRunnerManifest,
    /// Observed host attestation (identity only).
    pub attestation: &'a MacosHostAttestation,
    /// Golden family dir holding `manifest.json` + captured image.
    pub golden_dir: &'a Path,
    pub policy: &'a BenchPolicy,
    /// Current workspace atlas pin (trusted-side recompute, not evidence).
    pub atlas_manifest_sha256: String,
    /// Current workspace shader pin (trusted-side recompute, not evidence).
    pub shader_canonical_sha256: String,
}

/// Coordinator-verified lane outcome.
#[derive(Debug, Clone)]
pub struct MacosGateOutcome {
    pub verdict: VerdictStatus,
    /// Non-pass reasons, in lane order.
    pub reasons: Vec<String>,
    /// Recomputed host verdict; `None` when execution never happened.
    pub host_verdict: Option<HostVerdict>,
    /// Golden diff recomputed from raw readback evidence.
    pub golden_ok: bool,
    /// Post-run external EACS reset was initiated (protocol-level).
    pub post_run_reset_started: bool,
    /// Ordered lane step log for run evidence.
    pub trail: Vec<String>,
}

impl MacosGateOutcome {
    fn refused(reason: String, trail: Vec<String>) -> Self {
        Self {
            verdict: VerdictStatus::Fail,
            reasons: vec![reason],
            host_verdict: None,
            golden_ok: false,
            post_run_reset_started: false,
            trail,
        }
    }
}

/// External post-run EACS reset hook. Real EACS/ADE/MDM control is
/// deferred-hw; the protocol event below is the trusted-side contract either
/// way.
pub trait MacosResetController {
    /// Initiate external EACS restore after a candidate run. `Ok` = protocol
    /// restore started (host leaves candidate-ready state).
    fn start_post_run_eacs(&mut self, state: &mut MacosRecoveryState) -> Result<(), String>;
}

/// Protocol-level controller: applies the external-restore event.
pub struct MacosEacsProtocolResetController;

impl MacosResetController for MacosEacsProtocolResetController {
    fn start_post_run_eacs(&mut self, state: &mut MacosRecoveryState) -> Result<(), String> {
        state.apply(MacosRecoveryEvent::StartExternalRestore {
            external_controller: true,
        })
    }
}

/// Post-run EACS is required; a reset that cannot start quarantines the host
/// at protocol level (T23) and escalates the lane verdict to `Error`.
fn require_post_run_eacs(
    reset: &mut dyn MacosResetController,
    recovery: &mut MacosRecoveryState,
    verdict: &mut VerdictStatus,
    reasons: &mut Vec<String>,
) -> bool {
    match reset.start_post_run_eacs(recovery) {
        Ok(()) => true,
        Err(e) => {
            *verdict = worse(*verdict, VerdictStatus::Error);
            reasons.push(format!("post-run eacs reset not started: {e}"));
            // Un-reset host is not reusable: force protocol quarantine.
            let _ = recovery.apply(MacosRecoveryEvent::ForceQuarantine {
                reason: format!("post-run eacs reset not started: {e}"),
            });
            false
        }
    }
}

/// Run the macOS candidate gate lane. See module docs for order + risk.
pub fn run_macos_gate(
    params: &MacosGateParams<'_>,
    recovery: &mut MacosRecoveryState,
    transport: &mut dyn AgentTransport,
    archive: &ArchiveBlob,
    readback_png: &[u8],
    reset: &mut dyn MacosResetController,
) -> MacosGateOutcome {
    let mut trail = vec!["attest-identity".to_string()];

    // 1. Host attestation — identity only; refusal means no dispatch at all.
    //    Wrong model/chip (non-M4) and MoltenVK-style adapters arrive here as
    //    MacosAttestVerdict::Reject; SSV/MDM drift as Quarantine.
    let attest = validate_macos_attestation(params.runner_manifest, params.attestation);
    if attest.verdict != MacosAttestVerdict::ReadyForRecovery {
        return MacosGateOutcome::refused(
            format!(
                "host attestation (identity only) refused dispatch: {:?}: {}",
                attest.verdict,
                attest.reasons.join("; ")
            ),
            trail,
        );
    }

    // 2. Recovery protocol gate: dispatch only from ReadyForCandidate.
    trail.push("recovery-gate".into());
    if !recovery.phase.allows_candidate_provision() {
        return MacosGateOutcome::refused(
            format!(
                "recovery protocol not ready-for-candidate (phase {:?}); no dispatch",
                recovery.phase
            ),
            trail,
        );
    }

    // 3. Exact archive delivery; remote hash mismatch stops before exec.
    trail.push("deliver-archive".into());
    let evidence = match transport.deliver_and_collect(archive) {
        Ok(e) => e,
        Err(e) => {
            trail.push("post-run-eacs".into());
            // Delivery was attempted: require EACS reset anyway (hygiene).
            let mut verdict = VerdictStatus::Fail;
            let mut reasons = vec![format!("archive delivery stopped before exec: {e}")];
            let post_run_reset_started =
                require_post_run_eacs(reset, recovery, &mut verdict, &mut reasons);
            return MacosGateOutcome {
                verdict,
                reasons,
                host_verdict: None,
                golden_ok: false,
                post_run_reset_started,
                trail,
            };
        }
    };

    // 4. Raw evidence collected (samples / readback / manifests).
    trail.push("collect-evidence".into());
    let mut reasons = Vec::new();
    let mut verdict = VerdictStatus::Pass;

    if let Err(reason) = evidence_matches_contract(params.runner_manifest, &evidence) {
        verdict = worse(verdict, VerdictStatus::Fail);
        reasons.push(reason);
    }

    // 5a. Recompute perf/alloc/drain gates from raw samples only.
    trail.push("recompute-verdict".into());
    let host_verdict = verify_host(&archive.sha256, &evidence, params.policy);
    if host_verdict.verdict != VerdictStatus::Pass {
        reasons.push(host_verdict.reason.clone());
    }
    verdict = worse(verdict, host_verdict.verdict);

    // 5b. Recompute golden diff from raw readback bytes.
    trail.push("golden-diff".into());
    let golden_ok = match golden_diff(params, &evidence, readback_png) {
        Ok(()) => true,
        Err(e) => {
            verdict = worse(verdict, VerdictStatus::Fail);
            reasons.push(format!("golden diff reject: {e}"));
            false
        }
    };

    // 6. Post-run EACS reset is required; a run without reset is not verified
    //    and an un-reset host quarantines (T23).
    trail.push("post-run-eacs".into());
    let post_run_reset_started = require_post_run_eacs(reset, recovery, &mut verdict, &mut reasons);

    MacosGateOutcome {
        verdict,
        reasons,
        host_verdict: Some(host_verdict),
        golden_ok,
        post_run_reset_started,
        trail,
    }
}

/// Evidence identity must match the frozen runner contract (thin cross-check;
/// perf truth stays in `verify_host`). Non-Metal evidence is rejected here.
fn evidence_matches_contract(
    manifest: &MacosRunnerManifest,
    evidence: &HostEvidence,
) -> Result<(), String> {
    let host = &evidence.host_manifest;
    if host.host_id != manifest.runner_id {
        return Err(format!(
            "evidence host_id {} != runner {}",
            host.host_id, manifest.runner_id
        ));
    }
    if host.platform != manifest.platform {
        return Err(format!(
            "evidence platform {} != contract {}",
            host.platform, manifest.platform
        ));
    }
    if host.backend != manifest.backend {
        return Err(format!(
            "evidence backend {} != contract {}",
            host.backend, manifest.backend
        ));
    }
    Ok(())
}

/// Coordinator golden recompute: load reviewed golden, decode raw candidate
/// readback, byte-compare under the frozen zero-delta policy. Unlike the
/// Windows lane (`d3d12` → `direct3d12`), the T21 macOS contract already pins
/// the SDL driver name `metal`, so the evidence backend binds unmapped;
/// non-`metal` evidence fails both the contract cross-check and this binding.
fn golden_diff(
    params: &MacosGateParams<'_>,
    evidence: &HostEvidence,
    readback_png: &[u8],
) -> Result<(), String> {
    let manifest_path = params.golden_dir.join("manifest.json");
    let manifest = load_golden_manifest(&manifest_path).map_err(|e| e.to_string())?;
    let golden_rgba = load_golden_image(params.golden_dir, &manifest).map_err(|e| e.to_string())?;
    let candidate = decode_readback_png(
        "candidate-readback",
        readback_png,
        manifest.width,
        manifest.height,
    )
    .map_err(|e| e.to_string())?;
    let binding = HostBinding {
        backend: evidence.host_manifest.backend.clone(),
        os: os_family(&evidence.host_manifest.platform),
        adapter: evidence.host_manifest.gpu.clone(),
        atlas_manifest_sha256: params.atlas_manifest_sha256.clone(),
        shader_canonical_sha256: params.shader_canonical_sha256.clone(),
    };
    compare_readback(&manifest, &binding, &golden_rgba, &candidate).map_err(|e| e.to_string())
}

/// `macos-arm64` → `macos` (golden manifests bind an OS family).
fn os_family(platform: &str) -> String {
    platform.split('-').next().unwrap_or(platform).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use mmd_engine::bench::GATE_AGENT_COUNT;
    use mmd_engine::render::{
        BACKEND_METAL, GOLDEN_SCENE_STATIC_DEMO, GOLDEN_SCHEMA_VERSION, GOLDEN_STATUS_PLACEHOLDER,
        GoldenManifest, write_golden,
    };
    use tempfile::TempDir;

    use crate::archive::{ArchiveEntry, build_archive};
    use crate::macos::{parse_attestation_json, parse_manifest_toml};
    use crate::macos_recovery::simulate_successful_macos_eacs_drill;
    use crate::report::{ClaimedStats, HostManifest, RawTrialSamples};
    use crate::ssh::FakeAgent;
    use crate::verify::recompute_gate_aggregate;

    const PROFILE: &str = "mmd-lab-macos-ref";
    const TEST_ADAPTER: &str = "Apple M4 (unit fixture)";
    const ATLAS_PIN: &str = "atlas-pin";
    const SHADER_PIN: &str = "shader-pin";

    fn runner_manifest() -> MacosRunnerManifest {
        parse_manifest_toml(&format!(
            r#"
schema_version = "macos-runner-manifest-v1"
runner_id = "macos-ref"
platform = "macos-arm64"
os_name = "macOS"
os_version = "15"
os_build = "24A335"
arch = "arm64"
backend = "metal"
[hardware]
model_contains = "Mac mini"
chip_contains = "M4"
memory_gb = 16
[metal]
require_metal = true
device_name_contains = "Apple M4"
reject_substrings = ["MoltenVK", "llvmpipe", "SwiftShader"]
[security]
security_mode = "Full Security"
ssv_valid_required = true
full_security_required = true
[enrollment]
ade_required = true
mdm_enrolled_required = true
mdm_profile_id = "{PROFILE}"
[eacs]
reset_ack_required = true
preflight_ok_required = true
"#
        ))
        .unwrap()
    }

    fn attestation_pass() -> MacosHostAttestation {
        parse_attestation_json(&format!(
            r#"{{
  "schema_version": "macos-host-attestation-v1",
  "runner_id": "macos-ref",
  "platform": "macos-arm64",
  "os_name": "macOS",
  "os_version": "15",
  "os_build": "24A335",
  "arch": "arm64",
  "backend": "metal",
  "hardware_model": "Mac mini (2024)",
  "chip": "Apple M4",
  "memory_gb": 16,
  "metal_available": true,
  "metal_device_name": "Apple M4",
  "security_mode": "Full Security",
  "ssv_valid": true,
  "full_security": true,
  "ade_enrolled": true,
  "mdm_enrolled": true,
  "mdm_profile_id": "{PROFILE}",
  "eacs_preflight_ok": true,
  "eacs_reset_ack": true
}}"#
        ))
        .unwrap()
    }

    fn golden_pixels() -> Vec<u8> {
        // 4x4 RGBA gradient.
        (0..4u32 * 4 * 4).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn write_test_golden(dir: &Path) -> GoldenManifest {
        let mut manifest = GoldenManifest {
            schema_version: GOLDEN_SCHEMA_VERSION.into(),
            backend: BACKEND_METAL.into(),
            os: "macos".into(),
            status: GOLDEN_STATUS_PLACEHOLDER.into(),
            scene: GOLDEN_SCENE_STATIC_DEMO.into(),
            width: 4,
            height: 4,
            image_file: "golden.png".into(),
            image_sha256: String::new(),
            adapter: TEST_ADAPTER.into(),
            driver_info: "unit-fixture".into(),
            atlas_manifest_sha256: ATLAS_PIN.into(),
            shader_canonical_sha256: SHADER_PIN.into(),
            max_channel_delta: 0,
        };
        write_golden(dir, &mut manifest, &golden_pixels()).unwrap();
        manifest
    }

    fn readback_png(pixels: &[u8]) -> Vec<u8> {
        mmd_engine::render::encode_rgba_png(4, 4, pixels).unwrap()
    }

    fn pass_evidence() -> HostEvidence {
        let mut manifest = HostManifest::new("macos-ref", "macos-arm64", "metal", "macOS 15");
        manifest.gpu = TEST_ADAPTER.into();
        let mut e = HostEvidence::new(manifest, "pending");
        e.submitted_frames = 420;
        e.completed_frames = 420;
        e.max_in_flight = 2;
        e.project_rust_alloc_count = 0;
        for i in 0..7u32 {
            let ms = 11.0 + f64::from(i) * 0.02;
            e.raw_trials.push(RawTrialSamples {
                agent_count: GATE_AGENT_COUNT,
                trial_index: i,
                frame_service_ms: vec![ms; 128],
            });
        }
        honest_claim(&mut e);
        e
    }

    fn honest_claim(e: &mut HostEvidence) {
        let agg = recompute_gate_aggregate(e).unwrap();
        e.claimed = Some(ClaimedStats {
            median_p95_ms: agg.median_p95_ms,
            median_p99_ms: agg.median_p99_ms,
            verdict: "pass".into(),
        });
    }

    struct Lane {
        _tmp: TempDir,
        golden_dir: PathBuf,
        manifest: MacosRunnerManifest,
        attestation: MacosHostAttestation,
        archive: ArchiveBlob,
        policy: BenchPolicy,
    }

    impl Lane {
        fn new() -> Self {
            let tmp = TempDir::new().unwrap();
            let golden_dir = tmp.path().join("macos-metal");
            write_test_golden(&golden_dir);
            let archive = build_archive(vec![ArchiveEntry {
                path: "src/main.rs".into(),
                data: b"fn main() {}".to_vec(),
            }])
            .unwrap();
            Self {
                _tmp: tmp,
                golden_dir,
                manifest: runner_manifest(),
                attestation: attestation_pass(),
                archive,
                policy: BenchPolicy::production(),
            }
        }

        fn params(&self) -> MacosGateParams<'_> {
            MacosGateParams {
                runner_manifest: &self.manifest,
                attestation: &self.attestation,
                golden_dir: &self.golden_dir,
                policy: &self.policy,
                atlas_manifest_sha256: ATLAS_PIN.into(),
                shader_canonical_sha256: SHADER_PIN.into(),
            }
        }

        fn run(
            &self,
            evidence: HostEvidence,
            corrupt_archive: bool,
            readback: &[u8],
            reset: &mut dyn MacosResetController,
        ) -> (MacosGateOutcome, MacosRecoveryState) {
            let mut recovery =
                simulate_successful_macos_eacs_drill(PROFILE, "MAC-HOST-FRESH", None);
            let mut agent = FakeAgent::from_evidence(evidence);
            agent.corrupt_archive = corrupt_archive;
            let outcome = run_macos_gate(
                &self.params(),
                &mut recovery,
                &mut agent,
                &self.archive,
                readback,
                reset,
            );
            (outcome, recovery)
        }
    }

    struct DenyReset;

    impl MacosResetController for DenyReset {
        fn start_post_run_eacs(&mut self, _state: &mut MacosRecoveryState) -> Result<(), String> {
            Err("eacs controller unreachable".into())
        }
    }

    #[test]
    fn mac_gate_pass_lane() {
        let lane = Lane::new();
        let (outcome, recovery) = lane.run(
            pass_evidence(),
            false,
            &readback_png(&golden_pixels()),
            &mut MacosEacsProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Pass, "{outcome:?}");
        assert!(outcome.golden_ok);
        assert!(outcome.post_run_reset_started);
        assert!(outcome.host_verdict.unwrap().claimed_stats_match);
        assert_eq!(
            outcome.trail,
            vec![
                "attest-identity",
                "recovery-gate",
                "deliver-archive",
                "collect-evidence",
                "recompute-verdict",
                "golden-diff",
                "post-run-eacs",
            ]
        );
        // Post-run EACS left candidate-ready state (protocol restarted).
        assert!(!recovery.phase.allows_candidate_provision());
        assert!(!recovery.phase.is_quarantined());
    }

    #[test]
    fn mac_tampered_stats_fail() {
        let lane = Lane::new();
        let mut evidence = pass_evidence();
        // Forge the claimed report while raw samples stay slow-honest.
        evidence.claimed = Some(ClaimedStats {
            median_p95_ms: 11.06,
            median_p99_ms: 1.0,
            verdict: "pass".into(),
        });
        let (outcome, _) = lane.run(
            evidence,
            false,
            &readback_png(&golden_pixels()),
            &mut MacosEacsProtocolResetController,
        );
        assert_ne!(outcome.verdict, VerdictStatus::Pass);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("claimed stats mismatch")),
            "{outcome:?}"
        );
        let hv = outcome.host_verdict.unwrap();
        assert!(!hv.claimed_stats_match);
    }

    #[test]
    fn mac_non_metal_fails() {
        let lane = Lane::new();
        let mut evidence = pass_evidence();
        evidence.host_manifest.backend = "vulkan".into();
        let (outcome, _) = lane.run(
            evidence,
            false,
            &readback_png(&golden_pixels()),
            &mut MacosEacsProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Fail);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("evidence backend vulkan != contract metal")),
            "{outcome:?}"
        );
        // Non-Metal evidence also fails the golden backend binding.
        assert!(!outcome.golden_ok, "{outcome:?}");
    }

    #[test]
    fn mac_wrong_archive_fails() {
        let lane = Lane::new();
        let (outcome, _) = lane.run(
            pass_evidence(),
            true, // remote bytes corrupted → hash mismatch
            &readback_png(&golden_pixels()),
            &mut MacosEacsProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Fail);
        // No execution: no evidence-derived verdict exists.
        assert!(outcome.host_verdict.is_none());
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("hash mismatch") && r.contains("stops")),
            "{outcome:?}"
        );
        // Delivery attempted → EACS reset still forced.
        assert!(outcome.post_run_reset_started);
    }

    #[test]
    fn mac_50k_miss_fails() {
        let lane = Lane::new();
        let mut evidence = pass_evidence();
        // p95 stays fast; tail pushes p99 over the 25 ms limit.
        for t in &mut evidence.raw_trials {
            let mut samples = vec![11.0; 98];
            samples.extend([40.0, 40.0]);
            t.frame_service_ms = samples;
        }
        honest_claim(&mut evidence);
        let (outcome, _) = lane.run(
            evidence,
            false,
            &readback_png(&golden_pixels()),
            &mut MacosEacsProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Fail);
        assert!(
            outcome.reasons.iter().any(|r| r.contains("50k median p99")),
            "{outcome:?}"
        );
    }

    #[test]
    fn mac_wrong_golden_fails() {
        let lane = Lane::new();
        let mut pixels = golden_pixels();
        pixels[0] ^= 0x40; // one channel off → zero-delta policy rejects
        let (outcome, _) = lane.run(
            pass_evidence(),
            false,
            &readback_png(&pixels),
            &mut MacosEacsProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Fail);
        assert!(!outcome.golden_ok);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("golden diff reject")),
            "{outcome:?}"
        );
    }

    #[test]
    fn mac_alloc_count_fails() {
        let lane = Lane::new();
        let mut evidence = pass_evidence();
        evidence.project_rust_alloc_count = 3;
        let (outcome, _) = lane.run(
            evidence,
            false,
            &readback_png(&golden_pixels()),
            &mut MacosEacsProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Fail);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("frame allocations")),
            "{outcome:?}"
        );
    }

    #[test]
    fn mac_post_eacs_required_and_failure_quarantines() {
        let lane = Lane::new();
        let (outcome, recovery) = lane.run(
            pass_evidence(),
            false,
            &readback_png(&golden_pixels()),
            &mut DenyReset,
        );
        // Perf/golden fine, but a run without EACS reset is never verified.
        assert_eq!(outcome.verdict, VerdictStatus::Error);
        assert!(!outcome.post_run_reset_started);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("post-run eacs reset not started")),
            "{outcome:?}"
        );
        // T23: EACS failure quarantines the host at protocol level.
        assert!(recovery.phase.is_quarantined(), "{:?}", recovery.phase);
        assert!(!recovery.phase.allows_candidate_provision());
        let reason = recovery.quarantine_reason.as_deref().unwrap_or("");
        assert!(reason.contains("post-run eacs"), "reason={reason}");
    }

    #[test]
    fn mac_wrong_archive_and_denied_reset_escalates_and_quarantines() {
        let lane = Lane::new();
        let (outcome, recovery) = lane.run(
            pass_evidence(),
            true, // delivery fails
            &readback_png(&golden_pixels()),
            &mut DenyReset,
        );
        // Un-reset host after attempted delivery = protocol error + quarantine.
        assert_eq!(outcome.verdict, VerdictStatus::Error);
        assert!(!outcome.post_run_reset_started);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("post-run eacs reset not started")),
            "{outcome:?}"
        );
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("stopped before exec")),
            "{outcome:?}"
        );
        assert!(recovery.phase.is_quarantined(), "{:?}", recovery.phase);
    }

    #[test]
    fn mac_non_m4_reject_stops_before_dispatch() {
        let lane = Lane::new();
        let mut attestation = attestation_pass();
        attestation.hardware_model = "MacBook Pro (16-inch, 2023)".into();
        attestation.chip = "Apple M3 Pro".into();
        let params = MacosGateParams {
            attestation: &attestation,
            ..lane.params()
        };
        let mut recovery = simulate_successful_macos_eacs_drill(PROFILE, "MAC-HOST-FRESH", None);
        let mut agent = FakeAgent::from_evidence(pass_evidence());
        let outcome = run_macos_gate(
            &params,
            &mut recovery,
            &mut agent,
            &lane.archive,
            &readback_png(&golden_pixels()),
            &mut MacosEacsProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Fail);
        assert!(outcome.host_verdict.is_none());
        assert!(!outcome.trail.iter().any(|s| s == "deliver-archive"));
        assert!(
            outcome.reasons.iter().any(|r| r.contains("identity only")),
            "{outcome:?}"
        );
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("wrong model/chip")),
            "{outcome:?}"
        );
    }

    #[test]
    fn mac_moltenvk_reject_stops_before_dispatch() {
        let lane = Lane::new();
        let mut attestation = attestation_pass();
        attestation.metal_device_name = "MoltenVK (Apple M4)".into();
        let params = MacosGateParams {
            attestation: &attestation,
            ..lane.params()
        };
        let mut recovery = simulate_successful_macos_eacs_drill(PROFILE, "MAC-HOST-FRESH", None);
        let mut agent = FakeAgent::from_evidence(pass_evidence());
        let outcome = run_macos_gate(
            &params,
            &mut recovery,
            &mut agent,
            &lane.archive,
            &readback_png(&golden_pixels()),
            &mut MacosEacsProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Fail);
        assert!(outcome.host_verdict.is_none());
        assert!(!outcome.trail.iter().any(|s| s == "deliver-archive"));
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("metal device rejected")),
            "{outcome:?}"
        );
    }

    #[test]
    fn mac_recovery_not_ready_stops_before_dispatch() {
        let lane = Lane::new();
        let mut recovery = MacosRecoveryState::new(PROFILE); // Idle, never restored
        let mut agent = FakeAgent::from_evidence(pass_evidence());
        let outcome = run_macos_gate(
            &lane.params(),
            &mut recovery,
            &mut agent,
            &lane.archive,
            &readback_png(&golden_pixels()),
            &mut MacosEacsProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Fail);
        assert!(outcome.host_verdict.is_none());
        assert!(!outcome.trail.iter().any(|s| s == "deliver-archive"));
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("not ready-for-candidate")),
            "{outcome:?}"
        );
    }
}
