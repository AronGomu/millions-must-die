//! Build/bootstrap task runner.

mod atlases;
mod audio;
mod bootstrap;
mod digest;
mod placeholder_art;
mod shaders;

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
    /// Pin/check SDL3 native bootstrap contract
    Bootstrap {
        /// Offline verification only (no network)
        #[arg(long)]
        check: bool,
    },
    /// Verify tracked offline shader backend blobs
    Shaders {
        /// Verify manifests + digests
        #[arg(long)]
        check: bool,
    },
    /// Generate or verify deterministic sprite atlases
    Atlases {
        /// Verify tracked atlases match clean regeneration
        #[arg(long)]
        check: bool,
    },
    /// Generate or verify deterministic placeholder audio
    Audio {
        /// Verify tracked audio matches clean regeneration
        #[arg(long)]
        check: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Bootstrap { check } => {
            if let Err(err) = bootstrap::run_bootstrap(check) {
                eprintln!("bootstrap error: {err}");
                std::process::exit(1);
            }
        }
        Commands::Shaders { check } => {
            if let Err(err) = shaders::run_shaders(check) {
                eprintln!("shaders error: {err}");
                std::process::exit(1);
            }
        }
        Commands::Atlases { check } => {
            if let Err(err) = atlases::run_atlases(check) {
                eprintln!("atlases error: {err}");
                std::process::exit(1);
            }
        }
        Commands::Audio { check } => {
            let root = atlases::workspace_root_from_xtask_manifest();
            if let Err(err) = audio::run_audio(check, &root) {
                eprintln!("audio error: {err}");
                std::process::exit(1);
            }
        }
    }
}
