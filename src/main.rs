//! App binary: interactive `run` and timed `bench` entry points.

mod bench;
mod input;
mod overlay;
mod rts_input;
mod rts_overlay;
mod rts_run;
mod rts_script;
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
        /// Agent count override (default: scenario hard count = 5000)
        #[arg(long)]
        agents: Option<u32>,
        /// Scenario path (default: assets/scenarios/technical_prototype_v1.ron)
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Auto-exit after N frames (CI/smoke). Omit for interactive.
        #[arg(long)]
        frames: Option<u64>,
        /// Scripted key presses for runs with no keyboard: `FRAME:KEY[,...]`,
        /// 1-based frames, keys `esc`/`f1`/`space` (e.g. `4:space,20:esc`).
        #[arg(long, value_name = "FRAME:KEY,...")]
        inject_input: Option<String>,
    },
    /// Run the phase-1 RTS engine prototype scene.
    Rts {
        /// Scenario path (default: assets/scenarios/rts_prototype_v1.ron)
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Auto-exit after N frames (CI/smoke). Omit for interactive.
        #[arg(long)]
        frames: Option<u64>,
        /// Scripted input for runs with no keyboard or mouse:
        /// `FRAME:KIND[:ARGS]` entries separated by `;`.
        #[arg(long, value_name = "FRAME:KIND[:ARGS];...")]
        inject_input: Option<String>,
    },
    /// Developer benchmark harness — not a gate; optimization phase.
    ///
    /// Runs the scale curve and emits a JSON report. Its output gates nothing:
    /// performance measurement is retired from phase 0 to a later optimization
    /// phase (see docs/05-testing.md). Frozen tooling, kept for reuse.
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
            inject_input,
        } => {
            let opts = run::RunOptions {
                agents,
                scenario,
                frames,
                inject_input,
            };
            if let Err(e) = run::run(opts) {
                // The message is the UX: it names the file or the flag at
                // fault. The code distinguishes "no GPU here" from a defect.
                eprintln!("run failed: {e}");
                return ExitCode::from(e.exit_code());
            }
            ExitCode::SUCCESS
        }
        Commands::Rts {
            scenario,
            frames,
            inject_input,
        } => {
            let opts = rts_run::RtsOptions {
                scenario,
                frames,
                inject_input,
            };
            if let Err(e) = rts_run::run(opts) {
                eprintln!("rts failed: {e}");
                return ExitCode::from(e.exit_code());
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
