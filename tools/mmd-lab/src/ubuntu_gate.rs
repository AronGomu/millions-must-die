//! Ubuntu candidate gate lane (T17). Coordinator-side adapter, thin by design.
//!
//! Lane order (each step gates the next):
//! 1. Host attestation — identity only (contract match; never perf truth).
//! 2. Recovery protocol must be `ReadyForCandidate` (external restore lane).
//! 3. Exact archive delivery; remote hash verified — mismatch stops before exec.
//! 4. Collect raw evidence (samples / readback / manifests) via transport.
//! 5. Coordinator recomputes stats + golden diff from raw evidence; the
//!    candidate-reported verdict is never trusted.
//! 6. Post-run reset is forced at protocol level (external controller event).
//!
//! Fixture/fake-transport scope: the native Vulkan smoke/scale run and the
//! physical PXE reset are deferred-hw; this lane drives the exact same
//! coordinator protocol against fixture evidence.
//!
//! Bounded risk (accepted, documented): attestation covers host/source
//! identity only. A hostile candidate can still deny service or forge its own
//! raw output wholesale — including the adapter/os strings the golden
//! [`HostBinding`] is built from (candidate evidence, not attestation): the
//! captured golden is dev-adapter while the frozen contract pins RX 6400, so
//! binding identity cannot come from attestation until the golden is
//! recaptured on the ref host (deferred-hw; retire this note then). The
//! coordinator recomputes every gate from raw evidence and forces an external
//! post-run restore, but cannot cryptographically attest candidate behavior —
//! manual review + reset model remains required (see plan "Risks / stop
//! rules").

use std::path::Path;

use mmd_engine::bench::{BenchPolicy, VerdictStatus};
use mmd_engine::render::{
    HostBinding, compare_readback, decode_readback_png, load_golden_image, load_golden_manifest,
};

use crate::archive::ArchiveBlob;
use crate::report::HostEvidence;
use crate::ssh::AgentTransport;
use crate::ubuntu::{
    AttestVerdict, UbuntuHostAttestation, UbuntuRunnerManifest, validate_ubuntu_attestation,
};
use crate::ubuntu_recovery::{RecoveryEvent, RecoveryState};
use crate::verify::{HostVerdict, verify_host, worse};

/// Inputs frozen before dispatch (trusted side; never candidate-supplied).
pub struct UbuntuGateParams<'a> {
    pub runner_manifest: &'a UbuntuRunnerManifest,
    /// Observed host attestation (identity only).
    pub attestation: &'a UbuntuHostAttestation,
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
pub struct UbuntuGateOutcome {
    pub verdict: VerdictStatus,
    /// Non-pass reasons, in lane order.
    pub reasons: Vec<String>,
    /// Recomputed host verdict; `None` when execution never happened.
    pub host_verdict: Option<HostVerdict>,
    /// Golden diff recomputed from raw readback evidence.
    pub golden_ok: bool,
    /// Post-run external restore was initiated (protocol-level).
    pub post_run_reset_started: bool,
    /// Ordered lane step log for run evidence.
    pub trail: Vec<String>,
}

impl UbuntuGateOutcome {
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

/// External post-run reset hook. Real power/PXE control is deferred-hw; the
/// protocol event below is the trusted-side contract either way.
pub trait ResetController {
    /// Initiate external restore after a candidate run. `Ok` = protocol
    /// restore started (host leaves candidate-ready state).
    fn start_post_run_restore(&mut self, state: &mut RecoveryState) -> Result<(), String>;
}

/// Protocol-level controller: applies the external-restore event.
pub struct ProtocolResetController;

impl ResetController for ProtocolResetController {
    fn start_post_run_restore(&mut self, state: &mut RecoveryState) -> Result<(), String> {
        state.apply(RecoveryEvent::StartExternalRestore {
            external_controller: true,
        })
    }
}

/// Run the Ubuntu candidate gate lane. See module docs for order + risk.
pub fn run_ubuntu_gate(
    params: &UbuntuGateParams<'_>,
    recovery: &mut RecoveryState,
    transport: &mut dyn AgentTransport,
    archive: &ArchiveBlob,
    readback_png: &[u8],
    reset: &mut dyn ResetController,
) -> UbuntuGateOutcome {
    let mut trail = vec!["attest-identity".to_string()];

    // 1. Host attestation — identity only; refusal means no dispatch at all.
    let attest = validate_ubuntu_attestation(params.runner_manifest, params.attestation);
    if attest.verdict != AttestVerdict::ReadyForRecovery {
        return UbuntuGateOutcome::refused(
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
        return UbuntuGateOutcome::refused(
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
            trail.push("post-run-reset".into());
            // Delivery was attempted: force restore anyway (hygiene).
            let mut verdict = VerdictStatus::Fail;
            let mut reasons = vec![format!("archive delivery stopped before exec: {e}")];
            let post_run_reset_started = match reset.start_post_run_restore(recovery) {
                Ok(()) => true,
                Err(reset_err) => {
                    verdict = worse(verdict, VerdictStatus::Error);
                    reasons.push(format!("post-run reset not started: {reset_err}"));
                    false
                }
            };
            return UbuntuGateOutcome {
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

    // 6. Post-run reset is required; a run without reset is not verified.
    trail.push("post-run-reset".into());
    let post_run_reset_started = match reset.start_post_run_restore(recovery) {
        Ok(()) => true,
        Err(e) => {
            verdict = worse(verdict, VerdictStatus::Error);
            reasons.push(format!("post-run reset not started: {e}"));
            false
        }
    };

    UbuntuGateOutcome {
        verdict,
        reasons,
        host_verdict: Some(host_verdict),
        golden_ok,
        post_run_reset_started,
        trail,
    }
}

/// Evidence identity must match the frozen runner contract (thin cross-check;
/// perf truth stays in `verify_host`).
fn evidence_matches_contract(
    manifest: &UbuntuRunnerManifest,
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
/// readback, byte-compare under the frozen zero-delta policy.
fn golden_diff(
    params: &UbuntuGateParams<'_>,
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

/// `linux-x86_64` → `linux` (golden manifests bind an OS family).
fn os_family(platform: &str) -> String {
    platform.split('-').next().unwrap_or(platform).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use mmd_engine::bench::GATE_AGENT_COUNT;
    use mmd_engine::render::{
        GOLDEN_SCENE_STATIC_DEMO, GOLDEN_SCHEMA_VERSION, GOLDEN_STATUS_PLACEHOLDER, GoldenManifest,
        write_golden,
    };
    use tempfile::TempDir;

    use crate::archive::{ArchiveEntry, build_archive};
    use crate::report::{ClaimedStats, HostManifest, RawTrialSamples};
    use crate::ssh::FakeAgent;
    use crate::ubuntu::{parse_attestation_json, parse_manifest_toml};
    use crate::ubuntu_recovery::simulate_successful_drill;
    use crate::verify::recompute_gate_aggregate;

    const IMAGE_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TEST_ADAPTER: &str = "Test GPU RX 6400";
    const ATLAS_PIN: &str = "atlas-pin";
    const SHADER_PIN: &str = "shader-pin";

    fn runner_manifest() -> UbuntuRunnerManifest {
        parse_manifest_toml(&format!(
            r#"
schema_version = "ubuntu-runner-manifest-v1"
runner_id = "ubuntu-ref"
platform = "linux-x86_64"
os_name = "Ubuntu"
os_version = "24.04"
arch = "x86_64"
backend = "vulkan"
[cpu]
model_contains = "8600G"
[gpu]
model_contains = "RX 6400"
vram_mb = 4096
[driver]
name = "amdgpu"
version = "23.2.1"
[firmware]
bios_version = "F1a"
vbios_version = "113-D5040100-100"
secure_boot_required = true
[image]
digest_sha256 = "{IMAGE_DIGEST}"
[vulkan]
allow_software = false
device_name_contains = "RX 6400"
reject_substrings = ["llvmpipe", "lavapipe"]
"#
        ))
        .unwrap()
    }

    fn attestation_pass() -> UbuntuHostAttestation {
        parse_attestation_json(&format!(
            r#"{{
  "schema_version": "ubuntu-host-attestation-v1",
  "runner_id": "ubuntu-ref",
  "platform": "linux-x86_64",
  "os_name": "Ubuntu",
  "os_version": "24.04",
  "arch": "x86_64",
  "backend": "vulkan",
  "cpu_model": "AMD Ryzen 5 8600G",
  "gpu_model": "AMD Radeon RX 6400",
  "gpu_vram_mb": 4096,
  "driver_name": "amdgpu",
  "driver_version": "23.2.1",
  "bios_version": "F1a",
  "vbios_version": "113-D5040100-100",
  "secure_boot": true,
  "image_digest_sha256": "{IMAGE_DIGEST}",
  "vulkan_device_name": "AMD Radeon RX 6400",
  "vulkan_device_type": "discrete_gpu"
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
            backend: "vulkan".into(),
            os: "linux".into(),
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
        let mut manifest =
            HostManifest::new("ubuntu-ref", "linux-x86_64", "vulkan", "Ubuntu 24.04");
        manifest.gpu = TEST_ADAPTER.into();
        let mut e = HostEvidence::new(manifest, "pending");
        e.submitted_frames = 420;
        e.completed_frames = 420;
        e.max_in_flight = 2;
        e.project_rust_alloc_count = 0;
        for i in 0..7u32 {
            let ms = 10.0 + f64::from(i) * 0.02;
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
        manifest: UbuntuRunnerManifest,
        attestation: UbuntuHostAttestation,
        archive: ArchiveBlob,
        policy: BenchPolicy,
    }

    impl Lane {
        fn new() -> Self {
            let tmp = TempDir::new().unwrap();
            let golden_dir = tmp.path().join("linux-vulkan");
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

        fn params(&self) -> UbuntuGateParams<'_> {
            UbuntuGateParams {
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
            reset: &mut dyn ResetController,
        ) -> (UbuntuGateOutcome, RecoveryState) {
            let mut recovery =
                simulate_successful_drill(IMAGE_DIGEST, "ssh-ed25519 AAAAfresh", None);
            let mut agent = FakeAgent::from_evidence(evidence);
            agent.corrupt_archive = corrupt_archive;
            let outcome = run_ubuntu_gate(
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

    impl ResetController for DenyReset {
        fn start_post_run_restore(&mut self, _state: &mut RecoveryState) -> Result<(), String> {
            Err("external controller unreachable".into())
        }
    }

    #[test]
    fn ubuntu_gate_pass_lane() {
        let lane = Lane::new();
        let (outcome, recovery) = lane.run(
            pass_evidence(),
            false,
            &readback_png(&golden_pixels()),
            &mut ProtocolResetController,
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
                "post-run-reset",
            ]
        );
        // Post-run reset left candidate-ready state (protocol restarted).
        assert!(!recovery.phase.allows_candidate_provision());
    }

    #[test]
    fn ubuntu_tampered_stats_fail() {
        let lane = Lane::new();
        let mut evidence = pass_evidence();
        // Forge the claimed report while raw samples stay slow-honest.
        evidence.claimed = Some(ClaimedStats {
            median_p95_ms: 10.06,
            median_p99_ms: 1.0,
            verdict: "pass".into(),
        });
        let (outcome, _) = lane.run(
            evidence,
            false,
            &readback_png(&golden_pixels()),
            &mut ProtocolResetController,
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
    fn ubuntu_wrong_archive_fails() {
        let lane = Lane::new();
        let (outcome, _) = lane.run(
            pass_evidence(),
            true, // remote bytes corrupted → hash mismatch
            &readback_png(&golden_pixels()),
            &mut ProtocolResetController,
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
        // Delivery attempted → reset still forced.
        assert!(outcome.post_run_reset_started);
    }

    #[test]
    fn ubuntu_50k_miss_fails() {
        let lane = Lane::new();
        let mut evidence = pass_evidence();
        // p95 stays fast; tail pushes p99 over the 25 ms limit.
        for t in &mut evidence.raw_trials {
            let mut samples = vec![10.0; 98];
            samples.extend([40.0, 40.0]);
            t.frame_service_ms = samples;
        }
        honest_claim(&mut evidence);
        let (outcome, _) = lane.run(
            evidence,
            false,
            &readback_png(&golden_pixels()),
            &mut ProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Fail);
        assert!(
            outcome.reasons.iter().any(|r| r.contains("50k median p99")),
            "{outcome:?}"
        );
    }

    #[test]
    fn ubuntu_wrong_golden_fails() {
        let lane = Lane::new();
        let mut pixels = golden_pixels();
        pixels[0] ^= 0x40; // one channel off → zero-delta policy rejects
        let (outcome, _) = lane.run(
            pass_evidence(),
            false,
            &readback_png(&pixels),
            &mut ProtocolResetController,
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
    fn ubuntu_alloc_count_fails() {
        let lane = Lane::new();
        let mut evidence = pass_evidence();
        evidence.project_rust_alloc_count = 3;
        let (outcome, _) = lane.run(
            evidence,
            false,
            &readback_png(&golden_pixels()),
            &mut ProtocolResetController,
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
    fn ubuntu_reset_after_run_required() {
        let lane = Lane::new();
        let (outcome, recovery) = lane.run(
            pass_evidence(),
            false,
            &readback_png(&golden_pixels()),
            &mut DenyReset,
        );
        // Perf/golden fine, but a run without reset is never verified.
        assert_eq!(outcome.verdict, VerdictStatus::Error);
        assert!(!outcome.post_run_reset_started);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("post-run reset not started")),
            "{outcome:?}"
        );
        // Protocol state was not consumed by the failed reset.
        assert!(recovery.phase.allows_candidate_provision());
    }

    #[test]
    fn ubuntu_wrong_archive_and_denied_reset_escalates() {
        let lane = Lane::new();
        let (outcome, _) = lane.run(
            pass_evidence(),
            true, // delivery fails
            &readback_png(&golden_pixels()),
            &mut DenyReset,
        );
        // Un-reset host after attempted delivery = protocol error, not plain fail.
        assert_eq!(outcome.verdict, VerdictStatus::Error);
        assert!(!outcome.post_run_reset_started);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("post-run reset not started")),
            "{outcome:?}"
        );
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("stopped before exec")),
            "{outcome:?}"
        );
    }

    #[test]
    fn ubuntu_attestation_reject_stops_before_dispatch() {
        let lane = Lane::new();
        let mut attestation = attestation_pass();
        attestation.vulkan_device_name = "llvmpipe (LLVM)".into();
        attestation.vulkan_device_type = "cpu".into();
        let params = UbuntuGateParams {
            attestation: &attestation,
            ..lane.params()
        };
        let mut recovery = simulate_successful_drill(IMAGE_DIGEST, "ssh-ed25519 AAAAfresh", None);
        let mut agent = FakeAgent::from_evidence(pass_evidence());
        let outcome = run_ubuntu_gate(
            &params,
            &mut recovery,
            &mut agent,
            &lane.archive,
            &readback_png(&golden_pixels()),
            &mut ProtocolResetController,
        );
        assert_eq!(outcome.verdict, VerdictStatus::Fail);
        assert!(outcome.host_verdict.is_none());
        assert!(!outcome.trail.iter().any(|s| s == "deliver-archive"));
        assert!(
            outcome.reasons.iter().any(|r| r.contains("identity only")),
            "{outcome:?}"
        );
    }

    #[test]
    fn ubuntu_recovery_not_ready_stops_before_dispatch() {
        let lane = Lane::new();
        let mut recovery = RecoveryState::new(IMAGE_DIGEST); // Idle, never restored
        let mut agent = FakeAgent::from_evidence(pass_evidence());
        let outcome = run_ubuntu_gate(
            &lane.params(),
            &mut recovery,
            &mut agent,
            &lane.archive,
            &readback_png(&golden_pixels()),
            &mut ProtocolResetController,
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
