//! T18 Windows runner contract — manifest parse + fixture matrix.

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
    workspace_root().join("lab/manifests/windows-11-25h2-x86_64.toml")
}

fn fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("lab/fixtures/windows-attest")
        .join(name)
}

fn attest(observed: &Path) -> std::process::Output {
    lab_bin()
        .args(["attest-windows", "--manifest"])
        .arg(manifest_path())
        .arg("--observed")
        .arg(observed)
        .output()
        .expect("run attest-windows")
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
fn windows_manifest_wrong_ffu_fails() {
    let out = attest(&fixture("wrong-ffu.json"));
    assert!(
        !out.status.success(),
        "wrong ffu must fail; stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(stdout_verdict(&out), "quarantine");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.to_ascii_lowercase().contains("ffu digest") || stdout.contains("reason"),
        "stdout={stdout}"
    );
}

#[test]
fn windows_manifest_basic_renderer_fails() {
    let out = attest(&fixture("basic-renderer.json"));
    assert!(
        !out.status.success(),
        "basic renderer must fail; stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(stdout_verdict(&out), "reject");
    let stdout = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
    assert!(
        stdout.contains("basic render") || stdout.contains("basic render driver"),
        "stdout={stdout}"
    );
}

#[test]
fn windows_manifest_build_drift_fails() {
    let out = attest(&fixture("build-drift.json"));
    assert!(
        !out.status.success(),
        "build drift must fail; stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(stdout_verdict(&out), "maintenance-block");
    let stdout = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
    assert!(
        stdout.contains("os_build") || stdout.contains("maintenance"),
        "stdout={stdout}"
    );
}

#[test]
fn windows_manifest_fixture_passes() {
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
fn windows_manifest_file_parses() {
    assert!(
        manifest_path().is_file(),
        "missing {}",
        manifest_path().display()
    );
    let out = lab_bin()
        .args(["attest-windows", "--manifest"])
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
fn windows_manifest_dry_run_script_matrix() {
    let script = workspace_root().join("lab/provision/windows/attest.ps1");
    assert!(script.is_file(), "missing attest.ps1");
    let lab = env!("CARGO_BIN_EXE_mmd-lab");

    let cases = [
        ("pass.json", true, "ready-for-recovery"),
        ("wrong-ffu.json", false, "quarantine"),
        ("basic-renderer.json", false, "reject"),
        ("build-drift.json", false, "maintenance-block"),
    ];

    // Prefer pwsh/powershell when present; else rust CLI matrix covers same cases.
    let shell = ["pwsh", "powershell"].into_iter().find(|c| {
        Command::new(c)
            .arg("-NoProfile")
            .arg("-Command")
            .arg("$PSVersionTable")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    });

    if let Some(shell) = shell {
        for (name, expect_ok, verdict) in cases {
            let out = Command::new(shell)
                .args([
                    "-NoProfile",
                    "-File",
                    script.to_str().unwrap(),
                    "--dry-run",
                    "--fixture",
                    fixture(name).to_str().unwrap(),
                    "--manifest",
                    manifest_path().to_str().unwrap(),
                    "--lab-bin",
                    lab,
                ])
                .output()
                .expect("attest.ps1");
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
    } else {
        // No PowerShell: validate script exists + same cases via mmd-lab CLI.
        for (name, expect_ok, verdict) in cases {
            let out = attest(&fixture(name));
            let ok = out.status.success();
            assert_eq!(
                ok,
                expect_ok,
                "fixture {name} (cli fallback): ok={ok} expect={expect_ok}\nstdout={}\nstderr={}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(stdout_verdict(&out), verdict, "fixture {name}");
        }
    }
}
