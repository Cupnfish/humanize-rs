//! Humanize CLI
//!
//! Command-line interface for the Humanize Claude Code plugin.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod hook_input;

#[derive(Parser)]
#[command(name = "humanize")]
#[command(about = "Humanize CLI - Claude Code plugin for iterative development", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Setup commands for starting loops
    #[command(subcommand)]
    Setup(SetupCommands),

    /// Cancel active loops
    #[command(subcommand)]
    Cancel(CancelCommands),

    /// Hook subcommands (called by Claude Code hooks)
    /// All hooks read JSON input from stdin
    #[command(subcommand)]
    Hook(HookCommands),

    /// Stop hook subcommands
    #[command(subcommand)]
    Stop(StopCommands),

    /// Ask Codex a question
    AskCodex {
        /// The question to ask
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,

        /// Model to use
        #[arg(short, long, default_value = "gpt-5.4")]
        model: String,

        /// Effort level
        #[arg(short, long, default_value = "high")]
        effort: String,

        /// Timeout in seconds
        #[arg(short, long, default_value = "5400")]
        timeout: u64,
    },

    /// Generate implementation plan from draft
    GenPlan {
        /// Input draft file
        #[arg(short, long)]
        input: String,

        /// Output plan file
        #[arg(short, long)]
        output: String,
    },
}

#[derive(Subcommand)]
enum SetupCommands {
    /// Start an RLCR loop
    Rlcr {
        /// Path to the plan file
        plan_file: String,

        /// Maximum iterations
        #[arg(short, long, default_value = "42")]
        max_iterations: u32,

        /// Base branch for comparison
        #[arg(short, long)]
        base_branch: Option<String>,

        /// Codex model to use
        #[arg(short, long, default_value = "gpt-5.4")]
        codex_model: String,

        /// Enable agent teams mode
        #[arg(long)]
        agent_teams: bool,
    },

    /// Start a PR loop
    Pr {
        /// PR URL (e.g., https://github.com/owner/repo/pull/123)
        pr_url: String,
    },
}

#[derive(Subcommand)]
enum CancelCommands {
    /// Cancel an active RLCR loop
    Rlcr,

    /// Cancel an active PR loop
    Pr,
}

#[derive(Subcommand)]
enum HookCommands {
    /// Read validator hook - reads JSON from stdin
    ReadValidator,

    /// Write validator hook - reads JSON from stdin
    WriteValidator,

    /// Edit validator hook - reads JSON from stdin
    EditValidator,

    /// Bash command validator hook - reads JSON from stdin
    BashValidator,

    /// Plan file validator hook - reads JSON from stdin
    PlanFileValidator,

    /// PostToolUse hook for session handshake - reads JSON from stdin
    PostToolUse,
}

#[derive(Subcommand)]
enum StopCommands {
    /// RLCR stop hook
    Rlcr,

    /// PR loop stop hook
    Pr,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Setup(setup_cmd) => commands::handle_setup(setup_cmd),
        Commands::Cancel(cancel_cmd) => commands::handle_cancel(cancel_cmd),
        Commands::Hook(hook_cmd) => commands::handle_hook(hook_cmd),
        Commands::Stop(stop_cmd) => commands::handle_stop(stop_cmd),
        Commands::AskCodex { prompt, model, effort, timeout } => {
            commands::handle_ask_codex(&prompt.join(" "), &model, &effort, timeout)
        }
        Commands::GenPlan { input, output } => {
            commands::handle_gen_plan(&input, &output)
        }
    }
}
