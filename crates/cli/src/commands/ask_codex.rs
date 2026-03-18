use super::pr::*;
use super::*;

pub(super) fn handle_ask_codex(
    prompt: &str,
    model: &str,
    effort: &str,
    timeout: u64,
) -> Result<()> {
    ask_codex_native(prompt, model, effort, timeout)
}

fn ask_codex_native(prompt: &str, model: &str, effort: &str, timeout: u64) -> Result<()> {
    if prompt.trim().is_empty() {
        bail!("Error: No question or task provided");
    }

    let project_root = resolve_project_root()?;
    let skill_id = unique_run_id();
    let skill_dir = project_root.join(".humanize/skill").join(&skill_id);
    fs::create_dir_all(&skill_dir)?;

    let cache_dir = resolve_cache_dir(&project_root, &skill_id, &skill_dir)?;
    let stdout_file = cache_dir.join("codex-run.out");
    let stderr_file = cache_dir.join("codex-run.log");
    let cmd_file = cache_dir.join("codex-run.cmd");

    let mut options = humanize_core::codex::CodexOptions::from_env(&project_root);
    options.model = model.to_string();
    options.effort = effort.to_string();
    options.timeout_secs = timeout;

    fs::write(
        skill_dir.join("input.md"),
        format!(
            "# Ask Codex Input\n\n## Question\n\n{}\n\n## Configuration\n\n- Model: {}\n- Effort: {}\n- Timeout: {}s\n",
            prompt, model, effort, timeout
        ),
    )?;

    let args = humanize_core::codex::build_exec_args(&options);
    fs::write(
        &cmd_file,
        format!(
            "# Codex ask-codex invocation debug info\n# Working directory: {}\n# Timeout: {} seconds\n\ncodex {}\n\n# Prompt content:\n{}\n",
            project_root.display(),
            timeout,
            args.join(" "),
            prompt
        ),
    )?;

    eprintln!(
        "ask-codex: model={} effort={} timeout={}s",
        model, effort, timeout
    );
    eprintln!("ask-codex: cache={}", cache_dir.display());
    eprintln!("ask-codex: running codex exec...");

    match humanize_core::codex::run_exec(prompt, &options) {
        Ok(result) => {
            fs::write(&stdout_file, &result.stdout)?;
            fs::write(&stderr_file, &result.stderr)?;
            fs::write(skill_dir.join("output.md"), &result.stdout)?;
            fs::write(
                skill_dir.join("metadata.md"),
                format!(
                    "---\nmodel: {}\neffort: {}\ntimeout: {}\nexit_code: 0\nstatus: success\n---\n",
                    model, effort, timeout
                ),
            )?;
            print!("{}", result.stdout);
            Ok(())
        }
        Err(humanize_core::codex::CodexError::Timeout(secs)) => {
            fs::write(
                skill_dir.join("metadata.md"),
                format!(
                    "---\nmodel: {}\neffort: {}\ntimeout: {}\nexit_code: 124\nstatus: timeout\n---\n",
                    model, effort, timeout
                ),
            )?;
            eprintln!("Error: Codex timed out after {} seconds", secs);
            std::process::exit(124);
        }
        Err(humanize_core::codex::CodexError::Exit {
            exit_code,
            stdout,
            stderr,
        }) => {
            fs::write(&stdout_file, &stdout)?;
            fs::write(&stderr_file, &stderr)?;
            fs::write(
                skill_dir.join("metadata.md"),
                format!(
                    "---\nmodel: {}\neffort: {}\ntimeout: {}\nexit_code: {}\nstatus: error\n---\n",
                    model, effort, timeout, exit_code
                ),
            )?;
            if !stderr.trim().is_empty() {
                eprintln!("{}", stderr.trim());
            }
            std::process::exit(exit_code);
        }
        Err(humanize_core::codex::CodexError::EmptyOutput) => {
            fs::write(
                skill_dir.join("metadata.md"),
                format!(
                    "---\nmodel: {}\neffort: {}\ntimeout: {}\nexit_code: 0\nstatus: empty_response\n---\n",
                    model, effort, timeout
                ),
            )?;
            bail!("Error: Codex returned empty response");
        }
        Err(err) => Err(err.into()),
    }
}
