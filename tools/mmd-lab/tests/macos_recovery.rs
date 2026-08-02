//! T22 macOS recovery protocol — state machine + dry-run scripts.

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

fn mdm_profile() -> PathBuf {
    workspace_root().join("lab/provision/macos/mdm-profile.example.json")
}

fn runner_manifest() -> PathBuf {
    workspace_root().join("lab/manifests/macos-15-arm64.toml")
}

fn pass_fixture() -> PathBuf {
    workspace_root().join("lab/fixtures/macos-attest/pass.json")
}

fn missing_mdm_fixture() -> PathBuf {
    workspace_root().join("lab/fixtures/macos-attest/missing-mdm.json")
}

fn invalid_ssv_fixture() -> PathBuf {
    workspace_root().join("lab/fixtures/macos-attest/invalid-ssv.json")
}

fn recover_sh() -> PathBuf {
    workspace_root().join("lab/provision/macos/recover.sh")
}

fn stdout_contains(out: &std::process::Output, needle: &str) -> bool {
    String::from_utf8_lossy(&out.stdout).contains(needle)
}

#[test]
fn missed_eacs_ack_quarantines_via_cli() {
    let out = lab_bin()
        .args(["macos-recover-simulate", "--mdm-profile"])
        .arg(mdm_profile())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(pass_fixture())
        .args(["--path", "eacs", "--eacs-ack", "false"])
        .output()
        .expect("simulate");
    assert!(
        !out.status.success(),
        "missed eacs ack must fail; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("missed eacs ack") || stdout.contains("quarantine"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("allows_candidate_provision false"));
}

#[test]
fn reenroll_failure_quarantines_via_cli() {
    let out = lab_bin()
        .args(["macos-recover-simulate", "--mdm-profile"])
        .arg(mdm_profile())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(pass_fixture())
        .args(["--path", "eacs", "--reenroll-success", "false"])
        .output()
        .expect("simulate");
    assert!(!out.status.success(), "reenroll fail must fail protocol");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("reenroll failure") || stdout.contains("quarantine"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("allows_candidate_provision false"));
}

#[test]
fn mac_candidate_egress_blocks_unit_covered_in_lib() {
    // CLI dry-run always injects denied=true; unit module covers denied=false.
    let out = lab_bin()
        .args(["macos-recover-simulate", "--mdm-profile"])
        .arg(mdm_profile())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(pass_fixture())
        .args(["--path", "eacs"])
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
    assert!(stdout.contains("reset_path eacs") || stdout.contains("eacs"));
}

#[test]
fn dfu_fallback_path_ready_via_cli() {
    let out = lab_bin()
        .args(["macos-recover-simulate", "--mdm-profile"])
        .arg(mdm_profile())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(pass_fixture())
        .args(["--path", "dfu"])
        .output()
        .expect("simulate dfu");
    assert!(
        out.status.success(),
        "dfu drill failed: stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ready-for-candidate"), "stdout={stdout}");
    assert!(
        stdout.contains("dfu") || stdout.contains("reset_path dfu"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("allows_candidate_provision true"));
}

#[test]
fn missing_mdm_attest_fixture_quarantines_via_cli() {
    let out = lab_bin()
        .args(["macos-recover-simulate", "--mdm-profile"])
        .arg(mdm_profile())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(missing_mdm_fixture())
        .args(["--path", "eacs"])
        .output()
        .expect("simulate");
    assert!(
        !out.status.success(),
        "missing mdm attest must fail; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout_contains(&out, "allows_candidate_provision false"));
}

#[test]
fn invalid_ssv_attest_fixture_quarantines_via_cli() {
    let out = lab_bin()
        .args(["macos-recover-simulate", "--mdm-profile"])
        .arg(mdm_profile())
        .arg("--runner-manifest")
        .arg(runner_manifest())
        .arg("--attest-fixture")
        .arg(invalid_ssv_fixture())
        .args(["--path", "eacs"])
        .output()
        .expect("simulate");
    assert!(!out.status.success());
    assert!(stdout_contains(&out, "allows_candidate_provision false"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("ssv") || stdout.contains("quarantine"),
        "stdout={stdout}"
    );
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
            "--mdm-profile",
            mdm_profile().to_str().unwrap(),
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

    let dfu = Command::new("bash")
        .arg(&script)
        .args([
            "--dry-run",
            "--path",
            "dfu",
            "--lab-bin",
            lab,
            "--mdm-profile",
            mdm_profile().to_str().unwrap(),
            "--runner-manifest",
            runner_manifest().to_str().unwrap(),
            "--attest-fixture",
            pass_fixture().to_str().unwrap(),
        ])
        .output()
        .expect("recover.sh dfu");
    assert!(
        dfu.status.success(),
        "recover.sh dfu dry-run failed: stdout={} stderr={}",
        String::from_utf8_lossy(&dfu.stdout),
        String::from_utf8_lossy(&dfu.stderr)
    );
    assert!(stdout_contains(&dfu, "ready-for-candidate"));

    // Live without dry-run must refuse (no physical controller / MDM).
    let live = Command::new("bash")
        .arg(&script)
        .arg("--lab-bin")
        .arg(lab)
        .output()
        .expect("recover.sh live");
    assert_eq!(live.status.code(), Some(2), "live must exit 2");
}

#[test]
fn doctor_runner_macos_reports_blocked_user() {
    let out = lab_bin()
        .args(["doctor", "--runner", "macos"])
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
        stdout.contains("ok lab/provision/macos/recover.sh"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("ok lab/provision/macos/mdm-profile.example.json"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("TODO(user)") || stdout.contains("mdm"),
        "stdout={stdout}"
    );
}

#[test]
fn recovery_files_exist() {
    assert!(mdm_profile().is_file());
    assert!(Path::new(&recover_sh()).is_file());
    assert!(workspace_root()
        .join("docs/lab/macos-runner.md")
        .is_file());
}
