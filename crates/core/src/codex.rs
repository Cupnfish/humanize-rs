//! Codex command helpers for Humanize.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

use crate::constants::{
    DEFAULT_CODEX_EFFORT, DEFAULT_CODEX_MODEL, DEFAULT_CODEX_TIMEOUT_SECS, ENV_CODEX_BIN,
    ENV_CODEX_BYPASS_SANDBOX,
};

/// Configuration for Codex invocations.
#[derive(Debug, Clone)]
pub struct CodexOptions {
    pub model: String,
    pub effort: String,
    pub timeout_secs: u64,
    pub project_root: PathBuf,
    pub bypass_sandbox: bool,
}

impl Default for CodexOptions {
    fn default() -> Self {
        Self {
            model: DEFAULT_CODEX_MODEL.to_string(),
            effort: DEFAULT_CODEX_EFFORT.to_string(),
            timeout_secs: DEFAULT_CODEX_TIMEOUT_SECS,
            project_root: PathBuf::from("."),
            bypass_sandbox: false,
        }
    }
}

impl CodexOptions {
    /// Construct options using the current environment for sandbox bypass.
    pub fn from_env(project_root: impl AsRef<Path>) -> Self {
        let bypass = matches!(
            std::env::var(ENV_CODEX_BYPASS_SANDBOX).ok().as_deref(),
            Some("1") | Some("true")
        );

        Self {
            project_root: project_root.as_ref().to_path_buf(),
            bypass_sandbox: bypass,
            ..Self::default()
        }
    }
}

/// Result of a Codex command.
#[derive(Debug, Clone)]
pub struct CodexRunResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Errors from Codex command execution.
#[derive(Debug, thiserror::Error)]
pub enum CodexError {
    #[error("Codex process IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Codex exited with code {exit_code}")]
    Exit {
        exit_code: i32,
        stdout: String,
        stderr: String,
    },

    #[error("Codex timed out after {0} seconds")]
    Timeout(u64),

    #[error("Codex returned empty output")]
    EmptyOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CodexLauncher {
    Direct(OsString),
    CmdShim(PathBuf),
    PowerShellShim(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexBinaryResolution {
    pub launcher: &'static str,
    pub path: PathBuf,
}

/// Build arguments for `codex exec`.
pub fn build_exec_args(options: &CodexOptions) -> Vec<String> {
    let mut args = vec!["exec".to_string(), "-m".to_string(), options.model.clone()];
    if !options.effort.is_empty() {
        args.push("-c".to_string());
        args.push(format!("model_reasoning_effort={}", options.effort));
    }
    args.push(codex_auto_flag(options).to_string());
    args.push("-C".to_string());
    args.push(options.project_root.display().to_string());
    args.push("-".to_string());
    args
}

/// Build arguments for `codex review`.
pub fn build_review_args(base: &str, options: &CodexOptions) -> Vec<String> {
    let mut args = vec![
        "review".to_string(),
        "--base".to_string(),
        base.to_string(),
        "-c".to_string(),
        format!("model={}", options.model),
        "-c".to_string(),
        format!("review_model={}", options.model),
    ];

    if !options.effort.is_empty() {
        args.push("-c".to_string());
        args.push(format!("model_reasoning_effort={}", options.effort));
    }

    args
}

/// Determine the automation flag for Codex.
pub fn codex_auto_flag(options: &CodexOptions) -> &'static str {
    if options.bypass_sandbox {
        "--dangerously-bypass-approvals-and-sandbox"
    } else {
        "--full-auto"
    }
}

/// Run `codex exec` with the provided prompt on stdin.
pub fn run_exec(prompt: &str, options: &CodexOptions) -> Result<CodexRunResult, CodexError> {
    let mut command = codex_command()?;
    let mut child = command
        .args(build_exec_args(options))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(prompt.as_bytes())?;
    }

    let status = match child.wait_timeout(Duration::from_secs(options.timeout_secs))? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CodexError::Timeout(options.timeout_secs));
        }
    };
    let output = child.wait_with_output()?;
    let exit_code = status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if exit_code != 0 {
        return Err(CodexError::Exit {
            exit_code,
            stdout,
            stderr,
        });
    }
    if stdout.trim().is_empty() {
        return Err(CodexError::EmptyOutput);
    }

    Ok(CodexRunResult {
        stdout,
        stderr,
        exit_code,
    })
}

/// Run `codex review`.
pub fn run_review(base: &str, options: &CodexOptions) -> Result<CodexRunResult, CodexError> {
    let mut command = codex_command()?;
    let mut child = command
        .args(build_review_args(base, options))
        .current_dir(&options.project_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let status = match child.wait_timeout(Duration::from_secs(options.timeout_secs))? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CodexError::Timeout(options.timeout_secs));
        }
    };
    let output = child.wait_with_output()?;
    let exit_code = status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if exit_code != 0 {
        return Err(CodexError::Exit {
            exit_code,
            stdout,
            stderr,
        });
    }
    if stdout.trim().is_empty() && stderr.trim().is_empty() {
        return Err(CodexError::EmptyOutput);
    }

    Ok(CodexRunResult {
        stdout,
        stderr,
        exit_code,
    })
}

/// Detect `[P0]`..`[P9]` severity markers in review output.
pub fn contains_severity_markers(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.windows(4).any(|window| {
        window[0] == b'[' && window[1] == b'P' && window[2].is_ascii_digit() && window[3] == b']'
    })
}

pub fn detect_codex_binary() -> Result<CodexBinaryResolution, std::io::Error> {
    let launcher = resolve_codex_launcher()?;
    Ok(match launcher {
        CodexLauncher::Direct(program) => CodexBinaryResolution {
            launcher: "direct",
            path: PathBuf::from(program),
        },
        CodexLauncher::CmdShim(path) => CodexBinaryResolution {
            launcher: "cmd-shim",
            path,
        },
        CodexLauncher::PowerShellShim(path) => CodexBinaryResolution {
            launcher: "powershell-shim",
            path,
        },
    })
}

fn codex_command() -> Result<Command, std::io::Error> {
    let launcher = resolve_codex_launcher()?;
    Ok(command_for_launcher(&launcher))
}

fn resolve_codex_launcher() -> Result<CodexLauncher, std::io::Error> {
    if cfg!(windows) {
        resolve_windows_codex_launcher(std::env::var_os(ENV_CODEX_BIN), std::env::var_os("PATH"))
    } else {
        Ok(match std::env::var_os(ENV_CODEX_BIN) {
            Some(bin) if !bin.is_empty() => CodexLauncher::Direct(bin),
            _ => CodexLauncher::Direct(OsString::from("codex")),
        })
    }
}

fn resolve_windows_codex_launcher(
    override_bin: Option<OsString>,
    path_var: Option<OsString>,
) -> Result<CodexLauncher, std::io::Error> {
    let program = match override_bin {
        Some(bin) if !bin.is_empty() => bin,
        _ => OsString::from("codex"),
    };

    let program_path = PathBuf::from(&program);
    if program_path.is_absolute() || program_path.components().count() > 1 {
        return Ok(classify_windows_launcher(program_path));
    }

    if let Some(found) = search_windows_path(path_var.as_deref(), &program) {
        return Ok(classify_windows_launcher(found));
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!(
            "program not found (codex not found on PATH; set {} to override)",
            ENV_CODEX_BIN
        ),
    ))
}

fn search_windows_path(path_var: Option<&OsStr>, program: &OsStr) -> Option<PathBuf> {
    let path_var = path_var?;
    for dir in std::env::split_paths(path_var) {
        for candidate in windows_program_candidates(program) {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn windows_program_candidates(program: &OsStr) -> Vec<OsString> {
    if Path::new(program).extension().is_some() {
        return vec![program.to_os_string()];
    }

    let mut candidates = Vec::with_capacity(5);
    for suffix in [".exe", ".cmd", ".bat", ".ps1", ""] {
        let mut candidate = program.to_os_string();
        candidate.push(suffix);
        candidates.push(candidate);
    }
    candidates
}

fn classify_windows_launcher(path: PathBuf) -> CodexLauncher {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("cmd") | Some("bat") => CodexLauncher::CmdShim(path),
        Some("ps1") => CodexLauncher::PowerShellShim(path),
        _ => CodexLauncher::Direct(path.into_os_string()),
    }
}

fn command_for_launcher(launcher: &CodexLauncher) -> Command {
    match launcher {
        CodexLauncher::Direct(program) => Command::new(program),
        CodexLauncher::CmdShim(path) => {
            let mut command = Command::new("cmd");
            command.arg("/C").arg(path);
            command
        }
        CodexLauncher::PowerShellShim(path) => {
            let mut command = Command::new("powershell");
            command
                .arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(path);
            command
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn exec_args_match_legacy_shape() {
        let options = CodexOptions {
            model: "gpt-5.4".to_string(),
            effort: "high".to_string(),
            project_root: PathBuf::from("/tmp/project"),
            ..CodexOptions::default()
        };

        let args = build_exec_args(&options);
        assert_eq!(
            args,
            vec![
                "exec",
                "-m",
                "gpt-5.4",
                "-c",
                "model_reasoning_effort=high",
                "--full-auto",
                "-C",
                "/tmp/project",
                "-",
            ]
        );
    }

    #[test]
    fn review_args_match_legacy_shape() {
        let options = CodexOptions {
            model: "gpt-5.4".to_string(),
            effort: "high".to_string(),
            ..CodexOptions::default()
        };

        let args = build_review_args("main", &options);
        assert_eq!(
            args,
            vec![
                "review",
                "--base",
                "main",
                "-c",
                "model=gpt-5.4",
                "-c",
                "review_model=gpt-5.4",
                "-c",
                "model_reasoning_effort=high",
            ]
        );
    }

    #[test]
    fn severity_marker_detection_matches_review_contract() {
        assert!(contains_severity_markers("Issue: [P1] this is bad"));
        assert!(contains_severity_markers("[P0] blocker"));
        assert!(!contains_severity_markers("No priority markers here"));
        assert!(!contains_severity_markers("[PX] invalid"));
    }

    #[test]
    fn windows_launcher_prefers_exe_then_cmd() {
        let tempdir = TempDir::new().unwrap();
        let bin_dir = tempdir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("codex.cmd"), "").unwrap();
        std::fs::write(bin_dir.join("codex.exe"), "").unwrap();

        let path_var = std::env::join_paths([bin_dir]).unwrap();
        let launcher =
            resolve_windows_codex_launcher(None, Some(path_var)).expect("launcher should resolve");

        assert_eq!(
            launcher,
            CodexLauncher::Direct(tempdir.path().join("bin").join("codex.exe").into())
        );
    }

    #[test]
    fn windows_launcher_uses_cmd_shim_when_only_cmd_exists() {
        let tempdir = TempDir::new().unwrap();
        let bin_dir = tempdir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let cmd_path = bin_dir.join("codex.cmd");
        std::fs::write(&cmd_path, "").unwrap();

        let path_var = std::env::join_paths([bin_dir]).unwrap();
        let launcher =
            resolve_windows_codex_launcher(None, Some(path_var)).expect("launcher should resolve");

        assert_eq!(launcher, CodexLauncher::CmdShim(cmd_path));
    }

    #[test]
    fn windows_launcher_honors_override() {
        let tempdir = TempDir::new().unwrap();
        let cmd_path = tempdir.path().join("custom-codex.cmd");
        std::fs::write(&cmd_path, "").unwrap();

        let launcher = resolve_windows_codex_launcher(
            Some(cmd_path.as_os_str().to_os_string()),
            None::<OsString>,
        )
        .expect("launcher should resolve");

        assert_eq!(launcher, CodexLauncher::CmdShim(cmd_path));
    }
}
