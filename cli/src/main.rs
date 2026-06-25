//! BinBook device CLI tool
//!
//! Host-side tool for managing the Xteink X4 device via serial connection.
//! For now, this is a placeholder that will be filled in Task 10.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "binbook-cli")]
#[command(about = "CLI tool for BinBook device management")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Flash firmware to device
    Flash {
        /// Serial port
        #[arg(short, long)]
        port: String,

        /// Path to firmware binary
        #[arg(short, long)]
        firmware: PathBuf,
    },
    /// Upload a binbook file to device
    Upload {
        /// Serial port
        #[arg(short, long)]
        port: String,

        /// Path to binbook file
        #[arg(short, long)]
        file: PathBuf,

        /// Filename on device
        #[arg(short, long)]
        name: String,
    },
    /// List books stored on device
    List {
        /// Serial port
        #[arg(short, long)]
        port: String,
    },
    /// Delete a book from device
    Delete {
        /// Serial port
        #[arg(short, long)]
        port: String,

        /// Book name or ID
        #[arg(short, long)]
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Flash { port, firmware } => {
            println!("Flashing {} to {}...", firmware.display(), port);
            // TODO: Implement in Task 10
        }
        Commands::Upload { port, file, name } => {
            println!("Uploading {} as {} to {}...", file.display(), name, port);
            // TODO: Implement in Task 10
        }
        Commands::List { port } => {
            println!("Listing books on {}...", port);
            // TODO: Implement in Task 10
        }
        Commands::Delete { port, name } => {
            println!("Deleting {} from {}...", name, port);
            // TODO: Implement in Task 10
        }
    }
}
