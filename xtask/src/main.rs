//! Build/bootstrap task runner shell.

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
    /// Placeholder for SDL/shader/atlas bootstrap
    Bootstrap,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Bootstrap => {
            println!("bootstrap: shell only; native deps land in later tickets");
        }
    }
}
