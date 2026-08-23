use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "plexus", about = "ToDo")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Analyse {
        #[arg(long, default_value = ".")]
        root: PathBuf,

        #[arg(long)]
        lsp_server: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Analyse { root, lsp_server } => {
            println!("Analysing {}", root.display());
        }
    }
}
