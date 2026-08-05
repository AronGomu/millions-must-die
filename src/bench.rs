//! `bench` CLI: scale curve + versioned JSON report + exit codes.
//!
//! **Not a gate; optimization phase.** Performance measurement is retired from
//! phase 0 (see `docs/05-testing.md`). This harness is frozen developer
//! tooling: it still builds and its unit tests still run, but no verdict or
//! exit code it produces decides whether a change may merge.

use std::path::PathBuf;
use std::process::ExitCode;

use mmd_engine::bench::{BenchExitCode, BenchOptions, BenchPolicy, run_bench};

/// CLI options for timed bench.
#[derive(Debug, Clone, Default)]
pub struct BenchCliOptions {
    pub output: Option<PathBuf>,
    pub scenario: Option<PathBuf>,
    /// Force injectable short policy (also `MMD_BENCH_TEST_POLICY` env).
    pub test_policy: bool,
    /// CPU-only dry path (no GPU). For offline JSON/exit-code smoke.
    pub dry_cpu: bool,
    /// Force heap alloc inside measured frames (zero-alloc gate proof).
    pub inject_frame_alloc: bool,
}

/// Run bench harness; write JSON; map verdict → process exit code.
pub fn bench(opts: BenchCliOptions) -> ExitCode {
    let policy = BenchPolicy::resolve(opts.test_policy);
    eprintln!(
        "bench: developer tool — not a gate; performance is retired to the optimization phase \
         (docs/05-testing.md). This report gates nothing."
    );
    if policy.policy_id == "test-short-v1" {
        eprintln!("bench: using injectable short policy (full prod policy is hours)");
    } else {
        eprintln!("bench: production policy — 4 counts × (10s warmup + 7×60s trials); long run");
    }

    let scenario = opts
        .scenario
        .unwrap_or_else(mmd_engine::bench::default_scenario_path);

    let output = opts
        .output
        .or_else(|| std::env::var_os("MMD_BENCH_OUTPUT").map(PathBuf::from));

    let dry = opts.dry_cpu || std::env::var_os("MMD_BENCH_DRY").is_some();
    let inject =
        opts.inject_frame_alloc || std::env::var_os("MMD_BENCH_INJECT_FRAME_ALLOC").is_some();

    let run_opts = BenchOptions {
        policy,
        scenario,
        output: output.clone(),
        dry_cpu_only: dry,
        inject_frame_alloc: inject,
    };

    match run_bench(run_opts) {
        Ok(report) => {
            if let Some(path) = output.as_ref() {
                eprintln!("bench: wrote {}", path.display());
            } else {
                // stdout JSON when no --output
                match report.to_json_pretty() {
                    Ok(j) => println!("{j}"),
                    Err(e) => {
                        eprintln!("bench: serialize failed: {e}");
                        return exit(BenchExitCode::Error);
                    }
                }
            }
            eprintln!(
                "bench: verdict={:?} reason={} (informational — gates no merge)",
                report.verdict, report.verdict_reason
            );
            for s in &report.scale_results {
                eprintln!(
                    "bench: count={} blocking={} verdict={:?} p95={:.3} p99={:.3} max_if={} rust_allocs={}",
                    s.agent_count,
                    s.blocking,
                    s.verdict,
                    s.median_p95_frame_service_ms,
                    s.median_p99_frame_service_ms,
                    s.max_in_flight,
                    s.project_rust_alloc_count
                );
            }
            exit(report.exit_code())
        }
        Err(e) => {
            eprintln!("bench failed: {e}");
            exit(BenchExitCode::Error)
        }
    }
}

fn exit(code: BenchExitCode) -> ExitCode {
    ExitCode::from(code.as_i32() as u8)
}
