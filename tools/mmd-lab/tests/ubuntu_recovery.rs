//! T16 Ubuntu recovery protocol — state machine + dry-run scripts.

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
    workspace_root().join("lab/provision/ubuntu/image-manifest.toml")
}

fn runner_manifest() -> PathBuf {
    workspace_root().join("lab/manifests/ubuntu-24.04-x86_64.toml")
}

fn pass_fixture() -> PathBuf {
    workspace_root().join("lab/fixtures/ubuntu-attest/pass.json")
}

fn wrong_image_fixture() -> PathBuf {
    workspace_root().join("lab/fixtures/ubuntu-attest/wrong-image.json")
}

fn recover_sh() -> PathBuf {
    workspace_root().join("lab/provision/ubuntu/recover.sh")
}

fn stdout_contains(out: &std::process::Output, needle: &str) -> bool {
    String::from_utf8_lossy(&out.stdout).contains(needle)
}

#[test]
fn restore_digest_mismatch_quarantines_via_cli_fixture_path() {
    // Attest fixture with wrong image after simulated restore → quarantine.
    let out = lab_bin()
        .args([
            "ubuntu-recover-simulate",
            "--image-manifest",
        ])
        .arg(image_manifest())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(wrong_image_fixture())
        .output()
        .expect("simulate");
    assert!(
        !out.status.success(),
        "wrong image attest must fail protocol; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout_contains(&out, "quarantine") || stdout_contains(&out, "image digest"),
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
fn stale_host_key_fails_via_cli() {
    let out = lab_bin()
        .args([
            "ubuntu-recover-simulate",
            "--image-manifest",
        ])
        .arg(image_manifest())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(pass_fixture())
        .args([
            "--prior-host-key-fp",
            "ssh-ed25519 AAAAsame",
            "--new-host-key-fp",
            "ssh-ed25519 AAAAsame",
        ])
        .output()
        .expect("simulate");
    assert!(!out.status.success(), "stale key must fail");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("stale host key") || stdout.contains("quarantine"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("allows_candidate_provision false"));
}

#[test]
fn egress_canary_blocks_unit_covered_in_lib() {
    // CLI dry-run always injects denied=true; unit module covers denied=false.
    // Ensure happy-path dry-run records egress deny trail.
    let out = lab_bin()
        .args(["ubuntu-recover-simulate", "--image-manifest"])
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
fn recover_sh_dry_run_matrix() {
    let script = recover_sh();
    assert!(script.is_file(), "missing {}", script.display());
    let lab = env!("CARGO_BIN_EXE_mmd-lab");

    let out = Command::new("bash")
        .arg(&script)
        .args([
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
        .expect("recover.sh");
    assert!(
        out.status.success(),
        "recover.sh dry-run failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout_contains(&out, "ready-for-candidate"));

    // Live without dry-run must refuse (no physical controller).
    let live = Command::new("bash")
        .arg(&script)
        .arg("--lab-bin")
        .arg(lab)
        .output()
        .expect("recover.sh live");
    assert_eq!(live.status.code(), Some(2), "live must exit 2");
}

#[test]
fn doctor_runner_ubuntu_reports_blocked_user() {
    let out = lab_bin()
        .args(["doctor", "--runner", "ubuntu"])
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
    assert!(stdout.contains("ok lab/provision/ubuntu/recover.sh"), "stdout={stdout}");
    assert!(
        stdout.contains("ok lab/provision/ubuntu/image-manifest.toml"),
        "stdout={stdout}"
    );
}

#[test]
fn image_manifest_files_exist() {
    assert!(image_manifest().is_file());
    assert!(Path::new(&recover_sh()).is_file());
    assert!(workspace_root()
        .join("docs/lab/ubuntu-runner.md")
        .is_file());
}
