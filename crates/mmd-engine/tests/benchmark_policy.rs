//! T11 benchmark policy / stats / fence queue / verdict tests.

use std::time::{Duration, Instant};

use mmd_engine::bench::{
    BenchOptions, BenchPolicy, FenceQueue, FenceQueueError, GATE_AGENT_COUNT, GATE_P95_MS,
    GATE_P99_MS, MAX_FRAMES_IN_FLIGHT, MockFence, REPORT_SCHEMA_VERSION, ReportManifests,
    SCALE_COUNTS, STRETCH_AGENT_COUNT, TEST_SHORT_GATE_AGENT_COUNT, TEST_SHORT_SCALE_COUNTS,
    TrialAggregate, TrialPercentiles, VerdictStatus, WorkloadIdentity, build_report, is_noisy,
    median, normalized_mad, percentile_type7, run_bench, synthetic_scale_from_trial_p99s,
};
use mmd_engine::scenario::MAX_LIVE_AGENTS;
use sha2::{Digest, Sha256};

#[test]
fn type7_percentiles_match_fixture() {
    // Sorted 1..=100. Type-7 p95/p99 exact.
    let s: Vec<f64> = (1..=100).map(|x| x as f64).collect();
    // h95 = 99 * 0.95 = 94.05 → x[94] + 0.05*(x[95]-x[94]) = 95 + 0.05 = 95.05
    let p95 = percentile_type7(&s, 0.95);
    assert!((p95 - 95.05).abs() < 1e-12, "p95={p95} want 95.05");
    // h99 = 99 * 0.99 = 98.01 → 99 + 0.01*(100-99) = 99.01
    let p99 = percentile_type7(&s, 0.99);
    assert!((p99 - 99.01).abs() < 1e-12, "p99={p99} want 99.01");

    let tp = TrialPercentiles::from_samples(&s);
    assert!((tp.p95_ms - 95.05).abs() < 1e-12);
    assert!((tp.p99_ms - 99.01).abs() < 1e-12);
}

#[test]
fn trial_median_is_fourth_scalar() {
    // 7 trial p99s → sorted item index 3 (4th).
    let trials: Vec<TrialPercentiles> = [30.0, 10.0, 40.0, 20.0, 50.0, 25.0, 15.0]
        .into_iter()
        .map(|p99| TrialPercentiles {
            p95_ms: p99 - 5.0,
            p99_ms: p99,
            sample_count: 10,
        })
        .collect();
    let agg = TrialAggregate::from_trials(&trials);
    // sorted p99: 10,15,20,25,30,40,50 → median 25
    assert!((agg.median_p99_ms - 25.0).abs() < 1e-12);
    // sorted p95: 5,10,15,20,25,35,45 → median 20
    assert!((agg.median_p95_ms - 20.0).abs() < 1e-12);
    let mut v = [30.0, 10.0, 40.0, 20.0, 50.0, 25.0, 15.0];
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(v[3], 25.0);
    assert!((median(&v) - 25.0).abs() < 1e-12);
}

#[test]
fn rejects_normalized_mad_over_three_percent() {
    let quiet = [16.0, 16.1, 15.9, 16.05, 15.95, 16.02, 15.98];
    assert!(!is_noisy(&quiet), "nmad={}", normalized_mad(&quiet));

    let noisy = [10.0, 12.0, 16.0, 20.0, 25.0, 30.0, 40.0];
    let n = normalized_mad(&noisy);
    assert!(n > 0.03, "nmad={n}");
    assert!(is_noisy(&noisy));

    let policy = BenchPolicy::production();
    let p95 = [10.0, 12.0, 16.0, 20.0, 25.0, 30.0, 40.0];
    let p99 = [12.0, 14.0, 18.0, 22.0, 28.0, 35.0, 45.0];
    let scale = synthetic_scale_from_trial_p99s(&policy, GATE_AGENT_COUNT, p95, p99);
    assert_eq!(scale.verdict, VerdictStatus::Inconclusive);
}

#[test]
fn never_exceeds_two_in_flight() {
    assert_eq!(MAX_FRAMES_IN_FLIGHT, 2);
    let mut q = FenceQueue::<MockFence>::production();
    let t0 = Instant::now();

    // Frame 0, 1: no wait.
    assert!(q.take_oldest_if_full().is_none());
    q.submit(MockFence::new_delay(t0, Duration::from_millis(30)), t0)
        .unwrap();
    assert_eq!(q.in_flight(), 1);

    assert!(q.take_oldest_if_full().is_none());
    q.submit(
        MockFence::new_delay(t0, Duration::from_millis(30)),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(q.in_flight(), 2);
    assert_eq!(q.max_observed_in_flight(), 2);

    // Frame 2: must take oldest before admit.
    let oldest = q
        .take_oldest_if_full()
        .expect("must wait oldest before frame 3");
    oldest.fence.wait();
    q.complete_waited(oldest, Instant::now());
    assert_eq!(q.in_flight(), 1);
    q.submit(
        MockFence::new_delay(Instant::now(), Duration::from_millis(5)),
        Instant::now(),
    )
    .unwrap();
    assert!(q.in_flight() <= 2);
    q.assert_cap_held().unwrap();

    // Cannot submit when at cap without take.
    let mut q2 = FenceQueue::<u32>::new(2);
    q2.submit(1, Instant::now()).unwrap();
    q2.submit(2, Instant::now()).unwrap();
    let err = q2.submit(3, Instant::now()).unwrap_err();
    assert!(matches!(err, FenceQueueError::ExceededCap { .. }));
}

#[test]
fn drain_requires_all_completed() {
    let mut q = FenceQueue::<MockFence>::production();
    let t0 = Instant::now();
    q.submit(MockFence::new_delay(t0, Duration::from_millis(5)), t0)
        .unwrap();
    q.submit(
        MockFence::new_delay(t0, Duration::from_millis(5)),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(q.submitted(), 2);
    assert_eq!(q.completed(), 0);

    let pending = q.take_all_pending();
    for frame in pending {
        frame.fence.wait();
        q.complete_waited(frame, Instant::now());
    }
    q.finish_drain().unwrap();
    assert_eq!(q.submitted(), q.completed());
    assert_eq!(q.in_flight(), 0);

    // Incomplete drain fails.
    let mut q3 = FenceQueue::<MockFence>::production();
    q3.submit(
        MockFence::new_delay(Instant::now(), Duration::from_millis(1)),
        Instant::now(),
    )
    .unwrap();
    // Leave pending; finish_drain must fail.
    let err = q3.finish_drain().unwrap_err();
    assert!(matches!(err, FenceQueueError::DrainIncomplete { .. }));
}

#[test]
fn absolute_gate_fails_p99() {
    let policy = BenchPolicy::production();
    // Quiet enough MAD; p99 median just over 25 ms.
    let p99 = [24.0, 24.5, 25.0, 25.01, 25.02, 25.05, 25.1];
    let p95 = [15.0, 15.1, 15.2, 15.3, 15.4, 15.5, 15.6];
    assert!((median(&p99) - 25.01).abs() < 1e-9);
    assert!(median(&p99) > GATE_P99_MS);
    assert!(median(&p95) <= GATE_P95_MS);

    let scale = synthetic_scale_from_trial_p99s(&policy, GATE_AGENT_COUNT, p95, p99);
    assert_eq!(scale.verdict, VerdictStatus::Fail);
    assert!(scale.verdict_reason.contains("p99"));
}

#[test]
fn other_counts_never_block() {
    let policy = BenchPolicy::production();
    assert_eq!(SCALE_COUNTS, [1_000, 10_000, 50_000, 100_000]);
    assert_eq!(STRETCH_AGENT_COUNT, 100_000);

    let bad_p95 = [100.0; 7];
    let bad_p99 = [200.0; 7];
    let good_p95 = [10.0, 10.1, 10.2, 10.0, 10.05, 10.15, 9.95];
    let good_p99 = [12.0, 12.1, 12.2, 12.0, 12.05, 12.15, 11.95];

    let mut scales = Vec::new();
    for &c in &SCALE_COUNTS {
        let (p95, p99) = if c == GATE_AGENT_COUNT {
            (good_p95, good_p99)
        } else {
            (bad_p95, bad_p99)
        };
        scales.push(synthetic_scale_from_trial_p99s(&policy, c, p95, p99));
    }

    for s in &scales {
        if s.agent_count == GATE_AGENT_COUNT {
            assert_eq!(s.verdict, VerdictStatus::Pass);
            assert!(s.blocking);
        } else {
            assert_eq!(s.verdict, VerdictStatus::Recorded);
            assert!(!s.blocking);
            if s.agent_count == STRETCH_AGENT_COUNT {
                assert!(s.stretch);
            }
        }
    }

    let manifests = ReportManifests::new(
        WorkloadIdentity::new("technical_prototype_v1", "a".repeat(64), "b".repeat(64)),
        "vulkan",
        "test",
        1,
        "0.1.0",
        &policy,
    );
    let report = build_report(&policy, manifests, scales);
    assert_eq!(report.schema_version, REPORT_SCHEMA_VERSION);
    assert_eq!(report.verdict, VerdictStatus::Pass);
    assert!(!report.absolute_gate.relative_gates_enabled);
    let json = report.to_json_pretty().unwrap();
    assert!(json.contains("benchmark-report-v2"));
    assert!(json.contains("atlas_manifest_sha256"));
    assert!(json.contains("gpu_queue_latency"));
    assert!(json.contains("not true GPU"));
    assert!(json.contains("project_rust_alloc_count"));
    assert!(json.contains("project_alloc_visibility_note"));
    assert!(json.contains("SDL/driver C malloc"));
    let back: mmd_engine::bench::BenchmarkReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.verdict, VerdictStatus::Pass);
    assert_eq!(back.scale_results[2].project_rust_alloc_count, 0);
}

#[test]
fn dry_bench_pins_exact_atlas_manifest_bytes() {
    let report = run_bench(BenchOptions {
        policy: BenchPolicy::test_short(),
        dry_cpu_only: true,
        ..BenchOptions::default()
    })
    .expect("dry short bench");
    let manifest =
        std::fs::read(mmd_engine::workspace_root().join("assets/sprites/generated/manifest.json"))
            .expect("atlas manifest");
    let expected = hex::encode(Sha256::digest(&manifest));
    assert_eq!(report.manifests.atlas_manifest_sha256, expected);
    assert_eq!(report.manifests.atlas_manifest_sha256.len(), 64);
}

#[test]
fn benchmark_report_v2_schema_tracks_workload_identity() {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../schemas/benchmark-report-v2.schema.json"
    ))
    .expect("schema json");
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        REPORT_SCHEMA_VERSION
    );
    let required = schema["properties"]["manifests"]["required"]
        .as_array()
        .expect("manifest required");
    assert!(required.iter().any(|v| v == "atlas_manifest_sha256"));
    assert!(
        required
            .iter()
            .any(|v| v == "project_alloc_visibility_note")
    );
    let scale_required = schema["properties"]["scale_results"]["items"]["required"]
        .as_array()
        .expect("scale required");
    assert!(
        scale_required
            .iter()
            .any(|v| v == "project_rust_alloc_count")
    );
}

#[test]
fn allocation_count_fails_blocking_scale() {
    let policy = BenchPolicy::production();
    let good_p95 = [10.0, 10.1, 10.2, 10.0, 10.05, 10.15, 9.95];
    let good_p99 = [12.0, 12.1, 12.2, 12.0, 12.05, 12.15, 11.95];
    let mut scale = synthetic_scale_from_trial_p99s(&policy, GATE_AGENT_COUNT, good_p95, good_p99);
    // Simulate post-warmup project Rust allocs observed during measure.
    scale.project_rust_alloc_count = 3;
    let (v, reason) = mmd_engine::bench::scale_verdict(
        &policy,
        GATE_AGENT_COUNT,
        &TrialAggregate {
            median_p95_ms: scale.median_p95_frame_service_ms,
            median_p99_ms: scale.median_p99_frame_service_ms,
            nmad_p95: scale.nmad_p95,
            nmad_p99: scale.nmad_p99,
            noisy: false,
        },
        scale.submitted_frames,
        scale.completed_frames,
        scale.max_in_flight,
        3,
    );
    assert_eq!(v, VerdictStatus::Fail);
    assert!(reason.contains("project Rust frame allocations"));
    assert!(reason.contains("SDL/driver"));
}

#[test]
fn production_policy_locked_constants() {
    let p = BenchPolicy::production();
    assert_eq!(p.warmup, Duration::from_secs(10));
    assert_eq!(p.trial_count, 7);
    assert_eq!(p.trial_duration, Duration::from_secs(60));
    assert_eq!(p.frames_in_flight, 2);
    assert_eq!(p.gate_count, 50_000);
    assert_eq!(p.scale_counts, SCALE_COUNTS);
}

#[test]
fn test_short_policy_injectable() {
    let p = BenchPolicy::test_short();
    assert!(p.warmup < Duration::from_secs(1));
    assert!(p.trial_duration < Duration::from_secs(1));
    assert_eq!(p.trial_count, 7);
    // Its own ladder, not the frozen historical one: this policy runs the sim,
    // so every tier must sit at or under the live entity ceiling.
    assert_eq!(p.scale_counts, TEST_SHORT_SCALE_COUNTS);
    assert_eq!(p.gate_count, TEST_SHORT_GATE_AGENT_COUNT);
    assert_eq!(p.gate_count, MAX_LIVE_AGENTS);
    for c in &p.scale_counts {
        assert!(
            *c <= MAX_LIVE_AGENTS,
            "smoke tier {c} exceeds the live ceiling {MAX_LIVE_AGENTS}"
        );
    }
}
