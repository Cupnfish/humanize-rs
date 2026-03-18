use anyhow::{Context, Result, bail};
use include_dir::{Dir, DirEntry};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

use crate::InitTarget;

static HOST_ASSETS: Dir<'_> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/assets/claude");
const SETTINGS_BACKUP_FILE: &str = "settings.json.bak";
const COMMAND_FILE_MAP: &[(&str, &str)] = &[
    ("commands/gen-plan.md", "humanize-gen-plan.md"),
    ("commands/start-rlcr-loop.md", "humanize-start-rlcr-loop.md"),
    (
        "commands/resume-rlcr-loop.md",
        "humanize-resume-rlcr-loop.md",
    ),
    ("commands/start-pr-loop.md", "humanize-start-pr-loop.md"),
    ("commands/resume-pr-loop.md", "humanize-resume-pr-loop.md"),
    (
        "commands/cancel-rlcr-loop.md",
        "humanize-cancel-rlcr-loop.md",
    ),
    ("commands/cancel-pr-loop.md", "humanize-cancel-pr-loop.md"),
];
const AGENT_FILES: &[&str] = &[
    "agents/draft-relevance-checker.md",
    "agents/plan-compliance-checker.md",
];
const SKILL_DIRS: &[&str] = &[
    "skills/humanize",
    "skills/humanize-rlcr",
    "skills/humanize-gen-plan",
    "skills/ask-codex",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatchMode {
    Ask,
    Auto,
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatchResult {
    Patched { added_entries: usize },
    AlreadyPresent,
    Declined,
    Skipped,
}

pub fn run(
    target: InitTarget,
    global: bool,
    auto_patch: bool,
    no_patch: bool,
    show: bool,
    uninstall: bool,
) -> Result<()> {
    if !global {
        bail!("Only `humanize init --global` is supported right now.");
    }

    let patch_mode = if auto_patch {
        PatchMode::Auto
    } else if no_patch {
        PatchMode::Skip
    } else {
        PatchMode::Ask
    };

    if show && uninstall {
        bail!("`--show` and `--uninstall` cannot be used together.");
    }

    if show {
        return show_config(target);
    }

    if uninstall {
        return uninstall_global(target);
    }

    install_global(target, patch_mode)
}

fn install_global(target: InitTarget, patch_mode: PatchMode) -> Result<()> {
    let host_dir = resolve_host_dir(target)?;
    fs::create_dir_all(&host_dir)
        .with_context(|| format!("Failed to create host config dir: {}", host_dir.display()))?;

    let commands_dir = host_dir.join("commands");
    let workers_dir = host_dir.join(worker_dir_name(target));
    let skills_dir = host_dir.join("skills");
    fs::create_dir_all(&commands_dir)
        .with_context(|| format!("Failed to create {}", commands_dir.display()))?;
    fs::create_dir_all(&workers_dir)
        .with_context(|| format!("Failed to create {}", workers_dir.display()))?;
    fs::create_dir_all(&skills_dir)
        .with_context(|| format!("Failed to create {}", skills_dir.display()))?;

    let commands_changed = install_commands(target, &commands_dir)?;
    let workers_changed = install_workers(target, &workers_dir)?;
    let skills_changed = install_skills(&skills_dir)?;

    println!(
        "\nHumanize assets installed for {} (global).\n",
        host_display_name(target)
    );
    println!(
        "  Commands:  {} ({} files)",
        commands_dir.display(),
        COMMAND_FILE_MAP.len()
    );
    println!(
        "  {}:   {} ({} files)",
        worker_label(target),
        workers_dir.display(),
        AGENT_FILES.len()
    );
    println!(
        "  Skills:    {} ({} skills)",
        skills_dir.display(),
        SKILL_DIRS.len()
    );
    if commands_changed == 0 && workers_changed == 0 && skills_changed == 0 {
        println!("\n  Asset files were already up to date.");
    }

    match patch_settings_json(&host_dir.join("settings.json"), patch_mode, target)? {
        PatchResult::Patched { added_entries } => {
            println!(
                "\n  settings.json: added {} Humanize hook entries",
                added_entries
            );
            let backup_path = host_dir.join(SETTINGS_BACKUP_FILE);
            if backup_path.exists() {
                println!("  Backup: {}", backup_path.display());
            }
            println!(
                "  Restart {}. Test with: git status",
                host_display_name(target)
            );
        }
        PatchResult::AlreadyPresent => {
            println!("\n  settings.json: Humanize hooks already present");
            println!(
                "  Restart {}. Test with: git status",
                host_display_name(target)
            );
        }
        PatchResult::Declined | PatchResult::Skipped => {}
    }

    println!();
    Ok(())
}

fn show_config(target: InitTarget) -> Result<()> {
    let host_dir = resolve_host_dir(target)?;
    let settings_path = host_dir.join("settings.json");

    println!("Humanize {} configuration:\n", host_display_name(target));

    let command_count = installed_command_count(target, &host_dir.join("commands"))?;
    println!(
        "  Commands:   {}/{} installed",
        command_count,
        COMMAND_FILE_MAP.len()
    );

    let worker_count = installed_worker_count(target, &host_dir.join(worker_dir_name(target)))?;
    println!(
        "  {}:    {}/{} installed",
        worker_label(target),
        worker_count,
        AGENT_FILES.len()
    );

    let skill_count = installed_skill_count(&host_dir.join("skills"))?;
    println!(
        "  Skills:     {}/{} installed",
        skill_count,
        SKILL_DIRS.len()
    );

    let hook_count = installed_hook_count(&settings_path)?;
    println!(
        "  Hooks:      {}/{} configured",
        hook_count,
        expected_hook_entry_count()
    );

    if settings_path.exists() {
        println!("  settings:   {}", settings_path.display());
    } else {
        println!("  settings:   {} (missing)", settings_path.display());
    }

    println!("\nUsage:");
    println!("  humanize init --global");
    println!("  humanize init --global --target droid");
    println!("  humanize init --global --auto-patch");
    println!("  humanize init --global --target droid --auto-patch");
    println!("  humanize init --global --show");
    println!("  humanize init --global --target droid --show");
    println!("  humanize init --global --uninstall");
    println!("  humanize init --global --target droid --uninstall");

    Ok(())
}

fn uninstall_global(target: InitTarget) -> Result<()> {
    let host_dir = resolve_host_dir(target)?;
    let mut removed = Vec::new();

    for (_, dest_name) in COMMAND_FILE_MAP {
        let path = host_dir.join("commands").join(dest_name);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
            removed.push(format!("Command: {}", path.display()));
        }
    }

    for source_path in AGENT_FILES {
        let file_name = Path::new(source_path)
            .file_name()
            .context("Worker asset missing file name")?;
        let path = host_dir.join(worker_dir_name(target)).join(file_name);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
            removed.push(format!(
                "{}: {}",
                worker_label_singular(target),
                path.display()
            ));
        }
    }

    for skill_dir in SKILL_DIRS {
        let dir_name = Path::new(skill_dir)
            .file_name()
            .context("Skill asset missing directory name")?;
        let path = host_dir.join("skills").join(dir_name);
        if path.exists() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
            removed.push(format!("Skill: {}", path.display()));
        }
    }

    let removed_hooks = remove_humanize_hooks(&host_dir.join("settings.json"))?;
    if removed_hooks > 0 {
        removed.push(format!(
            "settings.json: removed {} hook entries",
            removed_hooks
        ));
    }

    if removed.is_empty() {
        println!(
            "Humanize was not installed in {} (nothing to remove).",
            host_dir.display()
        );
    } else {
        println!("Humanize uninstalled from {}:", host_display_name(target));
        for item in removed {
            println!("  - {}", item);
        }
        println!("\nRestart {} to apply changes.", host_display_name(target));
    }

    Ok(())
}

fn install_commands(target: InitTarget, dest_dir: &Path) -> Result<usize> {
    let mut changed = 0;
    for (source_path, dest_name) in COMMAND_FILE_MAP {
        let content = rendered_command_content(target, source_path)?;
        if write_text_if_changed(&dest_dir.join(dest_name), &content)? {
            changed += 1;
        }
    }
    Ok(changed)
}

fn install_workers(target: InitTarget, dest_dir: &Path) -> Result<usize> {
    let mut changed = 0;
    for source_path in AGENT_FILES {
        let file_name = Path::new(source_path)
            .file_name()
            .context("Worker asset missing file name")?;
        let content = rendered_worker_content(target, source_path)?;
        if write_text_if_changed(&dest_dir.join(file_name), &content)? {
            changed += 1;
        }
    }
    Ok(changed)
}

fn install_skills(dest_dir: &Path) -> Result<usize> {
    let mut changed = 0;
    for skill_dir in SKILL_DIRS {
        let dir = HOST_ASSETS
            .get_dir(skill_dir)
            .with_context(|| format!("Missing skill asset directory: {skill_dir}"))?;
        changed += install_directory_recursive(dir, "skills", dest_dir)?;
    }
    Ok(changed)
}

fn install_directory_recursive(
    dir: &Dir<'_>,
    root_prefix: &str,
    dest_root: &Path,
) -> Result<usize> {
    let mut changed = 0;
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(subdir) => {
                changed += install_directory_recursive(subdir, root_prefix, dest_root)?;
            }
            DirEntry::File(file) => {
                let relative_path = file.path().strip_prefix(root_prefix).with_context(|| {
                    format!(
                        "Failed to strip asset prefix from {}",
                        file.path().display()
                    )
                })?;
                let dest_path = dest_root.join(relative_path);
                let content = file.contents_utf8().with_context(|| {
                    format!("Asset is not valid UTF-8: {}", file.path().display())
                })?;
                if write_text_if_changed(&dest_path, content)? {
                    changed += 1;
                }
            }
        }
    }
    Ok(changed)
}

fn rendered_command_content(target: InitTarget, source_path: &str) -> Result<String> {
    let raw = asset_utf8(source_path)?;
    match target {
        InitTarget::Claude => Ok(raw.to_string()),
        InitTarget::Droid => transform_droid_command(raw),
    }
}

fn rendered_worker_content(target: InitTarget, source_path: &str) -> Result<String> {
    let raw = asset_utf8(source_path)?;
    match target {
        InitTarget::Claude => Ok(raw.to_string()),
        InitTarget::Droid => transform_droid_worker(raw),
    }
}

fn transform_droid_command(content: &str) -> Result<String> {
    let (frontmatter, body) = parse_markdown_frontmatter(content)?;
    let mut filtered = YamlMapping::new();

    if let Some(frontmatter) = frontmatter {
        copy_yaml_key(&frontmatter, &mut filtered, "description");
        copy_yaml_key(&frontmatter, &mut filtered, "argument-hint");
    }

    render_markdown_with_frontmatter(&filtered, body)
}

fn transform_droid_worker(content: &str) -> Result<String> {
    let (frontmatter, body) = parse_markdown_frontmatter(content)?;
    let mut filtered = YamlMapping::new();

    if let Some(frontmatter) = frontmatter {
        copy_yaml_key(&frontmatter, &mut filtered, "name");
        copy_yaml_key(&frontmatter, &mut filtered, "description");

        let tools_key = yaml_string("tools");
        let tools = parse_tools(frontmatter.get(&tools_key));
        if !tools.is_empty() {
            filtered.insert(
                tools_key,
                YamlValue::Sequence(tools.into_iter().map(YamlValue::String).collect()),
            );
        }
    }

    filtered.insert(
        yaml_string("model"),
        YamlValue::String("inherit".to_string()),
    );

    render_markdown_with_frontmatter(&filtered, body)
}

fn parse_markdown_frontmatter(content: &str) -> Result<(Option<YamlMapping>, &str)> {
    let Some(rest) = content.strip_prefix("---\n") else {
        return Ok((None, content));
    };
    let Some(end_index) = rest.find("\n---\n") else {
        return Ok((None, content));
    };

    let yaml_text = &rest[..end_index];
    let body = &rest[end_index + 5..];
    let frontmatter: YamlMapping =
        serde_yaml::from_str(yaml_text).context("Failed to parse markdown frontmatter")?;
    Ok((Some(frontmatter), body))
}

fn render_markdown_with_frontmatter(frontmatter: &YamlMapping, body: &str) -> Result<String> {
    if frontmatter.is_empty() {
        return Ok(body.trim_start().to_string());
    }

    let yaml = serde_yaml::to_string(frontmatter).context("Failed to serialize frontmatter")?;
    Ok(format!("---\n{}---\n\n{}", yaml, body.trim_start()))
}

fn copy_yaml_key(source: &YamlMapping, dest: &mut YamlMapping, key: &str) {
    let yaml_key = yaml_string(key);
    if let Some(value) = source.get(&yaml_key) {
        dest.insert(yaml_key, value.clone());
    }
}

fn parse_tools(value: Option<&YamlValue>) -> Vec<String> {
    match value {
        Some(YamlValue::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Some(YamlValue::Sequence(items)) => items
            .iter()
            .filter_map(YamlValue::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn yaml_string(key: &str) -> YamlValue {
    YamlValue::String(key.to_string())
}

fn patch_settings_json(
    settings_path: &Path,
    mode: PatchMode,
    target: InitTarget,
) -> Result<PatchResult> {
    let mut root = read_settings_json(settings_path)?;
    let hook_entries = hooks_object()?;
    let added_entries = count_missing_hook_entries(&root, &hook_entries);

    if added_entries == 0 {
        return Ok(PatchResult::AlreadyPresent);
    }

    match mode {
        PatchMode::Skip => {
            print_manual_hook_instructions(settings_path, target)?;
            return Ok(PatchResult::Skipped);
        }
        PatchMode::Ask => {
            if !prompt_user_consent(settings_path, target)? {
                print_manual_hook_instructions(settings_path, target)?;
                return Ok(PatchResult::Declined);
            }
        }
        PatchMode::Auto => {}
    }

    merge_hook_entries(&mut root, &hook_entries)?;
    backup_settings_if_needed(settings_path)?;
    write_json(settings_path, &root)?;

    Ok(PatchResult::Patched { added_entries })
}

fn remove_humanize_hooks(settings_path: &Path) -> Result<usize> {
    if !settings_path.exists() {
        return Ok(0);
    }

    let mut root = read_settings_json(settings_path)?;
    let hook_entries = hooks_object()?;
    let removed = remove_hook_entries(&mut root, &hook_entries)?;

    if removed > 0 {
        backup_settings_if_needed(settings_path)?;
        write_json(settings_path, &root)?;
    }

    Ok(removed)
}

fn read_settings_json(settings_path: &Path) -> Result<JsonValue> {
    if !settings_path.exists() {
        return Ok(json!({}));
    }

    let content = fs::read_to_string(settings_path)
        .with_context(|| format!("Failed to read {}", settings_path.display()))?;
    if content.trim().is_empty() {
        return Ok(json!({}));
    }

    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {} as JSON", settings_path.display()))
}

fn backup_settings_if_needed(settings_path: &Path) -> Result<()> {
    if !settings_path.exists() {
        return Ok(());
    }

    let backup_path = settings_path.with_file_name(SETTINGS_BACKUP_FILE);
    fs::copy(settings_path, &backup_path)
        .with_context(|| format!("Failed to backup {}", settings_path.display()))?;
    Ok(())
}

fn write_json(path: &Path, value: &JsonValue) -> Result<()> {
    let serialized = serde_json::to_string_pretty(value).context("Failed to serialize JSON")?;
    atomic_write(path, serialized.as_bytes())
}

fn merge_hook_entries(
    root: &mut JsonValue,
    hook_entries: &JsonMap<String, JsonValue>,
) -> Result<()> {
    let root_obj = ensure_object(root);
    let hooks_obj = ensure_object(root_obj.entry("hooks").or_insert_with(|| json!({})));

    for (event, value) in hook_entries {
        let entries = value
            .as_array()
            .with_context(|| format!("Hook event {event} must be an array"))?;
        let dest_array = ensure_array(hooks_obj.entry(event.clone()).or_insert_with(|| json!([])))?;

        for entry in entries {
            if !dest_array.iter().any(|existing| existing == entry) {
                dest_array.push(entry.clone());
            }
        }
    }

    Ok(())
}

fn remove_hook_entries(
    root: &mut JsonValue,
    hook_entries: &JsonMap<String, JsonValue>,
) -> Result<usize> {
    let mut removed = 0;
    let Some(hooks_value) = root.get_mut("hooks") else {
        return Ok(0);
    };
    let Some(hooks_obj) = hooks_value.as_object_mut() else {
        return Ok(0);
    };

    for (event, value) in hook_entries {
        let Some(dest_value) = hooks_obj.get_mut(event) else {
            continue;
        };
        let Some(dest_array) = dest_value.as_array_mut() else {
            continue;
        };
        let expected_entries = value
            .as_array()
            .with_context(|| format!("Hook event {event} must be an array"))?;

        let before = dest_array.len();
        dest_array.retain(|existing| {
            !expected_entries
                .iter()
                .any(|candidate| candidate == existing)
        });
        removed += before - dest_array.len();
    }

    hooks_obj.retain(|_, value| {
        value
            .as_array()
            .map(|array| !array.is_empty())
            .unwrap_or(true)
    });
    if hooks_obj.is_empty() {
        root.as_object_mut()
            .expect("root is object")
            .remove("hooks");
    }

    Ok(removed)
}

fn count_missing_hook_entries(
    root: &JsonValue,
    hook_entries: &JsonMap<String, JsonValue>,
) -> usize {
    let Some(existing_hooks) = root.get("hooks").and_then(JsonValue::as_object) else {
        return expected_hook_entry_count();
    };

    let mut missing = 0;
    for (event, value) in hook_entries {
        let expected_entries = match value.as_array() {
            Some(entries) => entries,
            None => continue,
        };
        let existing_entries = existing_hooks
            .get(event)
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();

        for entry in expected_entries {
            if !existing_entries.iter().any(|existing| existing == entry) {
                missing += 1;
            }
        }
    }

    missing
}

fn installed_hook_count(settings_path: &Path) -> Result<usize> {
    let root = read_settings_json(settings_path)?;
    Ok(expected_hook_entry_count() - count_missing_hook_entries(&root, &hooks_object()?))
}

fn hooks_object() -> Result<JsonMap<String, JsonValue>> {
    let hooks_text = asset_utf8("hooks/hooks.json")?;
    let root: JsonValue =
        serde_json::from_str(hooks_text).context("Failed to parse embedded hooks.json")?;
    root.get("hooks")
        .and_then(JsonValue::as_object)
        .cloned()
        .context("Embedded hooks.json is missing `hooks`")
}

fn expected_hook_entry_count() -> usize {
    hooks_object()
        .ok()
        .map(|map| {
            map.values()
                .filter_map(JsonValue::as_array)
                .map(Vec::len)
                .sum::<usize>()
        })
        .unwrap_or(0)
}

fn print_manual_hook_instructions(settings_path: &Path, target: InitTarget) -> Result<()> {
    let snippet = serde_json::to_string_pretty(&json!({ "hooks": hooks_object()? }))
        .context("Failed to render manual hook instructions")?;
    println!(
        "\n  MANUAL STEP: merge this into {}:\n",
        settings_path.display()
    );
    println!("{snippet}");
    println!(
        "\n  Restart {}. Test with: git status",
        host_display_name(target)
    );
    Ok(())
}

fn prompt_user_consent(settings_path: &Path, target: InitTarget) -> Result<bool> {
    eprintln!(
        "\nPatch {} hooks in {}? [y/N] ",
        host_display_name(target),
        settings_path.display()
    );

    if !io::stdin().is_terminal() {
        eprintln!("(non-interactive mode, defaulting to N)");
        return Ok(false);
    }

    let stdin = io::stdin();
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .context("Failed to read user input")?;

    let response = line.trim().to_ascii_lowercase();
    Ok(matches!(response.as_str(), "y" | "yes"))
}

fn write_text_if_changed(path: &Path, content: &str) -> Result<bool> {
    if path.exists() {
        let existing = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if existing == content {
            return Ok(false);
        }
    }

    atomic_write(path, content.as_bytes())?;
    Ok(true)
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("Path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;

    let mut temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("Failed to create temp file in {}", parent.display()))?;
    temp.write_all(content)
        .with_context(|| format!("Failed to write temp file for {}", path.display()))?;
    temp.persist(path)
        .map_err(|err| err.error)
        .with_context(|| format!("Failed to replace {}", path.display()))?;
    Ok(())
}

fn installed_command_count(target: InitTarget, commands_dir: &Path) -> Result<usize> {
    let mut count = 0;
    for (source_path, dest_name) in COMMAND_FILE_MAP {
        let expected = rendered_command_content(target, source_path)?;
        let path = commands_dir.join(dest_name);
        if path.exists() && fs::read_to_string(&path).ok().as_deref() == Some(expected.as_str()) {
            count += 1;
        }
    }
    Ok(count)
}

fn installed_worker_count(target: InitTarget, workers_dir: &Path) -> Result<usize> {
    let mut count = 0;
    for source_path in AGENT_FILES {
        let expected = rendered_worker_content(target, source_path)?;
        let file_name = Path::new(source_path)
            .file_name()
            .context("Worker asset missing file name")?;
        let path = workers_dir.join(file_name);
        if path.exists() && fs::read_to_string(&path).ok().as_deref() == Some(expected.as_str()) {
            count += 1;
        }
    }
    Ok(count)
}

fn installed_skill_count(skills_dir: &Path) -> Result<usize> {
    let mut count = 0;
    for skill_dir in SKILL_DIRS {
        let dir_name = Path::new(skill_dir)
            .file_name()
            .context("Skill asset missing directory name")?;
        let skill_md_path = skills_dir.join(dir_name).join("SKILL.md");
        let expected = asset_utf8(&format!("{skill_dir}/SKILL.md"))?;
        if skill_md_path.exists()
            && fs::read_to_string(&skill_md_path).ok().as_deref() == Some(expected)
        {
            count += 1;
        }
    }
    Ok(count)
}

fn asset_utf8(path: &str) -> Result<&'static str> {
    HOST_ASSETS
        .get_file(path)
        .with_context(|| format!("Missing embedded asset: {path}"))?
        .contents_utf8()
        .with_context(|| format!("Embedded asset is not valid UTF-8: {path}"))
}

fn resolve_host_dir(target: InitTarget) -> Result<PathBuf> {
    let env_key = match target {
        InitTarget::Claude => "HUMANIZE_CLAUDE_DIR",
        InitTarget::Droid => "HUMANIZE_DROID_DIR",
    };

    if let Some(path) = std::env::var_os(env_key) {
        return Ok(PathBuf::from(path));
    }

    dirs::home_dir()
        .map(|home| match target {
            InitTarget::Claude => home.join(".claude"),
            InitTarget::Droid => home.join(".factory"),
        })
        .context("Cannot determine home directory.")
}

fn host_display_name(target: InitTarget) -> &'static str {
    match target {
        InitTarget::Claude => "Claude Code",
        InitTarget::Droid => "Droid",
    }
}

fn worker_dir_name(target: InitTarget) -> &'static str {
    match target {
        InitTarget::Claude => "agents",
        InitTarget::Droid => "droids",
    }
}

fn worker_label(target: InitTarget) -> &'static str {
    match target {
        InitTarget::Claude => "Agents",
        InitTarget::Droid => "Droids",
    }
}

fn worker_label_singular(target: InitTarget) -> &'static str {
    match target {
        InitTarget::Claude => "Agent",
        InitTarget::Droid => "Droid",
    }
}

fn ensure_object(value: &mut JsonValue) -> &mut JsonMap<String, JsonValue> {
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("value must be an object")
}

fn ensure_array(value: &mut JsonValue) -> Result<&mut Vec<JsonValue>> {
    if !value.is_array() {
        *value = json!([]);
    }
    value.as_array_mut().context("value must be an array")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn transform_droid_command_strips_claude_only_fields() {
        let transformed =
            transform_droid_command(asset_utf8("commands/gen-plan.md").unwrap()).unwrap();
        assert!(transformed.contains("description:"));
        assert!(transformed.contains("argument-hint:"));
        assert!(!transformed.contains("hide-from-slash-command-tool"));
        assert!(!transformed.contains("allowed-tools"));
    }

    #[test]
    fn transform_droid_worker_uses_inherit_and_array_tools() {
        let transformed =
            transform_droid_worker(asset_utf8("agents/draft-relevance-checker.md").unwrap())
                .unwrap();
        assert!(transformed.contains("model: inherit"));
        assert!(transformed.contains("- Read"));
        assert!(transformed.contains("- Glob"));
        assert!(transformed.contains("- Grep"));
    }

    #[test]
    fn merge_hooks_is_idempotent() {
        let mut root = json!({});
        let hooks = hooks_object().unwrap();

        merge_hook_entries(&mut root, &hooks).unwrap();
        let first_count = count_missing_hook_entries(&root, &hooks);
        merge_hook_entries(&mut root, &hooks).unwrap();
        let second_count = count_missing_hook_entries(&root, &hooks);

        assert_eq!(first_count, 0);
        assert_eq!(second_count, 0);
    }

    #[test]
    fn remove_hooks_cleans_up_settings() {
        let temp = TempDir::new().unwrap();
        let settings = temp.path().join("settings.json");
        let hooks = hooks_object().unwrap();
        let mut root = json!({});
        merge_hook_entries(&mut root, &hooks).unwrap();
        write_json(&settings, &root).unwrap();

        let removed = remove_humanize_hooks(&settings).unwrap();
        assert_eq!(removed, expected_hook_entry_count());

        let after = read_settings_json(&settings).unwrap();
        assert!(after.get("hooks").is_none());
    }
}
