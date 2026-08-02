//! App binary: interactive `run` and timed `bench` entry points.

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
    /// Run interactive prototype scene
    Run,
    /// Run benchmark harness and emit JSON report
    Bench,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run => {
            println!("run: shell only (engine {})", mmd_engine::version());
        }
        Commands::Bench => {
            println!("bench: shell only (engine {})", mmd_engine::version());
        }
    }
}
