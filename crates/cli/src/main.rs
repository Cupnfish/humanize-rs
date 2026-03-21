//! Humanize CLI
//!
//! Command-line interface for the Humanize runtime used by Claude Code and Droid.

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, ValueEnum)]
enum PlanLockArg {
    Snapshot,
    SourceClean,
    SourceImmutable,
}
use std::io::IsTerminal;

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

        /// Internal mode: only validate IO and prepare the output scaffold
        #[arg(long, hide = true)]
        prepare_only: bool,
    },

    /// Install or inspect host plugin integration
    Init {
        /// Install into the user's host config directory instead of the current project
        #[arg(short = 'g', long)]
        global: bool,

        /// Target host
        #[arg(long, value_enum, default_value = "claude")]
        target: InitTarget,

        /// Show current Humanize host integration status
        #[arg(long)]
        show: bool,

        /// Legacy compatibility flag. Prefer `humanize uninstall`.
        #[arg(long, hide = true)]
        uninstall: bool,
    },

    /// Remove host integration previously installed by init
    Uninstall {
        /// Remove from the user's host config directory instead of the current project
        #[arg(short = 'g', long)]
        global: bool,

        /// Target host
        #[arg(long, value_enum, default_value = "claude")]
        target: InitTarget,
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

        /// Plan source lock mode for the RLCR session
        #[arg(long, value_enum, default_value = "snapshot")]
        plan_lock: PlanLockArg,

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
    Pr {
        /// Force cancel. Currently accepted for compatibility and has no additional effect.
        #[arg(long)]
        force: bool,
    },
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

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    if !matches!(
        cli.command,
        Commands::Init { .. } | Commands::Uninstall { .. } | Commands::Doctor { .. }
    ) && should_emit_plugin_sync_warnings()
    {
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
        Commands::GenPlan {
            input,
            output,
            prepare_only,
        } => commands::handle_gen_plan(&input, &output, prepare_only),
        Commands::Init {
            global,
            target,
            show,
            uninstall,
        } => {
            if uninstall {
                if show {
                    bail!("`--show` and `--uninstall` cannot be used together.");
                }
                init::run_uninstall(target, global)
            } else {
                init::run(target, global, show)
            }
        }
        Commands::Uninstall { global, target } => init::run_uninstall(target, global),
        Commands::Doctor { target } => init::run_doctor(target),
    }
}

fn should_emit_plugin_sync_warnings() -> bool {
    match parse_sync_warning_override(std::env::var("HUMANIZE_SYNC_WARNINGS").ok().as_deref()) {
        Some(value) => value,
        None => std::io::stderr().is_terminal(),
    }
}

fn parse_sync_warning_override(value: Option<&str>) -> Option<bool> {
    match value {
        Some(value) if value.eq_ignore_ascii_case("always") => Some(true),
        Some(value) if value.eq_ignore_ascii_case("never") => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_warning_override_parses_always() {
        assert_eq!(parse_sync_warning_override(Some("always")), Some(true));
        assert_eq!(parse_sync_warning_override(Some("ALWAYS")), Some(true));
    }

    #[test]
    fn sync_warning_override_parses_never() {
        assert_eq!(parse_sync_warning_override(Some("never")), Some(false));
        assert_eq!(parse_sync_warning_override(Some("NEVER")), Some(false));
    }

    #[test]
    fn sync_warning_override_ignores_unknown_values() {
        assert_eq!(parse_sync_warning_override(Some("auto")), None);
        assert_eq!(parse_sync_warning_override(Some("unexpected")), None);
        assert_eq!(parse_sync_warning_override(None), None);
    }

    #[test]
    fn uninstall_command_parses_target_and_scope() {
        let cli = Cli::try_parse_from(["humanize", "uninstall", "--global", "--target", "droid"])
            .expect("uninstall command should parse");

        match cli.command {
            Commands::Uninstall { global, target } => {
                assert!(global);
                assert_eq!(target, InitTarget::Droid);
            }
            _ => panic!("unexpected command parsed"),
        }
    }

    #[test]
    fn init_legacy_uninstall_flag_still_parses() {
        let cli = Cli::try_parse_from(["humanize", "init", "--uninstall"])
            .expect("legacy init --uninstall should parse");

        match cli.command {
            Commands::Init {
                global,
                target,
                show,
                uninstall,
            } => {
                assert!(!global);
                assert_eq!(target, InitTarget::Claude);
                assert!(!show);
                assert!(uninstall);
            }
            _ => panic!("unexpected command parsed"),
        }
    }
}
