//! T24 exact-hash 3-host merge gate. Aggregates the three candidate lanes
//! (T17 Ubuntu/Vulkan, T20 Windows/D3D12, T23 macOS/Metal) into one
//! coordinator-side merge verdict.
//!
//! Contract (plan T24):
//! - Runs on every merge and every path; no path filters exist.
//! - Fail-fast is false: ALL lanes are collected and every reason is
//!   reported before the verdict.
//! - Coordinator self-check is mandatory before dispatch (enforced by the
//!   CLI entry; dev tests may skip explicitly).
//! - Host/source identity is attested; the coordinator independently
//!   recomputes the policy verdict from raw evidence here — a lane- or
//!   candidate-reported pass can never override the recompute.
//! - 50k blocks. 1k/10k/100k evidence is required but nonblocking: the
//!   samples must exist, their values never gate.
//! - The owner merges exactly the tested archive hash, nothing else.
//!
//! Fixture/fake-transport scope: the three native lane runs and physical
//! reset/run/reset cycles are deferred-hw; the aggregate protocol below is
//! the trusted-side contract either way.

use mmd_engine::bench::{BenchPolicy, GATE_AGENT_COUNT, SCALE_COUNTS, VerdictStatus};

use crate::report::{HostEvidence, HostManifest};
use crate::verify::{HostVerdict, recompute_count_aggregate, verify_host, worse};

/// Native lane identity. All three are required for every merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LaneId {
    Ubuntu,
    Windows,
    Macos,
}

impl LaneId {
    /// Required lane set, in deterministic report order.
    pub const ALL: [LaneId; 3] = [LaneId::Ubuntu, LaneId::Windows, LaneId::Macos];

    pub fn as_str(self) -> &'static str {
        match self {
            LaneId::Ubuntu => "ubuntu",
            LaneId::Windows => "windows",
            LaneId::Macos => "macos",
        }
    }
}

/// One collected candidate lane result (coordinator-side; lane adapters in
/// `{ubuntu,windows,macos}_gate.rs` produce the verdict/golden/reset fields).
#[derive(Debug, Clone)]
pub struct LaneRun {
    pub lane: LaneId,
    /// Lane-recomputed verdict (never candidate-reported).
    pub verdict: VerdictStatus,
    /// Golden diff recomputed from raw readback evidence.
    pub golden_ok: bool,
    /// Post-run external restore was initiated (protocol-level).
    pub post_run_reset_started: bool,
    /// Non-pass reasons from the lane, in lane order.
    pub reasons: Vec<String>,
    /// Raw evidence the coordinator collected for this lane.
    pub evidence: HostEvidence,
}

/// Explicit merge decision. This is the only output the owner acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeDecision {
    /// Every lane passed every gate on the same exact source hash: the owner
    /// may merge exactly that hash.
    MergeExactHash,
    /// Anything else. No merge; reasons list every collected failure.
    Blocked,
}

impl MergeDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            MergeDecision::MergeExactHash => "merge-exact-hash",
            MergeDecision::Blocked => "blocked",
        }
    }
}

/// Per-lane aggregate row (recompute already folded in).
#[derive(Debug, Clone)]
pub struct LaneVerdict {
    pub lane: LaneId,
    pub host_id: String,
    pub platform: String,
    /// Worst of lane verdict and coordinator recompute.
    pub verdict: VerdictStatus,
    /// Coordinator recompute; `None` when the lane never executed.
    pub recomputed: Option<HostVerdict>,
    pub golden_ok: bool,
    pub post_run_reset_started: bool,
}

/// One nonblocking scale-evidence row (1k/10k/100k).
#[derive(Debug, Clone)]
pub struct ScaleRow {
    pub lane: LaneId,
    pub agent_count: u32,
    pub median_p95_ms: f64,
    pub median_p99_ms: f64,
}

/// Aggregate outcome over all lanes.
#[derive(Debug, Clone)]
pub struct MergeGateOutcome {
    pub decision: MergeDecision,
    pub overall: VerdictStatus,
    pub archive_sha256: String,
    pub lanes: Vec<LaneVerdict>,
    /// Recorded nonblocking evidence (deterministic lane-then-count order).
    pub scale_rows: Vec<ScaleRow>,
    /// Every collected reason across all lanes (fail-fast false).
    pub reasons: Vec<String>,
}

/// Aggregate all lanes into one merge decision. Collects everything before
/// deciding; never stops at the first failure.
pub fn aggregate_merge_gate(
    expected_archive_sha256: &str,
    lanes: &[LaneRun],
    policy: &BenchPolicy,
) -> MergeGateOutcome {
    let mut reasons: Vec<String> = Vec::new();
    let mut overall = VerdictStatus::Pass;
    let mut lane_verdicts: Vec<LaneVerdict> = Vec::new();
    let mut scale_rows: Vec<ScaleRow> = Vec::new();

    // Mixed-source detection across the lanes that did run. Undelivered
    // lanes (non-hex placeholder hashes) are excluded here — they surface as
    // per-lane archive mismatches via `verify_host` instead.
    let mut hashes: Vec<String> = lanes
        .iter()
        .map(|l| l.evidence.archive_sha256.to_ascii_lowercase())
        .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .collect();
    hashes.sort();
    hashes.dedup();
    if hashes.len() > 1 {
        overall = worse(overall, VerdictStatus::Fail);
        reasons.push(format!(
            "mixed source hashes across lanes: {} distinct archives; every lane must run exactly {expected_archive_sha256}",
            hashes.len()
        ));
    }

    for required in LaneId::ALL {
        let matched: Vec<&LaneRun> = lanes.iter().filter(|l| l.lane == required).collect();
        let run = match matched.as_slice() {
            [] => {
                overall = worse(overall, VerdictStatus::Fail);
                reasons.push(format!(
                    "required {} lane missing; every merge needs all native lanes",
                    required.as_str()
                ));
                lane_verdicts.push(LaneVerdict {
                    lane: required,
                    host_id: "(missing)".into(),
                    platform: "(missing)".into(),
                    verdict: VerdictStatus::Fail,
                    recomputed: None,
                    golden_ok: false,
                    post_run_reset_started: false,
                });
                continue;
            }
            [one] => *one,
            _ => {
                overall = worse(overall, VerdictStatus::Error);
                reasons.push(format!(
                    "duplicate {} lane runs in one aggregate; exactly one per lane",
                    required.as_str()
                ));
                // Fail-fast false: surface the extra runs' reasons too.
                for extra in &matched[1..] {
                    for r in &extra.reasons {
                        reasons.push(format!("{} duplicate lane: {r}", required.as_str()));
                    }
                }
                matched[0]
            }
        };

        let evidence = &run.evidence;

        // Stale manifest: evidence written under a schema this coordinator
        // does not currently trust blocks the merge.
        if evidence.schema_version != HostEvidence::SCHEMA
            || evidence.host_manifest.schema_version != HostManifest::SCHEMA
        {
            overall = worse(overall, VerdictStatus::Fail);
            reasons.push(format!(
                "{} lane stale manifest: evidence schema `{}` / host schema `{}` (expected `{}` / `{}`)",
                required.as_str(),
                evidence.schema_version,
                evidence.host_manifest.schema_version,
                HostEvidence::SCHEMA,
                HostManifest::SCHEMA
            ));
        }

        // Independent coordinator recompute — the lane verdict is folded in,
        // never substituted. A candidate/lane-reported pass cannot override.
        let recomputed = verify_host(expected_archive_sha256, evidence, policy);
        let folded = worse(run.verdict, recomputed.verdict);
        if recomputed.verdict != VerdictStatus::Pass {
            reasons.push(format!(
                "{} recompute: {}",
                required.as_str(),
                recomputed.reason
            ));
            if evidence
                .claimed
                .as_ref()
                .is_some_and(|c| c.verdict.eq_ignore_ascii_case("pass"))
            {
                reasons.push(format!(
                    "{} candidate-reported pass overridden by coordinator recompute",
                    required.as_str()
                ));
            }
        }
        for r in &run.reasons {
            reasons.push(format!("{} lane: {r}", required.as_str()));
        }
        // Fallback reasons keyed on the structured flags: lane adapters
        // always explain their own failures, so add these only when the
        // lane surfaced no reason text at all.
        if !run.golden_ok && run.reasons.is_empty() {
            reasons.push(format!("{} golden diff not verified", required.as_str()));
        }
        let mut lane_overall = folded;
        if !run.golden_ok {
            lane_overall = worse(lane_overall, VerdictStatus::Fail);
        }
        if !run.post_run_reset_started {
            lane_overall = worse(lane_overall, VerdictStatus::Error);
            if run.reasons.is_empty() {
                reasons.push(format!(
                    "{} post-run reset not started; run is not verified",
                    required.as_str()
                ));
            }
        }

        // Nonblocking scale evidence: required to exist, never gates on value.
        for count in SCALE_COUNTS {
            if count == GATE_AGENT_COUNT {
                continue;
            }
            match recompute_count_aggregate(evidence, count) {
                Ok(agg) => scale_rows.push(ScaleRow {
                    lane: required,
                    agent_count: count,
                    median_p95_ms: agg.median_p95_ms,
                    median_p99_ms: agg.median_p99_ms,
                }),
                Err(_) => {
                    lane_overall = worse(lane_overall, VerdictStatus::Fail);
                    reasons.push(format!(
                        "{} missing required nonblocking scale evidence at {count} agents",
                        required.as_str()
                    ));
                }
            }
        }

        overall = worse(overall, lane_overall);
        lane_verdicts.push(LaneVerdict {
            lane: required,
            host_id: evidence.host_manifest.host_id.clone(),
            platform: evidence.host_manifest.platform.clone(),
            verdict: lane_overall,
            recomputed: Some(recomputed),
            golden_ok: run.golden_ok,
            post_run_reset_started: run.post_run_reset_started,
        });
    }

    let decision = if overall == VerdictStatus::Pass {
        MergeDecision::MergeExactHash
    } else {
        MergeDecision::Blocked
    };
    MergeGateOutcome {
        decision,
        overall,
        archive_sha256: expected_archive_sha256.to_string(),
        lanes: lane_verdicts,
        scale_rows,
        reasons,
    }
}

/// Render the deterministic owner PR summary. Same inputs → byte-identical
/// markdown; always binds the exact archive hash.
pub fn render_merge_summary(
    mode: &str,
    git_commit: Option<&str>,
    dirty_worktree: bool,
    outcome: &MergeGateOutcome,
) -> String {
    let mut lines = Vec::new();
    lines.push("## mmd-lab 3-host merge gate".to_string());
    lines.push(format!("- mode: `{mode}`"));
    match git_commit {
        Some(c) => lines.push(format!("- git_commit: `{c}`")),
        None => lines.push("- git_commit: `(none)`".into()),
    }
    lines.push(format!("- archive_sha256: `{}`", outcome.archive_sha256));
    lines.push(format!("- dirty_worktree: {dirty_worktree}"));
    lines.push(format!("- decision: **{}**", outcome.decision.as_str()));
    lines.push(format!("- overall: **{}**", verdict_label(outcome.overall)));
    lines.push(String::new());
    lines.push("| lane | host | platform | p95_ms | p99_ms | golden | reset | verdict |".into());
    lines.push("| --- | --- | --- | ---: | ---: | --- | --- | --- |".into());
    for l in &outcome.lanes {
        let (p95, p99) = match &l.recomputed {
            Some(r) => (
                fmt_f64(r.recomputed_median_p95_ms),
                fmt_f64(r.recomputed_median_p99_ms),
            ),
            None => ("n/a".into(), "n/a".into()),
        };
        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            l.lane.as_str(),
            l.host_id,
            l.platform,
            p95,
            p99,
            l.golden_ok,
            l.post_run_reset_started,
            verdict_label(l.verdict),
        ));
    }
    lines.push(String::new());
    lines.push("### Scale evidence (recorded; only 50k blocks)".into());
    lines.push("| lane | agents | p95_ms | p99_ms | status |".into());
    lines.push("| --- | ---: | ---: | ---: | --- |".into());
    for s in &outcome.scale_rows {
        lines.push(format!(
            "| {} | {} | {} | {} | recorded |",
            s.lane.as_str(),
            s.agent_count,
            fmt_f64(s.median_p95_ms),
            fmt_f64(s.median_p99_ms),
        ));
    }
    if !outcome.reasons.is_empty() {
        lines.push(String::new());
        lines.push("### Blocking reasons".into());
        for r in &outcome.reasons {
            lines.push(format!("- {r}"));
        }
    }
    lines.push(String::new());
    lines.push(format!(
        "_Owner merges exactly archive `{}`; any other hash is untested. Coordinator-verified evidence; candidate claims are never trusted._",
        outcome.archive_sha256
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{ClaimedStats, HostManifest, RawTrialSamples};
    use crate::verify::recompute_gate_aggregate;

    const ARCHIVE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn evidence(id: &str, platform: &str, backend: &str, base_ms: f64) -> HostEvidence {
        let mut e = HostEvidence::new(
            HostManifest::new(id, platform, backend, "fixture-os"),
            ARCHIVE,
        );
        e.submitted_frames = 420;
        e.completed_frames = 420;
        e.max_in_flight = 2;
        e.project_rust_alloc_count = 0;
        for i in 0..7u32 {
            e.raw_trials.push(RawTrialSamples {
                agent_count: GATE_AGENT_COUNT,
                trial_index: i,
                frame_service_ms: vec![base_ms + f64::from(i) * 0.02; 128],
            });
        }
        // Required nonblocking scale evidence. 100k intentionally slow: it
        // must never gate.
        for count in SCALE_COUNTS {
            if count == GATE_AGENT_COUNT {
                continue;
            }
            let ms = match count {
                1_000 => 2.0,
                10_000 => 4.5,
                _ => 30.0,
            };
            e.raw_trials.push(RawTrialSamples {
                agent_count: count,
                trial_index: 0,
                frame_service_ms: vec![ms; 64],
            });
        }
        let agg = recompute_gate_aggregate(&e).unwrap();
        e.claimed = Some(ClaimedStats {
            median_p95_ms: agg.median_p95_ms,
            median_p99_ms: agg.median_p99_ms,
            verdict: "pass".into(),
        });
        e
    }

    fn pass_lane(lane: LaneId) -> LaneRun {
        let (id, platform, backend, base) = match lane {
            LaneId::Ubuntu => ("ubuntu-ref", "linux-x86_64", "vulkan", 10.0),
            LaneId::Windows => ("windows-ref", "windows-x86_64", "d3d12", 11.0),
            LaneId::Macos => ("macos-ref", "macos-arm64", "metal", 9.5),
        };
        LaneRun {
            lane,
            verdict: VerdictStatus::Pass,
            golden_ok: true,
            post_run_reset_started: true,
            reasons: Vec::new(),
            evidence: evidence(id, platform, backend, base),
        }
    }

    fn all_pass() -> Vec<LaneRun> {
        LaneId::ALL.map(pass_lane).to_vec()
    }

    fn aggregate(lanes: &[LaneRun]) -> MergeGateOutcome {
        aggregate_merge_gate(ARCHIVE, lanes, &BenchPolicy::production())
    }

    #[test]
    fn all_lanes_pass_allows_exact_hash_merge() {
        let outcome = aggregate(&all_pass());
        assert_eq!(outcome.overall, VerdictStatus::Pass, "{outcome:?}");
        assert_eq!(outcome.decision, MergeDecision::MergeExactHash);
        assert!(outcome.reasons.is_empty(), "{:?}", outcome.reasons);
        // 3 lanes × 3 nonblocking counts recorded.
        assert_eq!(outcome.scale_rows.len(), 9);
    }

    #[test]
    fn missing_os_lane_fails() {
        let lanes: Vec<LaneRun> = all_pass()
            .into_iter()
            .filter(|l| l.lane != LaneId::Macos)
            .collect();
        let outcome = aggregate(&lanes);
        assert_eq!(outcome.decision, MergeDecision::Blocked);
        assert_eq!(outcome.overall, VerdictStatus::Fail);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("required macos lane missing")),
            "{:?}",
            outcome.reasons
        );
        // Other lanes were still fully collected (fail-fast false).
        assert_eq!(outcome.lanes.len(), 3);
        assert_eq!(outcome.scale_rows.len(), 6);
    }

    #[test]
    fn mixed_hashes_fail() {
        let mut lanes = all_pass();
        lanes[1].evidence.archive_sha256 = "bb".repeat(32);
        let outcome = aggregate(&lanes);
        assert_eq!(outcome.decision, MergeDecision::Blocked);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("mixed source hashes")),
            "{:?}",
            outcome.reasons
        );
    }

    #[test]
    fn one_inconclusive_blocks() {
        let mut lanes = all_pass();
        // Noisy macos trials → lane recompute inconclusive.
        for (i, t) in lanes[2]
            .evidence
            .raw_trials
            .iter_mut()
            .filter(|t| t.agent_count == GATE_AGENT_COUNT)
            .enumerate()
        {
            t.frame_service_ms = vec![9.5 + i as f64 * 2.0; 128];
        }
        lanes[2].evidence.claimed = None; // no claim to dispute; noise alone blocks
        lanes[2].verdict = VerdictStatus::Inconclusive;
        let outcome = aggregate(&lanes);
        assert_eq!(outcome.decision, MergeDecision::Blocked);
        assert_eq!(outcome.overall, VerdictStatus::Inconclusive);
        assert!(
            outcome.reasons.iter().any(|r| r.contains("MAD")),
            "{:?}",
            outcome.reasons
        );
    }

    #[test]
    fn stale_manifest_fails() {
        let mut lanes = all_pass();
        lanes[0].evidence.host_manifest.schema_version = "lab-host-manifest-v0".into();
        let outcome = aggregate(&lanes);
        assert_eq!(outcome.decision, MergeDecision::Blocked);
        assert!(
            outcome.reasons.iter().any(|r| r.contains("stale manifest")),
            "{:?}",
            outcome.reasons
        );
    }

    #[test]
    fn forged_summary_blocks() {
        let mut lanes = all_pass();
        // Forge the windows candidate summary while raw samples stay honest.
        lanes[1].evidence.claimed = Some(ClaimedStats {
            median_p95_ms: 1.0,
            median_p99_ms: 1.0,
            verdict: "pass".into(),
        });
        let outcome = aggregate(&lanes);
        assert_eq!(outcome.decision, MergeDecision::Blocked);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("claimed stats mismatch")),
            "{:?}",
            outcome.reasons
        );
    }

    #[test]
    fn candidate_pass_cannot_override_recompute() {
        let mut lanes = all_pass();
        // Raw ubuntu samples miss the 50k gate; candidate still claims pass
        // (honest medians, forged verdict) and the lane adapter is (wrongly)
        // reported as pass — the aggregate recompute must still block.
        for t in lanes[0]
            .evidence
            .raw_trials
            .iter_mut()
            .filter(|t| t.agent_count == GATE_AGENT_COUNT)
        {
            t.frame_service_ms = vec![40.0; 128];
        }
        let agg = recompute_gate_aggregate(&lanes[0].evidence).unwrap();
        lanes[0].evidence.claimed = Some(ClaimedStats {
            median_p95_ms: agg.median_p95_ms,
            median_p99_ms: agg.median_p99_ms,
            verdict: "pass".into(),
        });
        lanes[0].verdict = VerdictStatus::Pass;
        let outcome = aggregate(&lanes);
        assert_eq!(outcome.decision, MergeDecision::Blocked);
        assert_eq!(outcome.overall, VerdictStatus::Fail);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("candidate-reported pass overridden")),
            "{:?}",
            outcome.reasons
        );
        assert!(
            outcome.reasons.iter().any(|r| r.contains("50k median p95")),
            "{:?}",
            outcome.reasons
        );
    }

    #[test]
    fn missing_scale_evidence_blocks() {
        let mut lanes = all_pass();
        lanes[0]
            .evidence
            .raw_trials
            .retain(|t| t.agent_count != 10_000);
        let outcome = aggregate(&lanes);
        assert_eq!(outcome.decision, MergeDecision::Blocked);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("scale evidence at 10000 agents")),
            "{:?}",
            outcome.reasons
        );
    }

    #[test]
    fn slow_scale_counts_do_not_block() {
        // Fixture evidence already records 100k at 30 ms (> 25 ms limit):
        // pass anyway — only 50k gates.
        let outcome = aggregate(&all_pass());
        assert_eq!(outcome.decision, MergeDecision::MergeExactHash);
        let slow = outcome
            .scale_rows
            .iter()
            .find(|s| s.agent_count == 100_000)
            .unwrap();
        assert!(slow.median_p99_ms > 25.0);
    }

    #[test]
    fn reset_not_started_blocks_as_error() {
        let mut lanes = all_pass();
        lanes[2].post_run_reset_started = false;
        lanes[2].verdict = VerdictStatus::Error;
        let outcome = aggregate(&lanes);
        assert_eq!(outcome.decision, MergeDecision::Blocked);
        assert_eq!(outcome.overall, VerdictStatus::Error);
        assert!(
            outcome
                .reasons
                .iter()
                .any(|r| r.contains("post-run reset not started")),
            "{:?}",
            outcome.reasons
        );
    }

    #[test]
    fn all_pass_prints_exact_summary() {
        let outcome = aggregate(&all_pass());
        let summary = render_merge_summary("pr", Some("deadbeefcafe"), false, &outcome);
        let expected = format!(
            "## mmd-lab 3-host merge gate\n\
             - mode: `pr`\n\
             - git_commit: `deadbeefcafe`\n\
             - archive_sha256: `{ARCHIVE}`\n\
             - dirty_worktree: false\n\
             - decision: **merge-exact-hash**\n\
             - overall: **pass**\n\
             \n\
             | lane | host | platform | p95_ms | p99_ms | golden | reset | verdict |\n\
             | --- | --- | --- | ---: | ---: | --- | --- | --- |\n\
             | ubuntu | ubuntu-ref | linux-x86_64 | 10.0600 | 10.0600 | true | true | pass |\n\
             | windows | windows-ref | windows-x86_64 | 11.0600 | 11.0600 | true | true | pass |\n\
             | macos | macos-ref | macos-arm64 | 9.5600 | 9.5600 | true | true | pass |\n\
             \n\
             ### Scale evidence (recorded; only 50k blocks)\n\
             | lane | agents | p95_ms | p99_ms | status |\n\
             | --- | ---: | ---: | ---: | --- |\n\
             | ubuntu | 1000 | 2.0000 | 2.0000 | recorded |\n\
             | ubuntu | 10000 | 4.5000 | 4.5000 | recorded |\n\
             | ubuntu | 100000 | 30.0000 | 30.0000 | recorded |\n\
             | windows | 1000 | 2.0000 | 2.0000 | recorded |\n\
             | windows | 10000 | 4.5000 | 4.5000 | recorded |\n\
             | windows | 100000 | 30.0000 | 30.0000 | recorded |\n\
             | macos | 1000 | 2.0000 | 2.0000 | recorded |\n\
             | macos | 10000 | 4.5000 | 4.5000 | recorded |\n\
             | macos | 100000 | 30.0000 | 30.0000 | recorded |\n\
             \n\
             _Owner merges exactly archive `{ARCHIVE}`; any other hash is untested. Coordinator-verified evidence; candidate claims are never trusted._"
        );
        assert_eq!(summary, expected);
        // Deterministic: same inputs → byte-identical output.
        let again = render_merge_summary("pr", Some("deadbeefcafe"), false, &outcome);
        assert_eq!(summary, again);
    }

    #[test]
    fn blocked_summary_lists_all_collected_reasons() {
        let mut lanes = all_pass();
        // Two independent failures in different lanes: both must surface.
        lanes[0].evidence.archive_sha256 = "cc".repeat(32);
        lanes[2].golden_ok = false;
        lanes[2].verdict = VerdictStatus::Fail;
        lanes[2].reasons.push("golden diff reject: fixture".into());
        let outcome = aggregate(&lanes);
        assert_eq!(outcome.decision, MergeDecision::Blocked);
        let summary = render_merge_summary("pr", None, false, &outcome);
        assert!(summary.contains("- decision: **blocked**"), "{summary}");
        assert!(summary.contains("mixed source hashes"), "{summary}");
        assert!(summary.contains("golden diff reject"), "{summary}");
        assert!(
            summary.contains(&format!("archive `{ARCHIVE}`")),
            "{summary}"
        );
    }
}
