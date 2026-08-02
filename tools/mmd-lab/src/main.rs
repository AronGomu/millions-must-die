//! Trusted local lab CLI shell. Candidate archives must never supply this binary.

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "mmd-lab",
    version,
    about = "Trusted local validation lab coordinator"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Inspect lab host readiness
    Doctor,
    /// Validate candidate archive / local gates (shell)
    Validate,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Doctor => {
            println!("doctor: shell only; host checks land in later tickets");
        }
        Commands::Validate => {
            println!("validate: shell only; gate dispatch lands in later tickets");
        }
    }
}
