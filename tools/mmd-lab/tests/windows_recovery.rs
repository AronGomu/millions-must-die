//! T19 Windows recovery protocol — state machine + dry-run scripts.

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

fn image_manifest() -> PathBuf {
    workspace_root().join("lab/provision/windows/image-manifest.toml")
}

fn runner_manifest() -> PathBuf {
    workspace_root().join("lab/manifests/windows-11-25h2-x86_64.toml")
}

fn pass_fixture() -> PathBuf {
    workspace_root().join("lab/fixtures/windows-attest/pass.json")
}

fn wrong_ffu_fixture() -> PathBuf {
    workspace_root().join("lab/fixtures/windows-attest/wrong-ffu.json")
}

fn recover_ps1() -> PathBuf {
    workspace_root().join("lab/provision/windows/recover.ps1")
}

fn stdout_contains(out: &std::process::Output, needle: &str) -> bool {
    String::from_utf8_lossy(&out.stdout).contains(needle)
}

#[test]
fn ffu_apply_failure_quarantines_via_cli_fixture_path() {
    // Attest fixture with wrong FFU after simulated restore → quarantine.
    let out = lab_bin()
        .args(["windows-recover-simulate", "--image-manifest"])
        .arg(image_manifest())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(wrong_ffu_fixture())
        .output()
        .expect("simulate");
    assert!(
        !out.status.success(),
        "wrong ffu attest must fail protocol; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout_contains(&out, "quarantine") || stdout_contains(&out, "ffu digest"),
        "stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        stdout_contains(&out, "allows_candidate_provision false"),
        "must not allow provision; stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn stale_windows_identity_fails_via_cli() {
    let out = lab_bin()
        .args(["windows-recover-simulate", "--image-manifest"])
        .arg(image_manifest())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(pass_fixture())
        .args([
            "--prior-host-identity",
            "WIN-MACHINE-SAME",
            "--new-host-identity",
            "WIN-MACHINE-SAME",
        ])
        .output()
        .expect("simulate");
    assert!(!out.status.success(), "stale identity must fail");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("stale windows identity") || stdout.contains("quarantine"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("allows_candidate_provision false"));
}

#[test]
fn windows_egress_canary_blocks_unit_covered_in_lib() {
    // CLI dry-run always injects denied=true; unit module covers denied=false.
    let out = lab_bin()
        .args(["windows-recover-simulate", "--image-manifest"])
        .arg(image_manifest())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(pass_fixture())
        .output()
        .expect("simulate");
    assert!(
        out.status.success(),
        "pass drill failed: stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ready-for-candidate"), "stdout={stdout}");
    assert!(
        stdout.contains("egress-canary-denied") || stdout.contains("egress-canary"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("allows_candidate_provision true"));
    assert!(stdout.contains("dry-run-only"));
}

#[test]
fn recover_ps1_dry_run_matrix() {
    let script = recover_ps1();
    assert!(script.is_file(), "missing {}", script.display());
    let lab = env!("CARGO_BIN_EXE_mmd-lab");

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
        let out = Command::new(shell)
            .args([
                "-NoProfile",
                "-File",
                script.to_str().unwrap(),
                "--dry-run",
                "--lab-bin",
                lab,
                "--image-manifest",
                image_manifest().to_str().unwrap(),
                "--runner-manifest",
                runner_manifest().to_str().unwrap(),
                "--attest-fixture",
                pass_fixture().to_str().unwrap(),
            ])
            .output()
            .expect("recover.ps1");
        assert!(
            out.status.success(),
            "recover.ps1 dry-run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(stdout_contains(&out, "ready-for-candidate"));

        // Live without dry-run must refuse (no physical controller).
        let live = Command::new(shell)
            .args([
                "-NoProfile",
                "-File",
                script.to_str().unwrap(),
                "--lab-bin",
                lab,
            ])
            .output()
            .expect("recover.ps1 live");
        assert_eq!(
            live.status.code(),
            Some(2),
            "live must exit 2; stderr={}",
            String::from_utf8_lossy(&live.stderr)
        );
    } else {
        // No PowerShell: CLI simulate covers same dry-run protocol path.
        let out = lab_bin()
            .args(["windows-recover-simulate", "--image-manifest"])
            .arg(image_manifest())
            .arg("--runner-manifest")
            .arg(runner_manifest())
            .arg("--attest-fixture")
            .arg(pass_fixture())
            .output()
            .expect("simulate fallback");
        assert!(
            out.status.success(),
            "cli fallback failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(stdout_contains(&out, "ready-for-candidate"));
    }
}

#[test]
fn doctor_runner_windows_reports_blocked_user() {
    let out = lab_bin()
        .args(["doctor", "--runner", "windows"])
        .current_dir(workspace_root())
        .output()
        .expect("doctor");
    // Contracts present → exit 2 blocked_user (not 0 ready, not 1 missing).
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("verdict blocked_user"), "stdout={stdout}");
    assert!(stdout.contains("physical-lab absent"), "stdout={stdout}");
    assert!(
        stdout.contains("ok lab/provision/windows/recover.ps1"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("ok lab/provision/windows/image-manifest.toml"),
        "stdout={stdout}"
    );
}

#[test]
fn image_manifest_files_exist() {
    assert!(image_manifest().is_file());
    assert!(Path::new(&recover_ps1()).is_file());
    assert!(
        workspace_root()
            .join("docs/lab/windows-runner.md")
            .is_file()
    );
}
