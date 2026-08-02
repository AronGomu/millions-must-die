//! App binary: interactive `run` and timed `bench` entry points.

mod bench;
mod input;
mod overlay;
mod run;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Counting allocator for bench zero-alloc gate (inert until MeasureGuard).
#[global_allocator]
static GLOBAL: mmd_engine::alloc_guard::CountingAllocator =
    mmd_engine::alloc_guard::CountingAllocator;

#[derive(Debug, Parser)]
#[command(
    name = "millions_must_die",
    version,
    about = "Millions Must Die phase-0 technical prototype"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run interactive prototype scene (moving flow-field horde)
    Run {
        /// Agent count override (default: scenario hard count = 50000)
        #[arg(long)]
        agents: Option<u32>,
        /// Scenario path (default: assets/scenarios/technical_prototype_v1.ron)
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Auto-exit after N frames (CI/smoke). Omit for interactive.
        #[arg(long)]
        frames: Option<u64>,
    },
    /// Run benchmark harness and emit JSON report
    Bench {
        /// Write versioned JSON report to this path
        #[arg(long)]
        output: Option<PathBuf>,
        /// Scenario path override
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Injectable short policy for local smoke (also MMD_BENCH_TEST_POLICY).
        /// Production defaults stay locked without this flag.
        #[arg(long)]
        test_policy: bool,
        /// CPU-only dry path (no GPU). For offline JSON/exit-code smoke.
        #[arg(long)]
        dry_cpu: bool,
        /// Inject a Rust heap alloc each measured frame (proves zero-alloc gate).
        #[arg(long)]
        inject_frame_alloc: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run {
            agents,
            scenario,
            frames,
        } => {
            let opts = run::RunOptions {
                agents,
                scenario,
                frames,
            };
            if let Err(e) = run::run(opts) {
                eprintln!("run failed: {e}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Commands::Bench {
            output,
            scenario,
            test_policy,
            dry_cpu,
            inject_frame_alloc,
        } => bench::bench(bench::BenchCliOptions {
            output,
            scenario,
            test_policy,
            dry_cpu,
            inject_frame_alloc,
        }),
    }
}
