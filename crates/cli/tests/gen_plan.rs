use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct GenPlanEnv {
    _tempdir: TempDir,
    project_dir: PathBuf,
    bin_dir: PathBuf,
}

impl GenPlanEnv {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let project_dir = root.join("project");
        let bin_dir = root.join("bin");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();
        Self {
            _tempdir: tempdir,
            project_dir,
            bin_dir,
        }
    }

    fn project(&self) -> &Path {
        &self.project_dir
    }

    fn path_env(&self) -> String {
        format!(
            "{}:{}",
            self.bin_dir.display(),
            std::env::var("PATH").unwrap()
        )
    }

    fn mock_codex(&self, script: &str) {
        let path = self.bin_dir.join("codex");
        fs::write(&path, script).unwrap();
        make_executable(&path);
    }
}

#[test]
fn gen_plan_uses_codex_output_and_keeps_original_draft_section() {
    let env = GenPlanEnv::new();
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let counter_file = env.project().join("codex-count");
    fs::write(
        env.project().join("draft.md"),
        "# Draft\n\nNeed a parser.\n",
    )
    .unwrap();
    fs::create_dir_all(env.project().join("docs")).unwrap();
    env.mock_codex(
        "#!/bin/bash\ncount_file=\"${MOCK_CODEX_COUNTER_FILE:?}\"\ncount=0\n[[ -f \"$count_file\" ]] && count=$(cat \"$count_file\")\ncount=$((count + 1))\nprintf '%s' \"$count\" > \"$count_file\"\nif [[ \"$1\" == \"exec\" ]]; then\n  cat >/dev/null\n  if [[ \"$count\" == \"1\" ]]; then\n    printf 'RELEVANT: parser work matches this repository\\n'\n  elif [[ \"$count\" == \"2\" ]]; then\n    printf '{\"issues\":[],\"metrics\":[],\"mixed_languages\":false,\"language_candidates\":[],\"notes\":[]}'\n  else\n    cat <<'EOF'\n```markdown\n# Parser Plan\n\n## Goal Description\nBuild the parser.\n\n## Acceptance Criteria\n- AC-1: Parse valid inputs\n  - Positive Tests (expected to PASS):\n    - accepts valid example\n  - Negative Tests (expected to FAIL):\n    - rejects malformed example\n```\nEOF\n  fi\nfi\n",
    );

    let output = Command::new(bin())
        .args([
            "gen-plan",
            "--input",
            "draft.md",
            "--output",
            "docs/plan.md",
        ])
        .env("PATH", env.path_env())
        .env(
            "MOCK_CODEX_COUNTER_FILE",
            counter_file.display().to_string(),
        )
        .env("CLAUDE_PLUGIN_ROOT", plugin_root.display().to_string())
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .current_dir(env.project())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let plan = fs::read_to_string(env.project().join("docs/plan.md")).unwrap();
    assert!(plan.contains("# Parser Plan"));
    assert!(plan.contains("AC-1"));
    assert!(plan.contains("--- Original Design Draft Start ---"));
    assert!(plan.contains("Need a parser."));
}

#[test]
fn gen_plan_blocks_when_noninteractive_clarification_is_required() {
    let env = GenPlanEnv::new();
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let counter_file = env.project().join("codex-count");
    fs::write(
        env.project().join("draft.md"),
        "# Draft\n\nNeed something vague.\n",
    )
    .unwrap();
    fs::create_dir_all(env.project().join("docs")).unwrap();
    env.mock_codex(
        "#!/bin/bash\ncount_file=\"${MOCK_CODEX_COUNTER_FILE:?}\"\ncount=0\n[[ -f \"$count_file\" ]] && count=$(cat \"$count_file\")\ncount=$((count + 1))\nprintf '%s' \"$count\" > \"$count_file\"\nif [[ \"$1\" == \"exec\" ]]; then\n  cat >/dev/null\n  if [[ \"$count\" == \"1\" ]]; then\n    printf 'RELEVANT: related\\n'\n  else\n    printf '{\"issues\":[{\"question\":\"Which parser format?\",\"why\":\"Plan is ambiguous\",\"options\":[\"json\",\"yaml\"]}],\"metrics\":[],\"mixed_languages\":false,\"language_candidates\":[],\"notes\":[]}'\n  fi\nfi\n",
    );

    let output = Command::new(bin())
        .args([
            "gen-plan",
            "--input",
            "draft.md",
            "--output",
            "docs/plan.md",
        ])
        .env("PATH", env.path_env())
        .env(
            "MOCK_CODEX_COUNTER_FILE",
            counter_file.display().to_string(),
        )
        .env("CLAUDE_PLUGIN_ROOT", plugin_root.display().to_string())
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .current_dir(env.project())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires user clarification"));
}

#[test]
fn gen_plan_prepare_only_writes_template_and_draft_without_codex() {
    let env = GenPlanEnv::new();
    fs::write(
        env.project().join("draft.md"),
        "# Draft\n\nNeed a parser.\n",
    )
    .unwrap();
    fs::create_dir_all(env.project().join("docs")).unwrap();

    let output = Command::new(bin())
        .args([
            "gen-plan",
            "--input",
            "draft.md",
            "--output",
            "docs/plan.md",
            "--prepare-only",
        ])
        .env("PATH", env.path_env())
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .current_dir(env.project())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let plan = fs::read_to_string(env.project().join("docs/plan.md")).unwrap();
    assert!(plan.contains("## Goal Description"));
    assert!(plan.contains("## Acceptance Criteria"));
    assert!(plan.contains("--- Original Design Draft Start ---"));
    assert!(plan.contains("Need a parser."));
}

#[test]
fn gen_plan_prepare_only_accepts_host_orchestration_flags() {
    let env = GenPlanEnv::new();
    fs::write(
        env.project().join("draft.md"),
        "# Draft\n\nNeed a parser.\n",
    )
    .unwrap();
    fs::create_dir_all(env.project().join("docs")).unwrap();

    let output = Command::new(bin())
        .args([
            "gen-plan",
            "--input",
            "draft.md",
            "--output",
            "docs/plan.md",
            "--prepare-only",
            "--discussion",
            "--auto-start-rlcr-if-converged",
        ])
        .env("PATH", env.path_env())
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .current_dir(env.project())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn gen_plan_rejects_discussion_and_direct_together() {
    let env = GenPlanEnv::new();
    fs::write(
        env.project().join("draft.md"),
        "# Draft\n\nNeed a parser.\n",
    )
    .unwrap();
    fs::create_dir_all(env.project().join("docs")).unwrap();

    let output = Command::new(bin())
        .args([
            "gen-plan",
            "--input",
            "draft.md",
            "--output",
            "docs/plan.md",
            "--prepare-only",
            "--discussion",
            "--direct",
        ])
        .env("PATH", env.path_env())
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .current_dir(env.project())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Cannot use --discussion and --direct together"));
}

#[test]
fn gen_plan_prepare_only_accepts_output_in_current_dir() {
    let env = GenPlanEnv::new();
    fs::write(
        env.project().join("draft.md"),
        "# Draft\n\nNeed a parser.\n",
    )
    .unwrap();

    let output = Command::new(bin())
        .args([
            "gen-plan",
            "--input",
            "draft.md",
            "--output",
            "plan.md",
            "--prepare-only",
        ])
        .env("PATH", env.path_env())
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .current_dir(env.project())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(env.project().join("plan.md").is_file());
}

#[test]
fn gen_draft_creates_internal_artifact() {
    let env = GenPlanEnv::new();
    fs::write(
        env.project().join("draft.md"),
        "# Parser Draft\n\nNeed a parser.\n",
    )
    .unwrap();

    let output = Command::new(bin())
        .args([
            "gen-draft",
            "--input",
            "draft.md",
            "--title",
            "Parser Draft",
        ])
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .env("CLAUDE_SESSION_ID", "thread-planning")
        .current_dir(env.project())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Draft Handle: parser-draft"),
        "stdout={stdout}"
    );

    let index = read_json(env.project().join(".humanize/planning/index.json"));
    let drafts = index.get("drafts").and_then(Value::as_array).unwrap();
    assert_eq!(drafts.len(), 1);
    assert_eq!(
        drafts[0].get("handle").and_then(Value::as_str),
        Some("parser-draft")
    );
}

#[test]
fn gen_plan_without_args_uses_pending_draft_artifact() {
    let env = GenPlanEnv::new();
    let counter_file = env.project().join("codex-count");
    fs::write(
        env.project().join("draft.md"),
        "# Parser Draft\n\nNeed a parser.\n",
    )
    .unwrap();
    env.mock_codex(
        "#!/bin/bash\ncount_file=\"${MOCK_CODEX_COUNTER_FILE:?}\"\ncount=0\n[[ -f \"$count_file\" ]] && count=$(cat \"$count_file\")\ncount=$((count + 1))\nprintf '%s' \"$count\" > \"$count_file\"\nif [[ \"$1\" == \"exec\" ]]; then\n  cat >/dev/null\n  if [[ \"$count\" == \"1\" ]]; then\n    printf 'RELEVANT: parser work matches this repository\\n'\n  elif [[ \"$count\" == \"2\" ]]; then\n    printf '{\"issues\":[],\"metrics\":[],\"mixed_languages\":false,\"language_candidates\":[],\"notes\":[]}'\n  else\n    cat <<'EOF'\n```markdown\n# Parser Plan\n\n## Goal Description\nBuild the parser.\n\n## Acceptance Criteria\n- AC-1: Parse valid inputs\n  - Positive Tests (expected to PASS):\n    - accepts valid example\n  - Negative Tests (expected to FAIL):\n    - rejects malformed example\n```\nEOF\n  fi\nfi\n",
    );

    let draft = Command::new(bin())
        .args([
            "gen-draft",
            "--input",
            "draft.md",
            "--title",
            "Parser Draft",
        ])
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .env("CLAUDE_SESSION_ID", "thread-planning")
        .current_dir(env.project())
        .output()
        .unwrap();
    assert!(draft.status.success());

    let output = Command::new(bin())
        .args(["gen-plan"])
        .env("PATH", env.path_env())
        .env(
            "MOCK_CODEX_COUNTER_FILE",
            counter_file.display().to_string(),
        )
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .env("CLAUDE_SESSION_ID", "thread-planning")
        .current_dir(env.project())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Draft Handle: parser-draft"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("Plan Handle: parser-draft"),
        "stdout={stdout}"
    );

    let index = read_json(env.project().join(".humanize/planning/index.json"));
    let plans = index.get("plans").and_then(Value::as_array).unwrap();
    assert_eq!(plans.len(), 1);
    let plan_id = plans[0].get("id").and_then(Value::as_str).unwrap();
    let plan_path = env
        .project()
        .join(".humanize/planning/plans")
        .join(plan_id)
        .join("plan.md");
    let plan = fs::read_to_string(plan_path).unwrap();
    assert!(plan.contains("# Parser Plan"));
    assert!(plan.contains("--- Original Design Draft Start ---"));
}

#[test]
fn gen_plan_without_args_errors_when_no_pending_draft_exists() {
    let env = GenPlanEnv::new();
    let counter_file = env.project().join("codex-count");
    fs::write(
        env.project().join("draft.md"),
        "# Parser Draft\n\nNeed a parser.\n",
    )
    .unwrap();
    env.mock_codex(
        "#!/bin/bash\ncount_file=\"${MOCK_CODEX_COUNTER_FILE:?}\"\ncount=0\n[[ -f \"$count_file\" ]] && count=$(cat \"$count_file\")\ncount=$((count + 1))\nprintf '%s' \"$count\" > \"$count_file\"\nif [[ \"$1\" == \"exec\" ]]; then\n  cat >/dev/null\n  if [[ \"$count\" == \"1\" ]]; then\n    printf 'RELEVANT: parser work matches this repository\\n'\n  elif [[ \"$count\" == \"2\" ]]; then\n    printf '{\"issues\":[],\"metrics\":[],\"mixed_languages\":false,\"language_candidates\":[],\"notes\":[]}'\n  else\n    printf '# Parser Plan\\n\\n## Goal Description\\nBuild the parser.\\n\\n## Acceptance Criteria\\n- AC-1: Parse valid inputs\\n'\n  fi\nfi\n",
    );

    let draft = Command::new(bin())
        .args([
            "gen-draft",
            "--input",
            "draft.md",
            "--title",
            "Parser Draft",
        ])
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .env("CLAUDE_SESSION_ID", "thread-planning")
        .current_dir(env.project())
        .output()
        .unwrap();
    assert!(draft.status.success());

    let first_plan = Command::new(bin())
        .args(["gen-plan"])
        .env("PATH", env.path_env())
        .env(
            "MOCK_CODEX_COUNTER_FILE",
            counter_file.display().to_string(),
        )
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .env("CLAUDE_SESSION_ID", "thread-planning")
        .current_dir(env.project())
        .output()
        .unwrap();
    assert!(first_plan.status.success());

    let second_plan = Command::new(bin())
        .args(["gen-plan"])
        .env("PATH", env.path_env())
        .env(
            "MOCK_CODEX_COUNTER_FILE",
            counter_file.display().to_string(),
        )
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .env("CLAUDE_SESSION_ID", "thread-planning")
        .current_dir(env.project())
        .output()
        .unwrap();

    assert!(!second_plan.status.success());
    let stderr = String::from_utf8_lossy(&second_plan.stderr);
    assert!(
        stderr.contains("No draft pending plan generation"),
        "stderr={stderr}"
    );
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }
}

fn read_json(path: PathBuf) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}
