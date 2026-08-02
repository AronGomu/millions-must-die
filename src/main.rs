//! App binary: interactive `run` and timed `bench` entry points.

mod input;
mod overlay;
mod run;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
    Bench,
}

fn main() {
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
                std::process::exit(1);
            }
        }
        Commands::Bench => {
            println!("bench: shell only (engine {})", mmd_engine::version());
        }
    }
}
