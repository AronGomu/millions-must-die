use std::process::Command;

fn lab_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mmd-lab"))
}

#[test]
fn lab_help_lists_core_commands() {
    let output = lab_bin().arg("--help").output().expect("run lab --help");
    assert!(
        output.status.success(),
        "lab --help exit nonzero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for cmd in [
        "doctor",
        "attest-ubuntu",
        "attest-windows",
        "attest-macos",
        "ubuntu-recover-simulate",
        "windows-recover-simulate",
        "install",
        "self-check",
        "archive",
        "validate",
    ] {
        assert!(stdout.contains(cmd), "help missing {cmd}:\n{stdout}");
    }
}
