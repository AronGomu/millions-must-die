//! T20 Windows candidate gate lane — CLI contract tests (fixture/fake transport).
//!
//! Mirrors the Ubuntu lane (T17): same evidence/trust rules, D3D12 asserted,
//! native run + physical FFU reset deferred-hw.

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

fn candidate_root() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), b"fn main() {}").unwrap();
    dir
}

fn run_lane(extra: &[&str]) -> Output {
    let root = candidate_root();
    let out = lab_bin()
        .args([
            "validate-runner",
            "--runner",
            "windows",
            "--commit",
            "deadbeefcafe",
            "--skip-self-check",
            "--root",
        ])
        .arg(root.path())
        .args(extra)
        .output()
        .expect("run mmd-lab");
    drop(root);
    out
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn windows_candidate_pass_lane_cli() {
    let out = run_lane(&[]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "stdout:\n{text}");
    assert!(text.contains("windows_verdict Pass"), "{text}");
    assert!(text.contains("golden_ok true"), "{text}");
    assert!(text.contains("post_run_reset_started true"), "{text}");
    assert!(text.contains("commit deadbeefcafe"), "{text}");
    assert!(text.contains("deferred-hw"), "{text}");
}

#[test]
fn windows_tampered_stats_fail_cli() {
    let fixture =
        workspace_root().join("lab/fixtures/windows-candidate/evidence-tampered-stats.json");
    let out = run_lane(&["--evidence-fixture", fixture.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("claimed stats mismatch"), "{text}");
    assert!(text.contains("claimed_stats_match false"), "{text}");
}

#[test]
fn windows_wrong_backend_fails_cli() {
    let fixture =
        workspace_root().join("lab/fixtures/windows-candidate/evidence-wrong-backend.json");
    let out = run_lane(&["--evidence-fixture", fixture.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("evidence backend"), "{text}");
}

#[test]
fn windows_50k_miss_fails_cli() {
    let fixture = workspace_root().join("lab/fixtures/windows-candidate/evidence-slow-p99.json");
    let out = run_lane(&["--evidence-fixture", fixture.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("50k median p99"), "{text}");
}

#[test]
fn windows_alloc_count_fails_cli() {
    let fixture = workspace_root().join("lab/fixtures/windows-candidate/evidence-alloc.json");
    let out = run_lane(&["--evidence-fixture", fixture.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("frame allocations"), "{text}");
}

#[test]
fn windows_wrong_golden_fails_cli() {
    // Tamper one pixel of the fixture golden and present it as readback.
    let ws = workspace_root();
    let golden_dir = ws.join("lab/fixtures/windows-candidate/golden");
    let manifest =
        mmd_engine::render::load_golden_manifest(&golden_dir.join("manifest.json")).unwrap();
    let mut rgba = mmd_engine::render::load_golden_image(&golden_dir, &manifest).unwrap();
    rgba[0] ^= 0x40;
    let png = mmd_engine::render::encode_rgba_png(manifest.width, manifest.height, &rgba).unwrap();
    let tmp = tempdir().unwrap();
    let readback = tmp.path().join("readback-wrong.png");
    fs::write(&readback, png).unwrap();

    let out = run_lane(&["--readback-fixture", readback.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("golden diff reject"), "{text}");
    assert!(text.contains("golden_ok false"), "{text}");
}

#[test]
fn windows_basic_renderer_reject_cli() {
    // Basic Render Driver attestation refuses dispatch before any delivery.
    let fixture = workspace_root().join("lab/fixtures/windows-attest/basic-renderer.json");
    let out = run_lane(&["--attest-fixture", fixture.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("identity only"), "{text}");
    assert!(!text.contains("deliver-archive"), "{text}");
}

#[test]
fn windows_run_candidate_script_fixture_and_live_modes() {
    // PowerShell wrapper: fixture mode passes; live mode exits 2 (deferred-hw).
    let ws = workspace_root();
    let script = ws.join("lab/provision/windows/run-candidate.ps1");
    assert!(script.is_file(), "missing run-candidate.ps1");

    let shell = ["pwsh", "powershell"].into_iter().find(|c| {
        Command::new(c)
            .args(["-NoProfile", "-Command", "$PSVersionTable"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    });
    let Some(shell) = shell else {
        // No PowerShell host: CLI tests above already cover the same lane.
        eprintln!("skipping run-candidate.ps1 execution: no pwsh/powershell");
        return;
    };

    let root = candidate_root();
    let out = Command::new(shell)
        .args([
            "-NoProfile",
            "-File",
            script.to_str().unwrap(),
            "--fixture-mode",
            "--commit",
            "deadbeefcafe",
            "--lab-bin",
            env!("CARGO_BIN_EXE_mmd-lab"),
            "--root",
        ])
        .arg(root.path())
        .args(["--skip-self-check"])
        .output()
        .expect("run-candidate.ps1 fixture mode");
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout:\n{text}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("windows_verdict Pass"), "{text}");

    let out = Command::new(shell)
        .args(["-NoProfile", "-File", script.to_str().unwrap()])
        .output()
        .expect("run-candidate.ps1 live mode");
    assert_eq!(
        out.status.code(),
        Some(2),
        "live mode must exit 2 deferred-hw"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("deferred-hw"), "stderr:\n{err}");

    // Usage errors are exit 2 (never 1 = gate fail): unknown arg + missing value.
    let out = Command::new(shell)
        .args([
            "-NoProfile",
            "-File",
            script.to_str().unwrap(),
            "--no-such-flag",
        ])
        .output()
        .expect("run-candidate.ps1 unknown arg");
    assert_eq!(out.status.code(), Some(2), "unknown arg must exit 2");

    let out = Command::new(shell)
        .args([
            "-NoProfile",
            "-File",
            script.to_str().unwrap(),
            "--fixture-mode",
            "--commit",
        ])
        .output()
        .expect("run-candidate.ps1 missing value");
    assert_eq!(out.status.code(), Some(2), "missing value must exit 2");
}
