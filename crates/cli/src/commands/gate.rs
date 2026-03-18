use super::pr::*;
use super::*;

pub(super) fn handle_gate(cmd: GateCommands) -> Result<()> {
    match cmd {
        GateCommands::Rlcr {
            session_id,
            transcript_path,
            project_root,
            json,
        } => handle_gate_rlcr(
            session_id.as_deref(),
            transcript_path.as_deref(),
            project_root.as_deref(),
            json,
        ),
    }
}

fn handle_gate_rlcr(
    session_id: Option<&str>,
    transcript_path: Option<&str>,
    project_root: Option<&str>,
    print_json: bool,
) -> Result<()> {
    let project_root = project_root
        .map(PathBuf::from)
        .unwrap_or(resolve_project_root()?);
    let input = serde_json::json!({
        "hook_event_name": "Stop",
        "stop_hook_active": false,
        "cwd": project_root.display().to_string(),
        "session_id": session_id,
        "transcript_path": transcript_path,
    });

    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let output = Command::new(&exe)
        .args(["stop", "rlcr"])
        .env("CLAUDE_PROJECT_DIR", project_root.display().to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(input.to_string().as_bytes())?;
            }
            child.wait_with_output()
        })?;

    if !output.status.success() {
        eprintln!(
            "Error: stop hook process exited with code {:?}",
            output.status.code()
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            eprintln!("{}", stderr.trim());
        }
        std::process::exit(20);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        println!("ALLOW: stop gate passed.");
        std::process::exit(0);
    }

    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("Error: stop hook returned non-JSON output");
            eprintln!("{}", stdout);
            std::process::exit(20);
        }
    };

    let decision = parsed
        .get("decision")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if decision == "block" {
        if print_json {
            println!("{}", stdout);
        } else {
            if let Some(system_message) = parsed.get("systemMessage").and_then(|v| v.as_str()) {
                if !system_message.is_empty() {
                    println!("BLOCK: {}", system_message);
                }
            }
            if let Some(reason) = parsed.get("reason").and_then(|v| v.as_str()) {
                if !reason.is_empty() {
                    println!("{}", reason);
                }
            }
        }
        std::process::exit(10);
    }

    eprintln!("Error: Unexpected hook decision: {}", decision);
    eprintln!("{}", stdout);
    std::process::exit(20);
}
