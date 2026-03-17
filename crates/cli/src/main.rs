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

    /// Monitor Humanize sessions
    #[command(subcommand)]
    Monitor(MonitorCommands),

    /// Install runtime assets into a plugin root
    Install {
        /// Plugin root directory for prompt-template/, hooks/, commands/, agents/, and skills/
        #[arg(long)]
        plugin_root: Option<String>,
    },

    /// Install Humanize skills into Codex/Kimi skill directories
    InstallSkills {
        /// Target runtime(s): kimi, codex, or both
        #[arg(long, default_value = "kimi")]
        target: String,

        /// Legacy alias for the target skills directory
        #[arg(long)]
        skills_dir: Option<String>,

        /// Kimi skills directory
        #[arg(long)]
        kimi_skills_dir: Option<String>,

        /// Codex skills directory
        #[arg(long)]
        codex_skills_dir: Option<String>,

        /// Preview without writing
        #[arg(long)]
        dry_run: bool,
    },

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
        #[arg(short = 'm', long = "max-iterations", alias = "max", default_value = "42")]
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
        #[arg(short = 'm', long = "max-iterations", alias = "max", default_value = "42")]
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

    match cli.command {
        Commands::Setup(setup_cmd) => commands::handle_setup(setup_cmd),
        Commands::Cancel(cancel_cmd) => commands::handle_cancel(cancel_cmd),
        Commands::Hook(hook_cmd) => commands::handle_hook(hook_cmd),
        Commands::Stop(stop_cmd) => commands::handle_stop(stop_cmd),
        Commands::Monitor(monitor_cmd) => commands::handle_monitor(monitor_cmd),
        Commands::Install { plugin_root } => commands::handle_install(plugin_root.as_deref()),
        Commands::InstallSkills {
            target,
            skills_dir,
            kimi_skills_dir,
            codex_skills_dir,
            dry_run,
        } => commands::handle_install_skills(
            &target,
            skills_dir.as_deref(),
            kimi_skills_dir.as_deref(),
            codex_skills_dir.as_deref(),
            dry_run,
        ),
        Commands::Gate(gate_cmd) => commands::handle_gate(gate_cmd),
        Commands::AskCodex { prompt, model, effort, timeout } => {
            commands::handle_ask_codex(&prompt.join(" "), &model, &effort, timeout)
        }
        Commands::GenPlan { input, output } => {
            commands::handle_gen_plan(&input, &output)
        }
    }
}
