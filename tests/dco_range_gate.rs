//! Offline DCO range gate behavioral tests for `scripts/check-dco`.
//!
//! Fixtures use throwaway TempDir git repos only — never the product worktree.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use tempfile::TempDir;

static COMMIT_SEQ: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        run_git(dir.path(), &["init", "-b", "main"]);
        run_git(dir.path(), &["config", "user.name", "DCO Tester"]);
        run_git(
            dir.path(),
            &["config", "user.email", "dco-tester@example.com"],
        );
        run_git(dir.path(), &["config", "commit.gpgsign", "false"]);
        Self { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn commit(&self, msg_body: &str) -> String {
        let n = COMMIT_SEQ.fetch_add(1, Ordering::Relaxed);
        let tracked = self.path().join("tracked.txt");
        std::fs::write(&tracked, format!("change-{n}\n")).expect("write tracked.txt");
        run_git(self.path(), &["add", "tracked.txt"]);
        let mut child = Command::new("git")
            .current_dir(self.path())
            .args([
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--cleanup=verbatim",
                "-F",
                "-",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn git commit");
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(msg_body.as_bytes())
            .expect("write commit msg");
        let out = child.wait_with_output().expect("wait git commit");
        assert!(
            out.status.success(),
            "git commit failed: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let sha = run_git_stdout(self.path(), &["rev-parse", "HEAD"]);
        sha.trim().to_string()
    }
}

fn run_git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn git {:?}: {e}", args));
    assert!(
        out.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run_git_stdout(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn git {:?}: {e}", args));
    assert!(
        out.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn signed_msg(subject: &str) -> String {
    format!("{subject}\n\nSigned-off-by: DCO Tester <dco-tester@example.com>\n")
}

fn run_check(fixture: &Path, base: &str, cand: &str) -> Output {
    let script = repo_root().join("scripts/check-dco");
    #[cfg(unix)]
    {
        Command::new(&script)
            .current_dir(fixture)
            .args([base, cand])
            .output()
            .unwrap_or_else(|e| panic!("spawn scripts/check-dco: {e}"))
    }
    #[cfg(not(unix))]
    {
        Command::new("bash")
            .arg(&script)
            .current_dir(fixture)
            .args([base, cand])
            .output()
            .unwrap_or_else(|e| {
                panic!(
                    "spawn bash scripts/check-dco failed: {e}; \
                     bash must be on PATH to run dco_range_gate tests on non-unix"
                )
            })
    }
}

fn run_check_args(fixture: &Path, args: &[&str]) -> Output {
    let script = repo_root().join("scripts/check-dco");
    #[cfg(unix)]
    {
        Command::new(&script)
            .current_dir(fixture)
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("spawn scripts/check-dco: {e}"))
    }
    #[cfg(not(unix))]
    {
        Command::new("bash")
            .arg(&script)
            .current_dir(fixture)
            .args(args)
            .output()
            .unwrap_or_else(|e| {
                panic!(
                    "spawn bash scripts/check-dco failed: {e}; \
                     bash must be on PATH to run dco_range_gate tests on non-unix"
                )
            })
    }
}

fn stderr_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn script_is_tracked_and_executable() {
    let script = repo_root().join("scripts/check-dco");
    assert!(script.is_file(), "scripts/check-dco must exist as file");

    let out = Command::new("git")
        .args([
            "-C",
            repo_root().to_str().expect("utf8 path"),
            "ls-files",
            "-s",
            "--",
            "scripts/check-dco",
        ])
        .output()
        .expect("git ls-files");
    assert!(out.status.success(), "git ls-files failed");
    let line = String::from_utf8_lossy(&out.stdout);
    let line = line.lines().next().unwrap_or("");
    assert!(
        line.starts_with("100755") && line.contains("scripts/check-dco"),
        "expected index mode 100755 for scripts/check-dco, got: {line:?}"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&script)
            .expect("metadata")
            .permissions()
            .mode();
        assert!(mode & 0o100 != 0, "owner-exec bit missing, mode={mode:#o}");
    }
}

#[test]
fn signed_range_exits_zero() {
    let fx = Fixture::new();
    let root = fx.commit(&signed_msg("signed root"));
    let child = fx.commit(&signed_msg("signed child"));
    let out = run_check(fx.path(), &root, &child);
    assert_eq!(out.status.code(), Some(0), "stderr={}", stderr_str(&out));
}

#[test]
fn unsigned_commit_exits_nonzero_and_names_hash() {
    let fx = Fixture::new();
    let root = fx.commit(&signed_msg("signed root"));
    let child = fx.commit("unsigned child\n");
    let out = run_check(fx.path(), &root, &child);
    assert_eq!(out.status.code(), Some(1), "stderr={}", stderr_str(&out));
    let err = stderr_str(&out);
    assert!(
        err.contains("missing Signed-off-by:"),
        "stderr missing marker: {err}"
    );
    assert!(err.contains(&child), "stderr missing child sha: {err}");
}

#[test]
fn mixed_range_reports_only_unsigned() {
    let fx = Fixture::new();
    let root = fx.commit(&signed_msg("signed root"));
    let mid = fx.commit("unsigned mid\n");
    let tip = fx.commit(&signed_msg("signed tip"));
    let out = run_check(fx.path(), &root, &tip);
    assert_eq!(out.status.code(), Some(1), "stderr={}", stderr_str(&out));
    let err = stderr_str(&out);
    assert!(err.contains(&mid), "stderr must name mid sha: {err}");
    // tip/root need not be named as missing
    let _ = tip;
}

#[test]
fn trailer_without_email_rejected() {
    let fx = Fixture::new();
    let root = fx.commit(&signed_msg("signed root"));
    let bad = fx.commit("no email\n\nSigned-off-by: nobody\n");
    let out = run_check(fx.path(), &root, &bad);
    assert_eq!(out.status.code(), Some(1), "stderr={}", stderr_str(&out));
    let err = stderr_str(&out);
    assert!(err.contains("missing Signed-off-by:"), "stderr={err}");
    assert!(err.contains(&bad), "stderr missing sha: {err}");
}

#[test]
fn wrong_key_trailer_rejected() {
    let fx = Fixture::new();
    let root = fx.commit(&signed_msg("signed root"));

    let acked = fx.commit("acked only\n\nAcked-by: DCO Tester <dco-tester@example.com>\n");
    let out_a = run_check(fx.path(), &root, &acked);
    assert_eq!(
        out_a.status.code(),
        Some(1),
        "Acked-by only should fail: stderr={}",
        stderr_str(&out_a)
    );
    let err_a = stderr_str(&out_a);
    assert!(err_a.contains("missing Signed-off-by:"), "stderr={err_a}");
    assert!(err_a.contains(&acked), "stderr missing acked sha: {err_a}");

    // Fresh base for plural spoof child from root via reset
    run_git(fx.path(), &["checkout", "--detach", &root]);
    run_git(fx.path(), &["checkout", "-B", "spoof"]);
    let plural = fx.commit("plural spoof\n\nSigned-off-bys: DCO Tester <dco-tester@example.com>\n");
    let out_b = run_check(fx.path(), &root, &plural);
    assert_eq!(
        out_b.status.code(),
        Some(1),
        "Signed-off-bys only should fail: stderr={}",
        stderr_str(&out_b)
    );
    let err_b = stderr_str(&out_b);
    assert!(err_b.contains("missing Signed-off-by:"), "stderr={err_b}");
    assert!(
        err_b.contains(&plural),
        "stderr missing plural sha: {err_b}"
    );
}

#[test]
fn lowercase_signed_off_by_key_accepted() {
    let fx = Fixture::new();
    let root = fx.commit(&signed_msg("signed root"));
    let child = fx.commit("lower key\n\nsigned-off-by: DCO Tester <dco-tester@example.com>\n");
    let out = run_check(fx.path(), &root, &child);
    assert_eq!(out.status.code(), Some(0), "stderr={}", stderr_str(&out));
}

#[test]
fn non_descendant_candidate_exits_nonzero() {
    let fx = Fixture::new();
    let root = fx.commit(&signed_msg("signed root"));

    run_git(fx.path(), &["checkout", "-b", "other"]);
    let b = fx.commit(&signed_msg("sibling B"));
    run_git(fx.path(), &["checkout", "main"]);
    let a = fx.commit(&signed_msg("sibling A"));

    let out = run_check(fx.path(), &a, &b);
    assert_eq!(out.status.code(), Some(1), "stderr={}", stderr_str(&out));
    let err = stderr_str(&out);
    assert!(
        err.contains("error: candidate is not a descendant of trusted base"),
        "stderr={err}"
    );
    assert!(err.contains(&a), "stderr missing A: {err}");
    assert!(err.contains(&b), "stderr missing B: {err}");
    let _ = root;
}

#[test]
fn equal_base_and_candidate_exits_zero() {
    let fx = Fixture::new();
    let sha = fx.commit(&signed_msg("solo"));
    let out = run_check(fx.path(), &sha, &sha);
    assert_eq!(out.status.code(), Some(0), "stderr={}", stderr_str(&out));
}

#[test]
fn usage_without_args_exits_two() {
    let fx = Fixture::new();
    let _ = fx.commit(&signed_msg("solo"));
    let out = run_check_args(fx.path(), &[]);
    assert_eq!(out.status.code(), Some(2), "stderr={}", stderr_str(&out));
    let err = stderr_str(&out);
    assert!(err.contains("usage:"), "stderr={err}");
    assert!(err.contains("TRUSTED_BASE_SHA"), "stderr={err}");
    assert!(err.contains("EXACT_CANDIDATE_SHA"), "stderr={err}");
}

#[test]
fn unresolvable_base_exits_two() {
    let fx = Fixture::new();
    let cand = fx.commit(&signed_msg("solo"));
    let out = run_check(fx.path(), "not-a-real-sha", &cand);
    assert_eq!(out.status.code(), Some(2), "stderr={}", stderr_str(&out));
    let err = stderr_str(&out);
    assert!(err.contains("trusted-base-sha"), "stderr={err}");
}
