use std::process::Command;

fn app_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_millions_must_die"))
}

#[test]
fn app_help_lists_run_and_bench() {
    let output = app_bin().arg("--help").output().expect("run app --help");
    assert!(
        output.status.success(),
        "app --help exit nonzero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run"), "help missing run:\n{stdout}");
    assert!(stdout.contains("bench"), "help missing bench:\n{stdout}");
}

#[test]
fn unknown_subcommand_fails() {
    let output = app_bin().arg("wat").output().expect("run app wat");
    assert!(!output.status.success(), "unknown subcommand must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stderr}{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        combined.to_ascii_lowercase().contains("usage")
            || combined.contains("unrecognized")
            || combined.contains("error"),
        "expected usage/error output:\n{combined}"
    );
}

#[test]
fn workspace_metadata_has_expected_members() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata = String::from_utf8_lossy(&output.stdout);
    for name in ["millions_must_die", "mmd-engine", "mmd-lab", "xtask"] {
        assert!(
            metadata.contains(&format!("\"name\":\"{name}\"")),
            "workspace metadata missing package {name}"
        );
    }
}
