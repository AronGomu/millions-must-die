//! Deterministic PR summary bound to exact source archive hash.

use mmd_engine::bench::VerdictStatus;

use crate::verify::MatrixVerdict;

/// Validation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidateMode {
    /// PR / merge gate: dirty worktree rejected.
    Pr,
    /// Explicit local experimentation.
    LocalDev,
}

impl ValidateMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pr" => Some(Self::Pr),
            "local-dev" => Some(Self::LocalDev),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pr => "pr",
            Self::LocalDev => "local-dev",
        }
    }
}

/// Render concise owner PR summary. Always includes exact archive SHA-256.
pub fn render_pr_summary(
    mode: ValidateMode,
    git_commit: Option<&str>,
    dirty_worktree: bool,
    matrix: &MatrixVerdict,
) -> String {
    let mut lines = Vec::new();
    lines.push("## mmd-lab validation".to_string());
    lines.push(format!("- mode: `{}`", mode.as_str()));
    if let Some(c) = git_commit {
        lines.push(format!("- git_commit: `{c}`"));
    } else {
        lines.push("- git_commit: `(none)`".into());
    }
    lines.push(format!("- archive_sha256: `{}`", matrix.archive_sha256));
    lines.push(format!("- dirty_worktree: {dirty_worktree}"));
    lines.push(format!(
        "- overall: **{}**",
        verdict_label(matrix.overall)
    ));
    lines.push(format!("- reason: {}", matrix.reason));
    lines.push(String::new());
    lines.push("| host | platform | p95_ms | p99_ms | claimed_ok | verdict |".into());
    lines.push("| --- | --- | ---: | ---: | --- | --- |".into());
    for h in &matrix.hosts {
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} |",
            h.host_id,
            h.platform,
            fmt_f64(h.recomputed_median_p95_ms),
            fmt_f64(h.recomputed_median_p99_ms),
            h.claimed_stats_match,
            verdict_label(h.verdict),
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "_Coordinator-verified evidence. Host/source identity may be attested; perf truth is recomputed from raw samples. archive=`{}`_",
        matrix.archive_sha256
    ));
    lines.join("\n")
}

fn verdict_label(v: VerdictStatus) -> &'static str {
    match v {
        VerdictStatus::Pass => "pass",
        VerdictStatus::Fail => "fail",
        VerdictStatus::Inconclusive => "inconclusive",
        VerdictStatus::Recorded => "recorded",
        VerdictStatus::Error => "error",
    }
}

fn fmt_f64(x: f64) -> String {
    if x.is_finite() {
        format!("{x:.4}")
    } else {
        "n/a".into()
    }
}

/// Local retention layout under retain root.
pub fn retain_run_dir(retain_root: &std::path::Path, archive_sha256: &str) -> std::path::PathBuf {
    retain_root.join(format!("sha256-{archive_sha256}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::{HostVerdict, MatrixVerdict};

    #[test]
    fn summary_includes_exact_hash() {
        let hash = "abc123".to_string() + &"d".repeat(58);
        let matrix = MatrixVerdict {
            archive_sha256: hash.clone(),
            hosts: vec![HostVerdict {
                host_id: "ubuntu-ref".into(),
                platform: "linux-x86_64".into(),
                archive_ok: true,
                remote_verified: true,
                recomputed_median_p95_ms: 10.0,
                recomputed_median_p99_ms: 12.0,
                nmad_p95: 0.01,
                nmad_p99: 0.01,
                claimed_stats_match: true,
                claimed: None,
                verdict: VerdictStatus::Pass,
                reason: "ok".into(),
            }],
            overall: VerdictStatus::Pass,
            reason: "all pass".into(),
        };
        let s = render_pr_summary(ValidateMode::Pr, Some("deadbeef"), false, &matrix);
        assert!(s.contains(&hash), "summary missing hash:\n{s}");
        assert!(s.contains("deadbeef"));
        assert!(s.contains("ubuntu-ref"));
    }
}
