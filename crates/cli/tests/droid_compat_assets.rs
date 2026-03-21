use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_file(path: &str) -> String {
    let path = repo_root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("failed to read {}", path.display()))
}

#[test]
fn plugin_agents_do_not_pin_models_in_frontmatter() {
    for path in [
        "agents/plan-compliance-checker.md",
        "agents/draft-relevance-checker.md",
    ] {
        let content = read_file(path);
        assert!(
            !content.contains("\nmodel:"),
            "{path} pins a model in frontmatter; plugin subagents should inherit the host/default model"
        );
    }
}

#[test]
fn command_docs_do_not_pin_task_subagent_models() {
    for path in ["commands/start-rlcr-loop.md", "commands/gen-plan.md"] {
        let content = read_file(path);
        assert!(
            !content.contains("- model: "),
            "{path} pins a Task subagent model; plugin commands should inherit the host/default model"
        );
    }
}
