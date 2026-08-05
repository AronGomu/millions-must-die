//! T24 exact-hash 3-host merge gate — CLI contract tests (fixture scope).

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

fn run_validate(extra: &[&str]) -> Output {
    let root = candidate_root();
    let out = lab_bin()
        .args([
            "validate",
            "--mode",
            "local-dev",
            "--commit",
            "deadbeefcafe",
            "--skip-self-check",
            "--root",
        ])
        .arg(root.path())
        .args(extra)
        .output()
        .expect("run mmd-lab validate");
    drop(root);
    out
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stdout_field(out: &Output, key: &str) -> String {
    stdout(out)
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{key} ")).map(str::to_string))
        .unwrap_or_else(|| panic!("missing stdout field {key}"))
}

#[test]
fn merge_gate_all_pass_allows_exact_hash() {
    let out = run_validate(&[]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "stdout:\n{text}");
    let hash = stdout_field(&out, "archive_sha256");
    assert_eq!(hash.len(), 64);
    assert!(text.contains("## mmd-lab 3-host merge gate"), "{text}");
    assert!(text.contains("- decision: **merge-exact-hash**"), "{text}");
    assert!(text.contains("- overall: **pass**"), "{text}");
    // Exact hash + commit bound into the summary.
    assert!(
        text.contains(&format!("- archive_sha256: `{hash}`")),
        "{text}"
    );
    assert!(
        text.contains(&format!("_Owner merges exactly archive `{hash}`")),
        "{text}"
    );
    assert!(text.contains("- git_commit: `deadbeefcafe`"), "{text}");
    // All three native lanes collected.
    for host in ["ubuntu-ref", "windows-ref", "macos-ref"] {
        assert!(text.contains(host), "missing {host}:\n{text}");
    }
    // Required nonblocking scale evidence recorded for every lane.
    for count in ["1000", "10000", "100000"] {
        assert!(
            text.contains(&format!("| ubuntu | {count} |")),
            "missing ubuntu {count} row:\n{text}"
        );
    }
    assert!(text.contains("deferred-hw"), "{text}");
}

#[test]
fn merge_gate_summary_is_deterministic() {
    let a = run_validate(&[]);
    let b = run_validate(&[]);
    assert_eq!(a.status.code(), Some(0));
    assert_eq!(b.status.code(), Some(0));
    let summary = |o: &Output| {
        let s = stdout(o);
        // Identical tempdir content ⇒ identical archive hash, so the full
        // summary must match byte-for-byte. Only the run-dependent
        // coordinator_sha256 trailer is stripped.
        s[s.find("## mmd-lab 3-host merge gate").unwrap()..]
            .lines()
            .take_while(|l| !l.starts_with("coordinator_sha256"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(summary(&a), summary(&b), "summary must be deterministic");
}

#[test]
fn merge_gate_one_lane_50k_miss_blocks() {
    let fixture = workspace_root().join("lab/fixtures/ubuntu-candidate/evidence-slow-p99.json");
    let out = run_validate(&["--ubuntu-evidence-fixture", fixture.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("- decision: **blocked**"), "{text}");
    assert!(text.contains("50k median p99"), "{text}");
    // Fail-fast false: the other lanes were still collected and reported.
    assert!(text.contains("windows-ref"), "{text}");
    assert!(text.contains("macos-ref"), "{text}");
}

#[test]
fn merge_gate_forged_lane_summary_blocks() {
    let fixture =
        workspace_root().join("lab/fixtures/macos-candidate/evidence-tampered-stats.json");
    let out = run_validate(&["--macos-evidence-fixture", fixture.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("- decision: **blocked**"), "{text}");
    assert!(text.contains("claimed stats mismatch"), "{text}");
}

#[test]
fn merge_gate_writes_summary_and_retains_evidence() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("ok.txt"), b"candidate").unwrap();
    let summary_path = dir.path().join("pr.md");
    let retain = dir.path().join("retain");
    let out = lab_bin()
        .args([
            "validate",
            "--mode",
            "local-dev",
            "--skip-self-check",
            "--root",
        ])
        .arg(&src)
        .args(["--summary-out"])
        .arg(&summary_path)
        .args(["--retain-dir"])
        .arg(&retain)
        .output()
        .unwrap();
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "stdout:\n{text}");
    let hash = stdout_field(&out, "archive_sha256");
    let summary = fs::read_to_string(&summary_path).unwrap();
    assert!(summary.contains(&hash), "summary must bind exact hash");
    let run_dir = retain.join(format!("sha256-{hash}"));
    assert!(run_dir.is_dir(), "missing retain dir");
    assert!(run_dir.join("pr_summary.md").is_file());
}
