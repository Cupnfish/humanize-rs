//! Humanize CLI
//!
//! Command-line interface for the Humanize runtime used by Claude Code and Droid.

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

mod commands;
mod hook_input;
mod init;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum InitTarget {
    Claude,
    Droid,
}

#[derive(Parser)]
#[command(name = "humanize")]
#[command(about = "Humanize CLI - Rust runtime for the Humanize host workflows", long_about = None)]
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

    /// Resume active loops from existing .humanize state
    #[command(subcommand)]
    Resume(ResumeCommands),

    /// Hook subcommands (called by plugin hooks)
    /// All hooks read JSON input from stdin
    #[command(subcommand)]
    Hook(HookCommands),

    /// Stop hook subcommands
    #[command(subcommand)]
    Stop(StopCommands),

    /// Monitor Humanize sessions
    #[command(subcommand)]
    Monitor(MonitorCommands),

    /// Gate commands for skill-mode loop enforcement
    #[command(subcommand)]
    Gate(GateCommands),

    /// Ask Codex a question
    AskCodex {
        /// The question to ask
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,

        /// Model to use
        #[arg(short, long, default_value = "gpt-5.4")]
        model: String,

        /// Effort level
        #[arg(short, long, default_value = "xhigh")]
        effort: String,

        /// Timeout in seconds
        #[arg(short, long, default_value = "3600")]
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

    /// Install or inspect host plugin integration
    Init {
        /// Install into the user's host config directory
        #[arg(short = 'g', long)]
        global: bool,

        /// Target host
        #[arg(long, value_enum, default_value = "claude")]
        target: InitTarget,

        /// Show current Humanize host integration status
        #[arg(long)]
        show: bool,

        /// Remove Humanize assets previously installed by init
        #[arg(long)]
        uninstall: bool,
    },

    /// Diagnose Humanize CLI and host plugin sync status
    Doctor {
        /// Target host. Omit to check all supported hosts.
        #[arg(long, value_enum)]
        target: Option<InitTarget>,
    },
}

#[derive(Subcommand)]
enum SetupCommands {
    /// Start an RLCR loop
    Rlcr {
        /// Path to the plan file
        plan_file: Option<String>,

        /// Explicit plan file path
        #[arg(long = "plan-file")]
        plan_file_explicit: Option<String>,

        /// Require the plan file to remain tracked in git
        #[arg(long)]
        track_plan_file: bool,

        /// Maximum iterations
        #[arg(
            short = 'm',
            long = "max-iterations",
            alias = "max",
            default_value = "42"
        )]
        max_iterations: u32,

        /// Base branch for comparison
        #[arg(short, long)]
        base_branch: Option<String>,

        /// Codex model to use
        #[arg(short, long, default_value = "gpt-5.4:xhigh")]
        codex_model: String,

        /// Timeout for each Codex invocation in seconds
        #[arg(long, default_value = "5400")]
        codex_timeout: u64,

        /// Require a git push after each round
        #[arg(long)]
        push_every_round: bool,

        /// Interval for full alignment checks
        #[arg(long, default_value = "5")]
        full_review_round: u32,

        /// Skip implementation and start directly in review mode
        #[arg(long)]
        skip_impl: bool,

        /// Let Claude answer Codex open questions directly instead of asking the user
        #[arg(long)]
        claude_answer_codex: bool,

        /// Enable agent teams mode
        #[arg(long)]
        agent_teams: bool,
    },

    /// Start a PR loop
    Pr {
        /// Monitor reviews from claude[bot]
        #[arg(long)]
        claude: bool,

        /// Monitor reviews from chatgpt-codex-connector[bot]
        #[arg(long)]
        codex: bool,

        /// Maximum iterations
        #[arg(
            short = 'm',
            long = "max-iterations",
            alias = "max",
            default_value = "42"
        )]
        max_iterations: u32,

        /// Codex model to use
        #[arg(short, long, default_value = "gpt-5.4:medium")]
        codex_model: String,

        /// Timeout for each Codex invocation in seconds
        #[arg(long, default_value = "900")]
        codex_timeout: u64,
    },
}

#[derive(Subcommand)]
enum CancelCommands {
    /// Cancel an active RLCR loop
    Rlcr {
        /// Force cancel even during Finalize Phase
        #[arg(long)]
        force: bool,
    },

    /// Cancel an active PR loop
    Pr,
}

#[derive(Subcommand)]
enum ResumeCommands {
    /// Resume the active RLCR loop
    Rlcr,

    /// Resume the active PR loop
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

#[derive(Subcommand)]
enum MonitorCommands {
    /// Monitor the latest RLCR loop
    Rlcr {
        /// Print one snapshot and exit
        #[arg(long)]
        once: bool,

        /// Refresh interval in seconds for watch mode
        #[arg(long, default_value = "2")]
        interval_secs: u64,
    },

    /// Monitor the latest PR loop
    Pr {
        /// Print one snapshot and exit
        #[arg(long)]
        once: bool,

        /// Refresh interval in seconds for watch mode
        #[arg(long, default_value = "2")]
        interval_secs: u64,
    },

    /// Monitor ask-codex skill invocations
    Skill {
        /// Print one snapshot and exit
        #[arg(long)]
        once: bool,

        /// Refresh interval in seconds for watch mode
        #[arg(long, default_value = "2")]
        interval_secs: u64,
    },
}

#[derive(Subcommand)]
enum GateCommands {
    /// RLCR stop-gate wrapper for non-hook environments
    Rlcr {
        /// Session ID forwarded to stop hook input
        #[arg(long)]
        session_id: Option<String>,

        /// Transcript path forwarded to stop hook input
        #[arg(long)]
        transcript_path: Option<String>,

        /// Project root override
        #[arg(long)]
        project_root: Option<String>,

        /// Print raw hook JSON when blocked
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if !matches!(cli.command, Commands::Init { .. } | Commands::Doctor { .. }) {
        init::warn_if_plugin_version_mismatch();
    }

    match cli.command {
        Commands::Setup(setup_cmd) => commands::handle_setup(setup_cmd),
        Commands::Cancel(cancel_cmd) => commands::handle_cancel(cancel_cmd),
        Commands::Resume(resume_cmd) => commands::handle_resume(resume_cmd),
        Commands::Hook(hook_cmd) => commands::handle_hook(hook_cmd),
        Commands::Stop(stop_cmd) => commands::handle_stop(stop_cmd),
        Commands::Monitor(monitor_cmd) => commands::handle_monitor(monitor_cmd),
        Commands::Gate(gate_cmd) => commands::handle_gate(gate_cmd),
        Commands::AskCodex {
            prompt,
            model,
            effort,
            timeout,
        } => commands::handle_ask_codex(&prompt.join(" "), &model, &effort, timeout),
        Commands::GenPlan { input, output } => commands::handle_gen_plan(&input, &output),
        Commands::Init {
            global,
            target,
            show,
            uninstall,
        } => init::run(target, global, show, uninstall),
        Commands::Doctor { target } => init::run_doctor(target),
    }
}
