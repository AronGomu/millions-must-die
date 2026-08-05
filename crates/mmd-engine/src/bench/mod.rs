//! Benchmark measurement: stats, 2-frame fence queue, policy, report, runner.

mod fence_queue;
mod policy;
mod report;
mod runner;
mod stats;

pub use fence_queue::{
    BeginFrame, CompletedFrame, FenceQueue, FenceQueueError, InflightFrame, MAX_FRAMES_IN_FLIGHT,
    MockFence,
};
pub use policy::{
    BenchPolicy, GATE_AGENT_COUNT, SCALE_COUNTS, STRETCH_AGENT_COUNT, TEST_POLICY_ENV,
};
pub use report::{
    AbsoluteGate, BenchExitCode, BenchmarkReport, REPORT_SCHEMA_VERSION, ReportManifests,
    ScaleResult, TrialReport, VerdictStatus, WorkloadIdentity, build_report, build_scale_result,
    scale_verdict, trial_report,
};
pub use runner::{BenchError, BenchOptions, default_scenario_path, run_bench, synthetic_scale_from_trial_p99s};
pub use stats::{
    GATE_P95_MS, GATE_P99_MS, MAD_SCALE, NMAD_LIMIT, SampleBuffer, TrialAggregate, TrialPercentiles,
    is_noisy, mad, median, normalized_mad, percentile_type7, percentile_type7_unsorted,
};
