mod daemon;
mod hash_service;
mod hasher;
mod persistence;
mod server;
mod transport;

use clap::{Parser, Subcommand};
use std::path::Path;

#[derive(Parser)]
#[command(name = "fswatchd")]
#[command(about = "Fast file system watcher daemon with content hashing")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compute hash of files matching a glob pattern
    Hash {
        /// Root directory
        root: String,
        /// Path relative to root
        path: String,
        /// Glob pattern to match files
        glob: String,
    },
    /// Start the daemon server
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon
    Start,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Hash { root, path, glob } => {
            let root_path = Path::new(&root);

            match hasher::list_files(root_path, &path, &glob) {
                Ok(files) => {
                    let file_count = files.len();
                    let mut hashes = Vec::with_capacity(file_count);

                    for file in files {
                        match hasher::hash_file(&file) {
                            Ok(h) => hashes.push(h),
                            Err(e) => {
                                eprintln!("Error reading {}: {}", file.display(), e);
                                return;
                            }
                        }
                    }

                    let hash = hasher::aggregate_hashes(hashes);
                    println!("{:016x}", hash);
                    eprintln!("files: {}", file_count);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                }
            }
        }
        Commands::Daemon { action } => match action {
            DaemonAction::Start => {
                if let Err(e) = server::run() {
                    eprintln!("Server error: {}", e);
                }
            }
        },
    }
}
