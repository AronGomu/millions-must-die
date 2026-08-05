//! T26 pilot dataset pipeline CLI contract (synthetic scope).
//!
//! Real 150 lane runs are deferred-hw; these tests prove the end-to-end
//! synth -> assemble -> disabled-candidate pipeline and that synthetic
//! provenance can never enable a baseline.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

fn lab_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mmd-lab"))
}

fn synth_reports(dir: &Path) {
    let output = lab_bin()
        .args([
            "pilot-synth",
            "--seed",
            "42",
            "--out-dir",
            dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pilot_report_count 150"), "{stdout}");
    assert!(stdout.contains("provenance synthetic"), "{stdout}");
}

#[test]
fn cli_pilot_pipeline_emits_disabled_candidates() {
    let dir = tempdir().unwrap();
    let reports = dir.path().join("reports");
    let out = dir.path().join("baselines");
    let golden_review = dir.path().join("goldens-manifest.json");
    synth_reports(&reports);

    let output = lab_bin()
        .args([
            "pilot-assemble",
            "--reports-dir",
            reports.to_str().unwrap(),
            "--margin",
            "0.05",
            "--out-dir",
            out.to_str().unwrap(),
            "--golden-review-out",
            golden_review.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pilot_report_count 150"), "{stdout}");
    assert!(
        stdout.contains("baselines remain disabled pending real-hardware pilot"),
        "{stdout}"
    );

    for file in [
        "ubuntu-vulkan.json",
        "windows-d3d12.json",
        "macos-metal.json",
    ] {
        let body: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(out.join(file)).unwrap()).unwrap();
        assert_eq!(body["schema_version"], "baseline-v1", "{file}");
        assert_eq!(body["enabled"], false, "{file}");
        assert_eq!(body["review_required"], true, "{file}");
        assert!(body["review"].is_null(), "{file}");
        assert_eq!(body["provenance"], "synthetic", "{file}");
        assert_eq!(body["sample_count"], 50, "{file}");
    }

    let review: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&golden_review).unwrap()).unwrap();
    assert_eq!(review["schema_version"], "golden-tolerance-review-v1");
    assert_eq!(review["reviewed"], false);
    assert_eq!(review["provenance"], "synthetic");
    let entries = review["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    for entry in entries {
        assert_eq!(entry["reviewed"], false);
        assert_eq!(entry["provenance"], "synthetic");
        assert_eq!(entry["tolerance"], entry["observed_max_channel_delta"]);
    }
}

#[test]
fn cli_pilot_149_reports_reject() {
    let dir = tempdir().unwrap();
    let reports = dir.path().join("reports");
    let out = dir.path().join("baselines");
    synth_reports(&reports);
    fs::remove_file(reports.join("macos/report-049.json")).unwrap();

    let output = lab_bin()
        .args([
            "pilot-assemble",
            "--reports-dir",
            reports.to_str().unwrap(),
            "--margin",
            "0.05",
            "--out-dir",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("exactly 150") && err.contains("149"), "{err}");
    assert!(!out.exists(), "no candidates on rejection");
}

#[test]
fn cli_synthetic_candidate_cannot_enable() {
    let dir = tempdir().unwrap();
    let reports = dir.path().join("reports");
    let out = dir.path().join("baselines");
    let enabled = dir.path().join("enabled.json");
    synth_reports(&reports);
    assert!(
        lab_bin()
            .args([
                "pilot-assemble",
                "--reports-dir",
                reports.to_str().unwrap(),
                "--margin",
                "0.05",
                "--out-dir",
                out.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );

    // Even a full owner review record must be refused on synthetic provenance.
    let output = lab_bin()
        .args([
            "calibrate",
            "--enable-reviewed",
            "--candidate",
            out.join("ubuntu-vulkan.json").to_str().unwrap(),
            "--reviewer",
            "owner",
            "--evidence-ref",
            "lab/calibration/synthetic-set",
            "--reviewed-at",
            "2026-08-05T00:00:00Z",
            "--out",
            enabled.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "synthetic enable must fail");
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("provenance 'synthetic' cannot enable"),
        "{err}"
    );
    assert!(!enabled.exists(), "no enabled baseline may be written");
}
