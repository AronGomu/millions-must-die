//! T25 relative calibration CLI contract.

use std::fs;
use std::process::Command;

use serde_json::json;
use tempfile::tempdir;

fn lab_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mmd-lab"))
}

fn stable_dataset_json(n: usize, drift_at: Option<usize>) -> String {
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let j = (i % 5) as f64 * 0.001;
        let mut sample = json!({
            "run_id": format!("run-{i:03}"),
            "median_p95_frame_service_ms": 12.0 + j,
            "median_p99_frame_service_ms": 14.0 + j,
            "median_sim_ms": 2.0 + j * 0.1,
            "median_upload_ms": 1.0 + j * 0.05,
            "median_gpu_queue_latency_ms": 3.0 + j * 0.2
        });
        if drift_at == Some(i) {
            sample.as_object_mut().unwrap().insert(
                "manifest_override".into(),
                json!({
                    "scenario_version": "scenario-v1",
                    "scenario_sha256": "abc123",
                    "shader_manifest_version": 1,
                    "os_build": "Ubuntu 24.04",
                    "driver": "DRIFTED",
                    "cpu": "8600G",
                    "gpu": "RX 6400",
                    "policy_id": "phase0-absolute-v1"
                }),
            );
        }
        samples.push(sample);
    }
    serde_json::to_string_pretty(&json!({
        "schema_version": "calibration-dataset-v1",
        "platform": "linux-x86_64",
        "backend": "vulkan",
        "manifest": {
            "scenario_version": "scenario-v1",
            "scenario_sha256": "abc123",
            "shader_manifest_version": 1,
            "os_build": "Ubuntu 24.04",
            "driver": "amdgpu 23.2",
            "cpu": "8600G",
            "gpu": "RX 6400",
            "policy_id": "phase0-absolute-v1"
        },
        "samples": samples
    }))
    .unwrap()
}

#[test]
fn cli_requires_50_clean_reports() {
    let dir = tempdir().unwrap();
    let ds = dir.path().join("ds.json");
    let out = dir.path().join("base.json");
    fs::write(&ds, stable_dataset_json(49, None)).unwrap();
    let output = lab_bin()
        .args([
            "calibrate",
            "--dataset",
            ds.to_str().unwrap(),
            "--margin",
            "0.05",
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("need ≥50") || err.contains("need >=50") || err.contains("Underfilled") || err.contains("50"),
        "{err}"
    );
    assert!(!out.exists());
}

#[test]
fn cli_mixed_manifest_rejects() {
    let dir = tempdir().unwrap();
    let ds = dir.path().join("ds.json");
    let out = dir.path().join("base.json");
    fs::write(&ds, stable_dataset_json(50, Some(7))).unwrap();
    let output = lab_bin()
        .args([
            "calibrate",
            "--dataset",
            ds.to_str().unwrap(),
            "--margin",
            "0.05",
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("mixed manifests") || err.contains("Mixed"), "{err}");
}

#[test]
fn cli_margin_must_exceed_noise() {
    let dir = tempdir().unwrap();
    let ds = dir.path().join("ds.json");
    let out = dir.path().join("base.json");
    fs::write(&ds, stable_dataset_json(50, None)).unwrap();
    // Tiny margin below any nonzero noise.
    let output = lab_bin()
        .args([
            "calibrate",
            "--dataset",
            ds.to_str().unwrap(),
            "--margin",
            "0.0000001",
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("margin must exceed noise"), "{err}");
}

#[test]
fn cli_output_requires_review_flag() {
    let dir = tempdir().unwrap();
    let ds = dir.path().join("ds.json");
    let out = dir.path().join("base.json");
    fs::write(&ds, stable_dataset_json(50, None)).unwrap();
    let output = lab_bin()
        .args([
            "calibrate",
            "--dataset",
            ds.to_str().unwrap(),
            "--margin",
            "0.05",
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("enabled false"), "{stdout}");
    assert!(stdout.contains("review_required true"), "{stdout}");
    let body: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(body["schema_version"], "baseline-v1");
    assert_eq!(body["enabled"], false);
    assert_eq!(body["review_required"], true);
    assert!(body["review"].is_null());
    assert_eq!(body["sample_count"], 50);
}

#[test]
fn cli_enable_reviewed_requires_owner() {
    let dir = tempdir().unwrap();
    let ds = dir.path().join("ds.json");
    let cand = dir.path().join("cand.json");
    let out = dir.path().join("enabled.json");
    fs::write(&ds, stable_dataset_json(50, None)).unwrap();
    assert!(lab_bin()
        .args([
            "calibrate",
            "--dataset",
            ds.to_str().unwrap(),
            "--margin",
            "0.05",
            "--out",
            cand.to_str().unwrap(),
        ])
        .status()
        .unwrap()
        .success());

    // Missing reviewer → fail; still disabled.
    let bad = lab_bin()
        .args([
            "calibrate",
            "--enable-reviewed",
            "--candidate",
            cand.to_str().unwrap(),
            "--evidence-ref",
            "ev-1",
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!bad.status.success());

    let good = lab_bin()
        .args([
            "calibrate",
            "--enable-reviewed",
            "--candidate",
            cand.to_str().unwrap(),
            "--reviewer",
            "owner",
            "--evidence-ref",
            "lab/calibration/set-1",
            "--reviewed-at",
            "2026-08-02T13:00:00Z",
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        good.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&good.stderr)
    );
    let body: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(body["enabled"], true);
    assert_eq!(body["review_required"], false);
    assert_eq!(body["review"]["reviewer"], "owner");
}
