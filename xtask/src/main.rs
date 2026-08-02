//! Build/bootstrap task runner.

mod atlases;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "xtask",
    version,
    about = "Repo bootstrap and reproducibility tasks"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Placeholder for SDL/shader bootstrap
    Bootstrap,
    /// Generate or verify deterministic sprite atlases
    Atlases {
        /// Verify tracked atlases match clean regeneration
        #[arg(long)]
        check: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Bootstrap => {
            println!("bootstrap: shell only; native deps land in later tickets");
        }
        Commands::Atlases { check } => {
            if let Err(err) = atlases::run_atlases(check) {
                eprintln!("atlases error: {err}");
                std::process::exit(1);
            }
        }
    }
}
