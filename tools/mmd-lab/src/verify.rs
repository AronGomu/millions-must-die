//! Coordinator recomputes stats/gates from raw samples. Candidate claims untrusted.

use mmd_engine::bench::{
    BenchPolicy, GATE_AGENT_COUNT, TrialAggregate, TrialPercentiles, VerdictStatus,
};

use crate::report::{ClaimedStats, HostEvidence};

/// Epsilon for claimed-vs-recomputed float compare.
pub const CLAIM_EPS_MS: f64 = 1e-6;

#[derive(Debug, Clone, PartialEq)]
pub struct HostVerdict {
    pub host_id: String,
    pub platform: String,
    pub archive_ok: bool,
    pub remote_verified: bool,
    pub recomputed_median_p95_ms: f64,
    pub recomputed_median_p99_ms: f64,
    pub nmad_p95: f64,
    pub nmad_p99: f64,
    pub claimed_stats_match: bool,
    pub claimed: Option<ClaimedStats>,
    pub verdict: VerdictStatus,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatrixVerdict {
    pub archive_sha256: String,
    pub hosts: Vec<HostVerdict>,
    pub overall: VerdictStatus,
    pub reason: String,
}

impl MatrixVerdict {
    #[allow(dead_code)]
    pub fn is_pass(&self) -> bool {
        self.overall == VerdictStatus::Pass
    }
}

/// Recompute 50k trial percentiles from raw samples only.
pub fn recompute_gate_aggregate(evidence: &HostEvidence) -> Result<TrialAggregate, String> {
    let mut indexed: Vec<(u32, TrialPercentiles)> = evidence
        .raw_trials
        .iter()
        .filter(|t| t.agent_count == GATE_AGENT_COUNT)
        .map(|t| {
            (
                t.trial_index,
                TrialPercentiles::from_samples(&t.frame_service_ms),
            )
        })
        .collect();
    if indexed.is_empty() {
        return Err(format!(
            "host {} missing raw samples for gate count {GATE_AGENT_COUNT}",
            evidence.host_manifest.host_id
        ));
    }
    indexed.sort_by_key(|(i, _)| *i);
    let trials: Vec<TrialPercentiles> = indexed.into_iter().map(|(_, t)| t).collect();
    Ok(TrialAggregate::from_trials(&trials))
}

/// Compare claimed summary to coordinator recompute.
pub fn claimed_matches(claimed: &ClaimedStats, agg: &TrialAggregate) -> bool {
    (claimed.median_p95_ms - agg.median_p95_ms).abs() <= CLAIM_EPS_MS
        && (claimed.median_p99_ms - agg.median_p99_ms).abs() <= CLAIM_EPS_MS
}

/// Independent host verdict from evidence + expected archive digest.
pub fn verify_host(
    expected_archive_sha256: &str,
    evidence: &HostEvidence,
    policy: &BenchPolicy,
) -> HostVerdict {
    let host_id = evidence.host_manifest.host_id.clone();
    let platform = evidence.host_manifest.platform.clone();

    if !evidence.archive_sha256.eq_ignore_ascii_case(expected_archive_sha256) {
        return HostVerdict {
            host_id,
            platform,
            archive_ok: false,
            remote_verified: evidence.remote_archive_verified,
            recomputed_median_p95_ms: f64::NAN,
            recomputed_median_p99_ms: f64::NAN,
            nmad_p95: f64::NAN,
            nmad_p99: f64::NAN,
            claimed_stats_match: false,
            claimed: evidence.claimed.clone(),
            verdict: VerdictStatus::Fail,
            reason: format!(
                "archive hash mismatch: evidence {} expected {expected_archive_sha256}",
                evidence.archive_sha256
            ),
        };
    }

    if !evidence.remote_archive_verified {
        return HostVerdict {
            host_id,
            platform,
            archive_ok: false,
            remote_verified: false,
            recomputed_median_p95_ms: f64::NAN,
            recomputed_median_p99_ms: f64::NAN,
            nmad_p95: f64::NAN,
            nmad_p99: f64::NAN,
            claimed_stats_match: false,
            claimed: evidence.claimed.clone(),
            verdict: VerdictStatus::Fail,
            reason: "remote archive hash verification failed".into(),
        };
    }

    let agg = match recompute_gate_aggregate(evidence) {
        Ok(a) => a,
        Err(reason) => {
            return HostVerdict {
                host_id,
                platform,
                archive_ok: true,
                remote_verified: true,
                recomputed_median_p95_ms: f64::NAN,
                recomputed_median_p99_ms: f64::NAN,
                nmad_p95: f64::NAN,
                nmad_p99: f64::NAN,
                claimed_stats_match: false,
                claimed: evidence.claimed.clone(),
                verdict: VerdictStatus::Error,
                reason,
            };
        }
    };

    let claimed_stats_match = match &evidence.claimed {
        Some(c) => claimed_matches(c, &agg),
        None => true, // no claim to dispute
    };

    // Apply same absolute gate logic as engine (blocking count path).
    let (mut verdict, mut reason) = absolute_gate(policy, &agg, evidence);

    if !claimed_stats_match {
        // Forged stats never become pass; surface as fail with recompute truth.
        if verdict == VerdictStatus::Pass {
            verdict = VerdictStatus::Fail;
        }
        reason = format!(
            "claimed stats mismatch coordinator recompute (claimed p95={} p99={}; recomputed p95={:.6} p99={:.6}); {reason}",
            evidence.claimed.as_ref().map(|c| c.median_p95_ms).unwrap_or(f64::NAN),
            evidence.claimed.as_ref().map(|c| c.median_p99_ms).unwrap_or(f64::NAN),
            agg.median_p95_ms,
            agg.median_p99_ms
        );
    }

    HostVerdict {
        host_id,
        platform,
        archive_ok: true,
        remote_verified: true,
        recomputed_median_p95_ms: agg.median_p95_ms,
        recomputed_median_p99_ms: agg.median_p99_ms,
        nmad_p95: agg.nmad_p95,
        nmad_p99: agg.nmad_p99,
        claimed_stats_match,
        claimed: evidence.claimed.clone(),
        verdict,
        reason,
    }
}

fn absolute_gate(
    policy: &BenchPolicy,
    agg: &TrialAggregate,
    evidence: &HostEvidence,
) -> (VerdictStatus, String) {
    if evidence.submitted_frames != evidence.completed_frames {
        return (
            VerdictStatus::Error,
            format!(
                "drain mismatch submitted={} completed={}",
                evidence.submitted_frames, evidence.completed_frames
            ),
        );
    }
    if evidence.max_in_flight > policy.frames_in_flight {
        return (
            VerdictStatus::Error,
            format!(
                "frames in flight {} exceeded cap {}",
                evidence.max_in_flight, policy.frames_in_flight
            ),
        );
    }
    if evidence.project_rust_alloc_count > 0 {
        return (
            VerdictStatus::Fail,
            format!(
                "project Rust frame allocations = {}",
                evidence.project_rust_alloc_count
            ),
        );
    }
    if agg.noisy {
        return (
            VerdictStatus::Inconclusive,
            format!(
                "50k normalized MAD over 3% (nmad_p95={:.4} nmad_p99={:.4})",
                agg.nmad_p95, agg.nmad_p99
            ),
        );
    }
    if agg.median_p95_ms > policy.p95_limit_ms() {
        return (
            VerdictStatus::Fail,
            format!(
                "50k median p95 {:.3} ms > {:.2} ms",
                agg.median_p95_ms,
                policy.p95_limit_ms()
            ),
        );
    }
    if agg.median_p99_ms > policy.p99_limit_ms() {
        return (
            VerdictStatus::Fail,
            format!(
                "50k median p99 {:.3} ms > {:.2} ms",
                agg.median_p99_ms,
                policy.p99_limit_ms()
            ),
        );
    }
    (
        VerdictStatus::Pass,
        format!(
            "50k median p95 {:.3} ms <= {:.2}; p99 {:.3} ms <= {:.2}",
            agg.median_p95_ms,
            policy.p95_limit_ms(),
            agg.median_p99_ms,
            policy.p99_limit_ms()
        ),
    )
}

/// Aggregate multi-host matrix. Missing host → fail. Any non-pass → worst status.
pub fn verify_matrix(
    expected_archive_sha256: &str,
    evidences: &[HostEvidence],
    required_host_ids: &[&str],
    policy: &BenchPolicy,
) -> MatrixVerdict {
    let mut hosts: Vec<HostVerdict> = evidences
        .iter()
        .map(|e| verify_host(expected_archive_sha256, e, policy))
        .collect();
    hosts.sort_by(|a, b| a.host_id.cmp(&b.host_id));

    for req in required_host_ids {
        if !hosts.iter().any(|h| h.host_id == *req) {
            hosts.push(HostVerdict {
                host_id: (*req).into(),
                platform: "missing".into(),
                archive_ok: false,
                remote_verified: false,
                recomputed_median_p95_ms: f64::NAN,
                recomputed_median_p99_ms: f64::NAN,
                nmad_p95: f64::NAN,
                nmad_p99: f64::NAN,
                claimed_stats_match: false,
                claimed: None,
                verdict: VerdictStatus::Fail,
                reason: "required host missing from matrix".into(),
            });
        }
    }
    hosts.sort_by(|a, b| a.host_id.cmp(&b.host_id));

    let (overall, reason) = fold_overall(&hosts);
    MatrixVerdict {
        archive_sha256: expected_archive_sha256.to_string(),
        hosts,
        overall,
        reason,
    }
}

fn fold_overall(hosts: &[HostVerdict]) -> (VerdictStatus, String) {
    if hosts.is_empty() {
        return (VerdictStatus::Error, "no hosts in matrix".into());
    }
    let mut worst = VerdictStatus::Pass;
    let mut reasons = Vec::new();
    for h in hosts {
        worst = worse(worst, h.verdict);
        if h.verdict != VerdictStatus::Pass {
            reasons.push(format!("{}: {}", h.host_id, h.reason));
        }
    }
    let reason = if reasons.is_empty() {
        format!("all {} hosts pass coordinator gates", hosts.len())
    } else {
        reasons.join("; ")
    };
    (worst, reason)
}

pub(crate) fn worse(a: VerdictStatus, b: VerdictStatus) -> VerdictStatus {
    use VerdictStatus::*;
    let rank = |v: VerdictStatus| match v {
        Pass | Recorded => 0,
        Inconclusive => 1,
        Fail => 2,
        Error => 3,
    };
    if rank(b) > rank(a) { b } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{HostManifest, RawTrialSamples};

    fn steady_samples(ms: f64, n: usize) -> Vec<f64> {
        vec![ms; n]
    }

    fn pass_evidence(id: &str, archive: &str) -> HostEvidence {
        let mut e = HostEvidence::new(
            HostManifest::new(id, "linux-x86_64", "vulkan", "Ubuntu 24.04"),
            archive,
        );
        e.submitted_frames = 100;
        e.completed_frames = 100;
        e.max_in_flight = 2;
        for i in 0..7 {
            e.raw_trials.push(RawTrialSamples {
                agent_count: GATE_AGENT_COUNT,
                trial_index: i,
                frame_service_ms: steady_samples(10.0 + i as f64 * 0.01, 64),
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

    #[test]
    fn recompute_detects_forged_p99() {
        let archive = "ab".repeat(32);
        let mut e = pass_evidence("ubuntu-ref", &archive);
        // Forge claimed p99 while raw samples stay fast.
        e.claimed = Some(ClaimedStats {
            median_p95_ms: 10.0,
            median_p99_ms: 1.0, // lie
            verdict: "pass".into(),
        });
        let v = verify_host(&archive, &e, &BenchPolicy::production());
        assert!(!v.claimed_stats_match);
        assert_ne!(v.verdict, VerdictStatus::Pass);
        assert!(v.reason.contains("claimed stats mismatch"));
    }

    #[test]
    fn archive_mismatch_fails() {
        let e = pass_evidence("ubuntu-ref", &"aa".repeat(32));
        let v = verify_host(&"bb".repeat(32), &e, &BenchPolicy::production());
        assert!(!v.archive_ok);
        assert_eq!(v.verdict, VerdictStatus::Fail);
    }
}
