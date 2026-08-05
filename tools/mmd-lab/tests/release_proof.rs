//! T27 release-proof CLI contract tests (freeze + check).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use tempfile::tempdir;

fn lab_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mmd-lab"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("tools/mmd-lab")
        .to_path_buf()
}

const COMMIT: &str = "82f162cce248d8af7286b21175de4b4b6183c9e4";
/// Raw production-policy bench evidence committed with the phase close.
const COMMITTED_EVIDENCE: &str = "lab/releases/evidence/linux-vulkan-bench-production-v1.json";

fn passing_report_json() -> serde_json::Value {
    let scale = |count: u64, blocking: bool, verdict: &str| {
        serde_json::json!({
            "agent_count": count,
            "blocking": blocking,
            "stretch": count == 100_000,
            "trials": [],
            "median_p95_frame_service_ms": 4.0,
            "median_p99_frame_service_ms": 6.0,
            "nmad_p95": 0.001,
            "nmad_p99": 0.001,
            "final_drain_ms": 0.1,
            "submitted_frames": 100,
            "completed_frames": 100,
            "max_in_flight": 2,
            "project_rust_alloc_count": 0,
            "verdict": verdict,
            "verdict_reason": "test row"
        })
    };
    serde_json::json!({
        "schema_version": "benchmark-report-v2",
        "manifests": {
            "scenario_version": "technical_prototype_v1",
            "scenario_sha256": "00",
            "atlas_manifest_sha256": "00",
            "backend": "vulkan",
            "adapter": "test-adapter",
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
        "scale_results": [
            scale(1000, false, "recorded"),
            scale(10000, false, "recorded"),
            scale(50000, true, "pass"),
            scale(100000, false, "recorded"),
        ],
        "verdict": "pass",
        "verdict_reason": "50k under limits"
    })
}

fn freeze(report: &serde_json::Value, commit: &str) -> (tempfile::TempDir, PathBuf, Output) {
    let dir = tempdir().unwrap();
    let report_path = dir.path().join("bench.json");
    fs::write(&report_path, serde_json::to_string_pretty(report).unwrap()).unwrap();
    let proof_path = dir.path().join("proof.json");
    let out = lab_bin()
        .args(["release-freeze", "--commit", commit, "--bench-report"])
        .arg(&report_path)
        .arg("--out")
        .arg(&proof_path)
        .output()
        .expect("run release-freeze");
    (dir, proof_path, out)
}

fn check(proof: &PathBuf, commit: &str, bench: Option<&PathBuf>) -> Output {
    let mut cmd = lab_bin();
    cmd.args(["release-check", "--commit", commit, "--proof"])
        .arg(proof);
    if let Some(b) = bench {
        cmd.arg("--bench-report").arg(b);
    }
    cmd.output().expect("run release-check")
}

#[test]
fn freeze_then_check_passes_linux_scope() {
    let (dir, proof_path, out) = freeze(&passing_report_json(), COMMIT);
    assert!(out.status.success(), "freeze failed: {out:?}");
    let bench = dir.path().join("bench.json");
    let out = check(&proof_path, COMMIT, Some(&bench));
    assert!(out.status.success(), "check failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(stdout.contains("linux-verified"));
    assert!(stdout.contains("deferred"));
    assert!(stdout.contains("lane windows-d3d12 state=deferred-hw"));
    assert!(stdout.contains("lane macos-metal state=deferred-hw"));
}

#[test]
fn check_rejects_mismatched_commit() {
    let (_dir, proof_path, out) = freeze(&passing_report_json(), COMMIT);
    assert!(out.status.success());
    let out = check(
        &proof_path,
        "1111111111111111111111111111111111111111",
        None,
    );
    assert_eq!(out.status.code(), Some(2), "expected rejection: {out:?}");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("commit"), "stderr: {err}");
}

#[test]
fn check_rejects_tampered_bench_evidence() {
    let (dir, proof_path, out) = freeze(&passing_report_json(), COMMIT);
    assert!(out.status.success());
    // Tamper with the raw evidence after freezing.
    let bench = dir.path().join("bench.json");
    let mut tampered = passing_report_json();
    tampered["scale_results"][2]["median_p95_frame_service_ms"] = serde_json::json!(1.0);
    fs::write(&bench, serde_json::to_string_pretty(&tampered).unwrap()).unwrap();
    let out = check(&proof_path, COMMIT, Some(&bench));
    assert_eq!(out.status.code(), Some(2), "expected rejection: {out:?}");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("sha256 mismatch"), "stderr: {err}");
}

#[test]
fn failed_gate_freezes_honest_failure() {
    let mut report = passing_report_json();
    report["scale_results"][2]["median_p95_frame_service_ms"] = serde_json::json!(20.0);
    report["scale_results"][2]["verdict"] = serde_json::json!("fail");
    report["verdict"] = serde_json::json!("fail");
    report["verdict_reason"] = serde_json::json!("50k median p95 20.000 ms > 16.67 ms");
    let (dir, proof_path, out) = freeze(&report, COMMIT);
    assert_eq!(out.status.code(), Some(1), "freeze should exit 1: {out:?}");
    let proof = fs::read_to_string(&proof_path).unwrap();
    assert!(proof.contains("\"status\": \"failed\""));
    let bench = dir.path().join("bench.json");
    let out = check(&proof_path, COMMIT, Some(&bench));
    assert_eq!(out.status.code(), Some(1), "honest failure exit 1: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(stdout.contains("honest phase failure"));
}

#[test]
fn tampered_proof_stats_rejected() {
    // Forgery class T27 exists to block: freeze honestly, then hand-edit the
    // PROOF (bench file untouched, sha256 still matches).
    let (dir, proof_path, out) = freeze(&passing_report_json(), COMMIT);
    assert!(out.status.success());
    let bench = dir.path().join("bench.json");
    let mut proof: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&proof_path).unwrap()).unwrap();
    proof["scale_results"][2]["median_p95_frame_service_ms"] = serde_json::json!(1.0);
    fs::write(&proof_path, serde_json::to_string_pretty(&proof).unwrap()).unwrap();
    let out = check(&proof_path, COMMIT, Some(&bench));
    assert_eq!(out.status.code(), Some(2), "expected rejection: {out:?}");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("stats diverge"), "stderr: {err}");
}

#[test]
fn massaged_status_rejected_against_failing_report() {
    // Freeze an honest failure, then massage the proof into a pass while
    // keeping the failing bench file. Cross-check must reject.
    let mut report = passing_report_json();
    report["scale_results"][2]["median_p95_frame_service_ms"] = serde_json::json!(20.0);
    report["scale_results"][2]["verdict"] = serde_json::json!("fail");
    report["verdict"] = serde_json::json!("fail");
    report["verdict_reason"] = serde_json::json!("50k median p95 20.000 ms > 16.67 ms");
    let (dir, proof_path, out) = freeze(&report, COMMIT);
    assert_eq!(out.status.code(), Some(1));
    let bench = dir.path().join("bench.json");
    let mut proof: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&proof_path).unwrap()).unwrap();
    proof["status"] = serde_json::json!("linux-verified-deferred-hw");
    proof["claim"] = serde_json::json!("Linux-verified; native cross-platform matrix deferred");
    proof["scale_results"][2]["median_p95_frame_service_ms"] = serde_json::json!(4.0);
    proof["scale_results"][2]["verdict"] = serde_json::json!("pass");
    proof["absolute_gate"]["verdict"] = serde_json::json!("pass");
    proof["absolute_gate"]["reason"] = serde_json::json!("massaged");
    fs::write(&proof_path, serde_json::to_string_pretty(&proof).unwrap()).unwrap();
    let out = check(&proof_path, COMMIT, Some(&bench));
    assert_eq!(out.status.code(), Some(2), "expected rejection: {out:?}");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        err.contains("inconsistent with bench report verdict") || err.contains("stats diverge"),
        "stderr: {err}"
    );
}

#[test]
fn inconclusive_report_cannot_freeze_any_status() {
    let mut report = passing_report_json();
    report["scale_results"][2]["verdict"] = serde_json::json!("inconclusive");
    report["scale_results"][2]["nmad_p95"] = serde_json::json!(0.08);
    report["verdict"] = serde_json::json!("inconclusive");
    report["verdict_reason"] = serde_json::json!("50k normalized MAD over 3%");
    let (_dir, _proof_path, out) = freeze(&report, COMMIT);
    assert_eq!(out.status.code(), Some(2), "expected rejection: {out:?}");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("inconclusive"), "stderr: {err}");
}

#[test]
fn test_policy_bench_evidence_rejected_at_freeze() {
    // A test-short-v1 report must never freeze a release proof, even though
    // its file hash would match perfectly.
    let mut report = passing_report_json();
    report["manifests"]["policy_id"] = serde_json::json!("test-short-v1");
    let (_dir, _proof_path, out) = freeze(&report, COMMIT);
    assert_eq!(out.status.code(), Some(2), "expected rejection: {out:?}");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("production-v1"), "stderr: {err}");
}

#[test]
fn release_proof_schema_tracks_struct() {
    // schemas/release-proof-v1.schema.json must stay in lockstep with the
    // serde types: freeze a proof and validate its shape against the schema's
    // required key sets.
    let schema: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace_root().join("schemas/release-proof-v1.schema.json"))
            .expect("schema file"),
    )
    .expect("schema json");
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        "release-proof-v1"
    );

    let (_dir, proof_path, out) = freeze(&passing_report_json(), COMMIT);
    assert!(out.status.success());
    let proof: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&proof_path).unwrap()).unwrap();

    let required = schema["required"].as_array().expect("required");
    for key in required {
        assert!(
            proof.get(key.as_str().unwrap()).is_some(),
            "frozen proof missing required key {key}"
        );
    }
    let scale_required = schema["properties"]["scale_results"]["items"]["required"]
        .as_array()
        .expect("scale required");
    let row = &proof["scale_results"][0];
    for key in scale_required {
        assert!(
            row.get(key.as_str().unwrap()).is_some(),
            "scale row missing required key {key}"
        );
    }
    // Noise evidence + locked thresholds must be visible in the proof.
    assert!(scale_required.iter().any(|v| v == "nmad_p95"));
    let gate_required = schema["properties"]["absolute_gate"]["required"]
        .as_array()
        .expect("gate required");
    assert!(gate_required.iter().any(|v| v == "nmad_limit"));

    // Reverse direction: every frozen key must be declared in the schema
    // (a struct field addition must force a schema update).
    for (key, _) in proof.as_object().unwrap() {
        assert!(
            schema["properties"].get(key).is_some(),
            "schema missing declaration for frozen key {key}"
        );
    }

    // Schema threshold consts must match the locked engine gate values.
    let gate_props = &schema["properties"]["absolute_gate"]["properties"];
    assert_eq!(
        gate_props["median_p95_limit_ms"]["const"].as_f64().unwrap(),
        mmd_engine::bench::GATE_P95_MS
    );
    assert_eq!(
        gate_props["median_p99_limit_ms"]["const"].as_f64().unwrap(),
        mmd_engine::bench::GATE_P99_MS
    );
    assert_eq!(
        gate_props["nmad_limit"]["const"].as_f64().unwrap(),
        mmd_engine::bench::NMAD_LIMIT
    );
}

#[test]
fn committed_release_proof_validates() {
    // Phase-0 close contract. The raw production bench evidence is committed
    // unconditionally; the frozen proof exists only when that evidence is
    // decisive. A missing proof is honest ONLY while the committed 50k
    // evidence is itself undecided (release.rs forbids freezing any status
    // from inconclusive/errored numbers) — decisive evidence with no proof
    // means the close was left unfinished, and fails here.
    let root = workspace_root();
    let evidence_path = root.join(COMMITTED_EVIDENCE);
    assert!(
        evidence_path.exists(),
        "missing committed bench evidence {}",
        evidence_path.display()
    );
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&evidence_path).unwrap()).unwrap();
    let report_verdict = report["verdict"].as_str().unwrap();

    let proof_path = root.join("lab/releases/technical-prototype-v1.json");
    if !proof_path.exists() {
        assert!(
            matches!(report_verdict, "inconclusive" | "error"),
            "no frozen release proof, but committed evidence is decisive \
             ({report_verdict}): freeze it or record the failure honestly"
        );
        return;
    }

    let proof: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&proof_path).unwrap()).unwrap();
    // Pin the exact benched candidate commit — a proof frozen against any
    // other commit is a different release and must fail this suite.
    assert_eq!(
        proof["candidate_commit"].as_str().unwrap(),
        COMMIT,
        "committed proof bound to unexpected candidate commit"
    );
    let evidence = root.join(proof["bench_evidence"]["path"].as_str().unwrap());
    assert!(
        evidence.exists(),
        "missing committed bench evidence {}",
        evidence.display()
    );
    let out = check(&proof_path, COMMIT, Some(&evidence));
    // Exit 0 (linux-verified) or 1 (honest failure) are both valid frozen
    // states; anything else means the committed proof is malformed.
    let code = out.status.code();
    assert!(
        code == Some(0) || code == Some(1),
        "committed proof rejected: {out:?}"
    );
    // Stdout verdict must reflect the frozen status; never an implied
    // full 3-OS pass.
    let status = proof["status"].as_str().unwrap();
    assert_ne!(status, "full-pass");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(stdout.contains(&format!("release_status {status}")));
    match status {
        "linux-verified-deferred-hw" => assert_eq!(code, Some(0)),
        "failed" => assert_eq!(code, Some(1)),
        other => panic!("unexpected frozen status {other}"),
    }
}
