use std::process::Command;

fn lab_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mmd-lab"))
}

#[test]
fn lab_help_lists_doctor_and_validate() {
    let output = lab_bin().arg("--help").output().expect("run lab --help");
    assert!(
        output.status.success(),
        "lab --help exit nonzero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor"), "help missing doctor:\n{stdout}");
    assert!(
        stdout.contains("validate"),
        "help missing validate:\n{stdout}"
    );
}
