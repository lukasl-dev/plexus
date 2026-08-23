use std::io;
use std::path::PathBuf;
use std::process;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

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

        #[arg(long, value_enum, default_value_t = Format::Summary)]
        format: Format,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Format {
    Summary,
    Json,
    Compact,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        let broken_pipe = error
            .downcast_ref::<serde_json::Error>()
            .and_then(serde_json::Error::io_error_kind)
            == Some(io::ErrorKind::BrokenPipe)
            || error.chain().any(|cause| {
                cause
                    .downcast_ref::<io::Error>()
                    .is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
            });
        if broken_pipe {
            return;
        }

        eprintln!("Error: {error:#}");
        process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Analyse {
            root,
            lsp_server,
            language,
            extension,
            format,
        } => {
            let graph = plexus::analysis::analyse(
                &root,
                &lsp_server,
                &language,
                extension.trim_start_matches('.'),
            )
            .await?;

            let stdout = io::stdout();
            let mut output = stdout.lock();
            match format {
                Format::Summary => plexus::output::write_summary(&graph, &mut output)?,
                Format::Json => plexus::output::write_json(&graph, &root, &mut output)?,
                Format::Compact => plexus::output::write_compact(&graph, &root, &mut output)?,
            }
        }
    }

    Ok(())
}
