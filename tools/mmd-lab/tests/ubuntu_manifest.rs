//! T15 Ubuntu runner contract — manifest parse + fixture matrix.

use std::path::{Path, PathBuf};
use std::process::Command;

fn lab_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mmd-lab"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace")
        .to_path_buf()
}

fn manifest_path() -> PathBuf {
    workspace_root().join("lab/manifests/ubuntu-24.04-x86_64.toml")
}

fn fixture(name: &str) -> PathBuf {
    workspace_root().join("lab/fixtures/ubuntu-attest").join(name)
}

fn attest(observed: &Path) -> std::process::Output {
    lab_bin()
        .args(["attest-ubuntu", "--manifest"])
        .arg(manifest_path())
        .arg("--observed")
        .arg(observed)
        .output()
        .expect("run attest-ubuntu")
}

fn stdout_verdict(out: &std::process::Output) -> String {
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("verdict ") {
            return rest.trim().to_string();
        }
    }
    panic!(
        "missing verdict in stdout:\n{}\nstderr:\n{}",
        s,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn ubuntu_manifest_wrong_image_fails() {
    let out = attest(&fixture("wrong-image.json"));
    assert!(
        !out.status.success(),
        "wrong image must fail; stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(stdout_verdict(&out), "quarantine");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("image digest") || stdout.contains("reason"),
        "stdout={stdout}"
    );
}

#[test]
fn ubuntu_manifest_software_vulkan_fails() {
    let out = attest(&fixture("software-vulkan.json"));
    assert!(
        !out.status.success(),
        "llvmpipe must fail; stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(stdout_verdict(&out), "reject");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.to_ascii_lowercase().contains("software vulkan")
            || stdout.to_ascii_lowercase().contains("llvmpipe"),
        "stdout={stdout}"
    );
}

#[test]
fn ubuntu_manifest_vbios_drift_fails() {
    let out = attest(&fixture("vbios-drift.json"));
    assert!(
        !out.status.success(),
        "vbios drift must fail; stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(stdout_verdict(&out), "quarantine");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.to_ascii_lowercase().contains("vbios"),
        "stdout={stdout}"
    );
}

#[test]
fn ubuntu_manifest_fixture_passes() {
    let out = attest(&fixture("pass.json"));
    assert!(
        out.status.success(),
        "pass fixture must succeed; stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(stdout_verdict(&out), "ready-for-recovery");
}

#[test]
fn ubuntu_manifest_file_parses() {
    assert!(manifest_path().is_file(), "missing {}", manifest_path().display());
    let out = lab_bin()
        .args(["attest-ubuntu", "--manifest"])
        .arg(manifest_path())
        .arg("--observed")
        .arg(fixture("pass.json"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "manifest parse/validate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn ubuntu_manifest_dry_run_script_matrix() {
    let script = workspace_root().join("lab/provision/ubuntu/attest.sh");
    assert!(script.is_file(), "missing attest.sh");
    let lab = env!("CARGO_BIN_EXE_mmd-lab");

    let cases = [
        ("pass.json", true, "ready-for-recovery"),
        ("wrong-image.json", false, "quarantine"),
        ("software-vulkan.json", false, "reject"),
        ("vbios-drift.json", false, "quarantine"),
    ];

    for (name, expect_ok, verdict) in cases {
        let out = Command::new("bash")
            .arg(&script)
            .args([
                "--dry-run",
                "--fixture",
                fixture(name).to_str().unwrap(),
                "--manifest",
                manifest_path().to_str().unwrap(),
                "--lab-bin",
                lab,
            ])
            .output()
            .expect("attest.sh");
        let ok = out.status.success();
        assert_eq!(
            ok,
            expect_ok,
            "fixture {name}: ok={ok} expect={expect_ok}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(stdout_verdict(&out), verdict, "fixture {name}");
    }
}
