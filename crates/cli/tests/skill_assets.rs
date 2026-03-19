use std::fs;
use std::path::PathBuf;

fn read_skill(path: &str) -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.join("../..");
    fs::read_to_string(repo_root.join(path)).unwrap()
}

#[test]
fn humanize_gen_plan_skill_is_host_driven_with_prepare_only() {
    let skill = read_skill("skills/humanize-gen-plan/SKILL.md");
    assert!(skill.contains("humanize gen-plan --prepare-only"));
    assert!(skill.contains("via AskUserQuestion"));
    assert!(skill.contains("using Edit tool"));
    assert!(!skill.contains("interactive CLI prompts"));
    assert!(!skill.contains("executes the workflow below end-to-end"));
}

#[test]
fn humanize_rlcr_skill_uses_native_cli_gate() {
    let skill = read_skill("skills/humanize-rlcr/SKILL.md");
    assert!(skill.contains("humanize gate rlcr"));
    assert!(skill.contains("humanize setup rlcr"));
    assert!(skill.contains("humanize cancel rlcr"));
    assert!(!skill.contains("{{HUMANIZE_RUNTIME_ROOT}}/scripts/rlcr-stop-gate.sh"));
}

#[test]
fn umbrella_humanize_skill_points_plan_generation_to_prepare_only_flow() {
    let skill = read_skill("skills/humanize/SKILL.md");
    assert!(skill.contains("humanize gen-plan --prepare-only"));
    assert!(skill.contains("host reasoning plus AskUserQuestion"));
    assert!(skill.contains("humanize gate rlcr"));
    assert!(!skill.contains("{{HUMANIZE_RUNTIME_ROOT}}/scripts/setup-rlcr-loop.sh"));
}
