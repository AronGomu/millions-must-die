//! T17 Ubuntu candidate gate lane — CLI contract tests (fixture/fake transport).

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
            "ubuntu",
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
fn ubuntu_candidate_pass_lane_cli() {
    let out = run_lane(&[]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "stdout:\n{text}");
    assert!(text.contains("ubuntu_verdict Pass"), "{text}");
    assert!(text.contains("golden_ok true"), "{text}");
    assert!(text.contains("post_run_reset_started true"), "{text}");
    assert!(text.contains("commit deadbeefcafe"), "{text}");
    assert!(text.contains("deferred-hw"), "{text}");
}

#[test]
fn ubuntu_tampered_stats_fail_cli() {
    let fixture =
        workspace_root().join("lab/fixtures/ubuntu-candidate/evidence-tampered-stats.json");
    let out = run_lane(&["--evidence-fixture", fixture.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("claimed stats mismatch"), "{text}");
    assert!(text.contains("claimed_stats_match false"), "{text}");
}

#[test]
fn ubuntu_50k_miss_fails_cli() {
    let fixture = workspace_root().join("lab/fixtures/ubuntu-candidate/evidence-slow-p99.json");
    let out = run_lane(&["--evidence-fixture", fixture.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("50k median p99"), "{text}");
}

#[test]
fn ubuntu_alloc_count_fails_cli() {
    let fixture = workspace_root().join("lab/fixtures/ubuntu-candidate/evidence-alloc.json");
    let out = run_lane(&["--evidence-fixture", fixture.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("frame allocations"), "{text}");
}

#[test]
fn ubuntu_wrong_golden_fails_cli() {
    // Tamper one pixel of the captured golden and present it as readback.
    let ws = workspace_root();
    let golden_dir = ws.join("lab/goldens/linux-vulkan");
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
