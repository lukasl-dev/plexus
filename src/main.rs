use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(about = "Build a graph of relationships in a codebase")]
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

        #[arg(long)]
        language: String,

        #[arg(long)]
        extension: String,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Analyse {
            root,
            lsp_server,
            language,
            extension,
        } => {
            let graph = plexus::analysis::analyse(
                &root,
                &lsp_server,
                &language,
                extension.trim_start_matches('.'),
            )
            .await?;

            println!(
                "{} nodes, {} relationships",
                graph.node_count(),
                graph.edge_count()
            );
        }
    }

    Ok(())
}
