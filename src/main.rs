pub mod errors;
pub mod models;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "memex",
    version,
    about = "Local, offline documentation context server (MCP) for LLMs & AI coding agents",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Interactively wire Memex MCP into local AI agents (Claude Code, Cursor, etc.)
    Install {
        /// Target agent(s) (e.g. claude, cursor, auto)
        #[arg(short, long)]
        target: Option<String>,

        /// Run non-interactively with default options
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Initialize Memex in a project directory and build the initial index
    Init {
        /// Target project path (defaults to current working directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Initialize even if the path looks like a filesystem root or home directory
        #[arg(short, long)]
        force: bool,

        /// Show verbose progress and memory statistics
        #[arg(short, long)]
        verbose: bool,
    },

    /// Incrementally update the index with changes since the last run
    Index {
        /// Target project path (defaults to current working directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Suppress non-error output (useful for git hooks)
        #[arg(short, long)]
        quiet: bool,

        /// Show verbose progress and memory statistics
        #[arg(short, long)]
        verbose: bool,
    },

    /// Start the MCP stdio JSON-RPC server
    Serve {
        /// Run in MCP mode communicating over stdio
        #[arg(long, default_value_t = true)]
        mcp: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Ensure all logging goes to stderr so stdout remains clean for MCP JSON-RPC protocol
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Install { target, yes } => {
            eprintln!("Installing Memex MCP (target: {:?}, non-interactive: {})", target, yes);
            // TODO: Call installer module
        }
        Commands::Init { path, force, verbose } => {
            eprintln!("Initializing Memex at {:?} (force: {}, verbose: {})", path, force, verbose);
            // TODO: Call init pipeline
        }
        Commands::Index { path, quiet, verbose } => {
            if !quiet {
                eprintln!("Indexing documentation at {:?} (verbose: {})", path, verbose);
            }
            // TODO: Call index pipeline
        }
        Commands::Serve { mcp } => {
            if mcp {
                eprintln!("Starting Memex MCP server on stdio...");
                // TODO: Start stdio MCP event loop
            }
        }
    }

    Ok(())
}
