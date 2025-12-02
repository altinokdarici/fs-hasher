mod daemon;
mod hash_service;
mod hasher;
mod logging;
mod persistence;
mod protocol;
mod server;
mod session;
mod transport;

use clap::{Parser, Subcommand};
use tracing::error;

#[derive(Parser)]
#[command(name = "fswatchd")]
#[command(about = "Fast file system watcher daemon with content hashing")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the daemon server
    Start,
}

fn main() {
    logging::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Start => {
            if let Err(e) = server::run() {
                error!("Server error: {}", e);
            }
        }
    }
}
