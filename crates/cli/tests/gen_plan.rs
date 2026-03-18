use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }
}
