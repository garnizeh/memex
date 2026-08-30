pub mod cli;
pub mod config;
pub mod discovery;
pub mod errors;
pub mod ingestion;
pub mod installer;
pub mod mcp;
pub mod models;
pub mod storage;

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
    /// Interactively wire Memex MCP into local AI agents (Claude Code, Cursor, Windsurf, Zed, Antigravity IDE)
    #[command(visible_alias = "register")]
    Install {
        /// Target agent(s) (e.g. claude, cursor, windsurf, zed, antigravity, all)
        #[arg(short, long)]
        target: Option<String>,

        /// Run non-interactively with default options
        #[arg(short = 'y', long)]
        yes: bool,

        /// Enable verbose MCP request debug logging into .memex/debug_mcp.log
        #[arg(long)]
        debug: bool,
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
        /// Optional path to project directory containing .memex/ (defaults to auto-detecting from current working directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Run in MCP mode communicating over stdio
        #[arg(long, default_value_t = true)]
        mcp: bool,

        /// Enable verbose MCP request debug logging into .memex/debug_mcp.log
        #[arg(long)]
        debug: bool,
    },

    /// Claude Code UserPromptSubmit prehook command for automated context injection
    #[command(name = "prompt-hook")]
    PromptHook {
        /// Enable verbose MCP request debug logging into .memex/debug_mcp.log
        #[arg(long)]
        debug: bool,
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
        Commands::Install { target, yes, debug } => {
            cli::install::run_install(target.as_deref(), yes, debug)?;
        }
        Commands::Init {
            path,
            force,
            verbose,
        } => {
            cli::init::run_init(&path, force, verbose)?;
        }
        Commands::Index {
            path,
            quiet,
            verbose,
        } => {
            cli::index::run_index(&path, quiet, verbose)?;
        }
        Commands::Serve { path, mcp, debug } => {
            cli::serve::run_serve(path.as_deref(), mcp, debug).await?;
        }
        Commands::PromptHook { debug } => {
            cli::prompt_hook::run_prompt_hook(debug)?;
        }
    }

    Ok(())
}
