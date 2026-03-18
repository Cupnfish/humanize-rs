use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use crate::InitTarget;

const DEFAULT_PLUGIN_SOURCE: &str = "https://github.com/Cupnfish/humanize-rs.git";
const PLUGIN_NAME: &str = "humanize-rs";
const LEGACY_CLAUDE_PLUGIN_NAME: &str = "humanize";
const STAMP_FILE: &str = "humanize-plugin-sync.json";

#[derive(Debug, Clone)]
struct MarketplaceRecord {
    name: String,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginSyncStamp {
    target: String,
    cli_version: String,
    plugin_name: String,
    plugin_spec: String,
    marketplace_name: String,
    source: String,
    scope: String,
    installed_at: String,
}

#[derive(Debug, Clone)]
struct PluginInstallStatus {
    installed: bool,
    version: Option<String>,
    details: String,
    scope: Option<String>,
    legacy: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudePluginRecord {
    id: String,
    version: String,
    scope: String,
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
}

pub fn run(target: InitTarget, global: bool, show: bool, uninstall: bool) -> Result<()> {
    if !global {
        bail!("Only `humanize init --global` is supported right now.");
    }

    if show && uninstall {
        bail!("`--show` and `--uninstall` cannot be used together.");
    }

    if show {
        return show_config(target);
    }

    if uninstall {
        return uninstall_global(target);
    }

    install_global(target)
}

pub fn warn_if_plugin_version_mismatch() {
    for target in [InitTarget::Claude, InitTarget::Droid] {
        if let Err(err) = maybe_warn_target_mismatch(target) {
            eprintln!(
                "Warning: failed to check Humanize {} sync: {}",
                host_display_name(target),
                err
            );
        }
    }
}

pub fn run_doctor(target: Option<InitTarget>) -> Result<()> {
    let targets = target
        .map(|one| vec![one])
        .unwrap_or_else(|| vec![InitTarget::Claude, InitTarget::Droid]);

    println!("Humanize Doctor\n");
    println!("CLI Version: {}", current_cli_version());
    println!();

    for (index, host) in targets.iter().enumerate() {
        if index > 0 {
            println!();
        }
        print_target_doctor(*host)?;
    }

    Ok(())
}

fn maybe_warn_target_mismatch(target: InitTarget) -> Result<()> {
    let current = current_cli_version().to_string();
    let stamp = read_stamp(target)?;
    let install_status = plugin_install_status(target)?;

    if install_status.scope.as_deref() != Some("user") && install_status.installed {
        eprintln!(
            "Warning: Humanize {} is currently resolved from {} scope ({}). `humanize init --global{}` only syncs user scope.",
            host_display_name(target),
            install_status.scope.as_deref().unwrap_or("unknown"),
            install_status.details,
            target_init_suffix(target)
        );
    }

    if let Some(stamp) = stamp.as_ref()
        && stamp.cli_version != current
    {
        eprintln!(
            "Warning: Humanize {} plugin was synced with CLI {} but current CLI is {}. Run `humanize init --global{}`.",
            host_display_name(target),
            stamp.cli_version,
            current,
            target_init_suffix(target)
        );
        return Ok(());
    }

    if install_status.legacy {
        eprintln!(
            "Warning: Humanize {} is using legacy plugin {}. Run `humanize init --global{}` and remove old local/project installs if needed.",
            host_display_name(target),
            install_status.details,
            target_init_suffix(target)
        );
        return Ok(());
    }

    if let Some(version) = install_status.version.as_deref()
        && version != current
    {
        eprintln!(
            "Warning: Humanize {} plugin version is {} but current CLI is {}. Run `humanize init --global{}`.",
            host_display_name(target),
            version,
            current,
            target_init_suffix(target)
        );
    }
    Ok(())
}

fn install_global(target: InitTarget) -> Result<()> {
    let source = detect_plugin_source(target)?;
    let marketplace_name = ensure_marketplace(target, &source)?;
    let plugin_spec = format!("{PLUGIN_NAME}@{marketplace_name}");

    let current_version = current_cli_version();
    if matches!(target, InitTarget::Claude) {
        maybe_remove_legacy_claude_user_plugin()?;
    }
    let existing = plugin_install_status(target)?;
    let previous_stamp = read_stamp(target)?;
    let action = if existing.installed
        && existing.version.as_deref() == Some(current_version)
        && previous_stamp
            .as_ref()
            .map(|stamp| stamp.cli_version == current_version && stamp.plugin_spec == plugin_spec)
            .unwrap_or(false)
    {
        "already in sync"
    } else if existing.installed && existing.version.as_deref() == Some(current_version) {
        "already installed"
    } else {
        ensure_plugin_installed(target, &plugin_spec)?
    };

    let stamp = PluginSyncStamp {
        target: target_name(target).to_string(),
        cli_version: current_version.to_string(),
        plugin_name: PLUGIN_NAME.to_string(),
        plugin_spec: plugin_spec.clone(),
        marketplace_name: marketplace_name.clone(),
        source: source.clone(),
        scope: "user".to_string(),
        installed_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    write_stamp(target, &stamp)?;

    println!(
        "\nHumanize plugin {} for {}.\n",
        action,
        host_display_name(target)
    );
    println!("  Source:      {}", source);
    println!("  Marketplace: {}", marketplace_name);
    println!("  Plugin:      {}", plugin_spec);
    println!("  CLI Version: {}", current_version);
    println!("  Scope:       user");
    if existing.scope.as_deref() != Some("user") && existing.installed {
        println!(
            "  Note:        A local/project Humanize plugin override may still exist: {}",
            existing.details
        );
    }
    println!(
        "\n  Restart {}. If you upgrade the CLI later, rerun `humanize init --global{}`.",
        host_display_name(target),
        target_init_suffix(target)
    );

    Ok(())
}

fn show_config(target: InitTarget) -> Result<()> {
    println!("Humanize {} plugin sync:\n", host_display_name(target));

    let source = detect_plugin_source(target)?;
    println!("  Preferred Source: {}", source);
    println!("  CLI Version:      {}", current_cli_version());

    if let Some(stamp) = read_stamp(target)? {
        println!("  Synced Via Init:  yes");
        println!("  Stamp Version:    {}", stamp.cli_version);
        println!("  Marketplace:      {}", stamp.marketplace_name);
        println!("  Plugin:           {}", stamp.plugin_spec);
        println!("  Synced At:        {}", stamp.installed_at);
    } else {
        println!("  Synced Via Init:  no");
    }

    if matches!(target, InitTarget::Claude) {
        if let Some(actual) = claude_installed_plugin_version(PLUGIN_NAME)? {
            println!("  Installed Plugin: {}", actual);
        } else {
            println!("  Installed Plugin: not detected in user scope");
        }
    }

    println!("\nUsage:");
    println!("  humanize init --global");
    println!("  humanize init --global --target droid");
    println!("  humanize init --global --show");
    println!("  humanize init --global --target droid --show");
    println!("  humanize init --global --uninstall");
    println!("  humanize init --global --target droid --uninstall");

    Ok(())
}

fn print_target_doctor(target: InitTarget) -> Result<()> {
    println!("{}:", host_display_name(target));

    let source = detect_plugin_source(target)?;
    println!("  Preferred Source: {}", source);

    let marketplaces = list_marketplaces(target).unwrap_or_default();
    let matching_marketplace = marketplaces
        .iter()
        .find(|record| source_matches(&record.source, &source));
    match matching_marketplace {
        Some(record) => println!("  Marketplace:      {} ({})", record.name, record.source),
        None => println!("  Marketplace:      missing for source {}", source),
    }

    let install_status = plugin_install_status(target)?;
    if install_status.installed {
        println!(
            "  Plugin Installed: yes{}",
            install_status
                .version
                .as_ref()
                .map(|v| format!(" ({v})"))
                .unwrap_or_default()
        );
    } else {
        println!("  Plugin Installed: no");
    }
    if !install_status.details.is_empty() {
        println!("  Plugin Detail:    {}", install_status.details);
    }
    if let Some(scope) = install_status.scope.as_deref() {
        println!("  Active Scope:     {}", scope);
    }
    if install_status.legacy {
        println!("  Legacy Plugin:    yes");
    }

    let stamp = read_stamp(target)?;
    match stamp {
        Some(ref stamp) => {
            println!("  Sync Stamp:       {}", stamp.cli_version);
            println!("  Synced At:        {}", stamp.installed_at);
            println!(
                "  Version Match:    {}",
                if stamp.cli_version == current_cli_version() {
                    "yes"
                } else {
                    "no"
                }
            );
        }
        None => {
            println!("  Sync Stamp:       missing");
            println!("  Version Match:    unknown");
        }
    }

    let recommendation = if matching_marketplace.is_none() || !install_status.installed {
        format!(
            "Run `humanize init --global{}`.",
            target_init_suffix(target)
        )
    } else if stamp
        .as_ref()
        .map(|s| s.cli_version.as_str() != current_cli_version())
        .unwrap_or(true)
    {
        format!(
            "Plugin sync is stale. Run `humanize init --global{}`.",
            target_init_suffix(target)
        )
    } else {
        "No action needed.".to_string()
    };
    println!("  Recommendation:   {}", recommendation);

    Ok(())
}

fn uninstall_global(target: InitTarget) -> Result<()> {
    let stamp = read_stamp(target)?;
    let plugin_spec = stamp
        .as_ref()
        .map(|s| s.plugin_spec.as_str())
        .unwrap_or(PLUGIN_NAME);

    let uninstall = run_host_command(target, &["plugin", "uninstall", "-s", "user", plugin_spec])?;
    if !uninstall.status.success() {
        let fallback =
            run_host_command(target, &["plugin", "uninstall", "-s", "user", PLUGIN_NAME])?;
        if !fallback.status.success() {
            bail!(
                "Failed to uninstall Humanize plugin from {}.\n{}\n{}",
                host_display_name(target),
                render_output(&uninstall),
                render_output(&fallback),
            );
        }
    }

    let stamp_path = stamp_path(target)?;
    if stamp_path.exists() {
        fs::remove_file(&stamp_path)
            .with_context(|| format!("Failed to remove {}", stamp_path.display()))?;
    }

    println!(
        "Humanize plugin uninstalled from {}.\n\n  Restart {} to apply changes.",
        host_display_name(target),
        host_display_name(target)
    );
    Ok(())
}

fn ensure_plugin_installed(target: InitTarget, plugin_spec: &str) -> Result<&'static str> {
    let update = run_host_command(target, &["plugin", "update", "-s", "user", plugin_spec])?;
    if update.status.success() {
        return Ok("updated");
    }

    let install = run_host_command(target, &["plugin", "install", "-s", "user", plugin_spec])?;
    if install.status.success() {
        return Ok("installed");
    }

    bail!(
        "Failed to install Humanize plugin for {}.\n{}\n{}",
        host_display_name(target),
        render_output(&update),
        render_output(&install),
    );
}

fn ensure_marketplace(target: InitTarget, source: &str) -> Result<String> {
    let add = run_host_command(target, &["plugin", "marketplace", "add", source])?;
    if add.status.success()
        && let Some(name) = parse_marketplace_name_from_add_output(target, &add)
    {
        return Ok(name);
    }
    let marketplaces = list_marketplaces(target)?;
    if let Some(found) = marketplaces
        .iter()
        .find(|record| source_matches(&record.source, source))
    {
        return Ok(found.name.clone());
    }

    bail!(
        "Failed to configure marketplace for {}.\n{}",
        host_display_name(target),
        render_output(&add),
    )
}

fn parse_marketplace_name_from_add_output(target: InitTarget, output: &Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    match target {
        InitTarget::Claude => {
            let needle = "Marketplace '";
            let start = combined.find(needle)? + needle.len();
            let rest = &combined[start..];
            let end = rest.find('\'')?;
            let name = rest[..end].trim();
            (!name.is_empty()).then(|| name.to_string())
        }
        InitTarget::Droid => {
            let needle = "Successfully added marketplace:";
            let start = combined.find(needle)? + needle.len();
            let name = combined[start..].lines().next()?.trim();
            (!name.is_empty()).then(|| name.to_string())
        }
    }
}

fn list_marketplaces(target: InitTarget) -> Result<Vec<MarketplaceRecord>> {
    let output = run_host_command(target, &["plugin", "marketplace", "list"])?;
    if !output.status.success() {
        bail!(
            "Failed to list {} marketplaces.\n{}",
            host_display_name(target),
            render_output(&output),
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(match target {
        InitTarget::Claude => parse_claude_marketplaces(&stdout),
        InitTarget::Droid => parse_droid_marketplaces(&stdout),
    })
}

fn parse_claude_marketplaces(stdout: &str) -> Vec<MarketplaceRecord> {
    let mut current_name: Option<String> = None;
    let mut records = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix('❯').map(str::trim) {
            if !name.is_empty() {
                current_name = Some(name.to_string());
            }
            continue;
        }
        if let Some(source) = trimmed.strip_prefix("Source: ") {
            if let Some(name) = current_name.take() {
                records.push(MarketplaceRecord {
                    name,
                    source: source.trim().to_string(),
                });
            }
        }
    }

    records
}

fn parse_droid_marketplaces(stdout: &str) -> Vec<MarketplaceRecord> {
    let mut records = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("Registered marketplaces:")
            || trimmed.starts_with("No marketplaces")
        {
            continue;
        }

        let parts = trimmed
            .split("  ")
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.len() >= 2 {
            records.push(MarketplaceRecord {
                name: parts[0].to_string(),
                source: parts.last().unwrap_or(&"").to_string(),
            });
        }
    }

    records
}

fn source_matches(listed_source: &str, desired_source: &str) -> bool {
    if let Ok(canonical) = fs::canonicalize(desired_source) {
        let canonical = canonical.display().to_string();
        if listed_source.contains(&canonical) {
            return true;
        }
    }

    if let Some(repo_slug) = github_repo_slug(desired_source) {
        let listed = listed_source.to_ascii_lowercase();
        if listed.contains(&repo_slug.to_ascii_lowercase()) {
            return true;
        }
    }

    listed_source.trim() == desired_source.trim()
}

fn github_repo_slug(source: &str) -> Option<String> {
    let trimmed = source.trim().trim_end_matches(".git").trim_end_matches('/');

    if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("http://github.com/") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        return Some(rest.to_string());
    }
    if let Some(start) = trimmed.find("GitHub (") {
        let rest = &trimmed[start + "GitHub (".len()..];
        if let Some(end) = rest.find(')') {
            return Some(rest[..end].to_string());
        }
    }

    None
}

fn extract_version_token(line: &str) -> Option<String> {
    for token in line.split_whitespace() {
        if token.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
            && token.chars().any(|ch| ch == '.')
        {
            return Some(token.to_string());
        }
    }
    None
}

fn claude_installed_plugin_version(plugin_name: &str) -> Result<Option<String>> {
    let output = run_host_command(InitTarget::Claude, &["plugin", "list", "--json"])?;
    if !output.status.success() {
        return Ok(None);
    }

    let plugins: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("Failed to parse `claude plugin list --json` output")?;
    let Some(items) = plugins.as_array() else {
        return Ok(None);
    };

    for item in items {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(scope) = item.get("scope").and_then(|v| v.as_str()) else {
            continue;
        };
        if scope == "user" && id.starts_with(&format!("{plugin_name}@")) {
            return Ok(item
                .get("version")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned));
        }
    }

    Ok(None)
}

fn plugin_install_status(target: InitTarget) -> Result<PluginInstallStatus> {
    match target {
        InitTarget::Claude => claude_plugin_install_status(),
        InitTarget::Droid => droid_plugin_install_status(),
    }
}

fn claude_plugin_install_status() -> Result<PluginInstallStatus> {
    let plugins = claude_plugins()?;
    if plugins.is_empty() {
        return Ok(PluginInstallStatus {
            installed: false,
            version: None,
            details: "not found in user scope".to_string(),
            scope: None,
            legacy: false,
        });
    }

    if let Some(active) = active_claude_plugin(&plugins) {
        return Ok(PluginInstallStatus {
            installed: active.id.starts_with(&format!("{PLUGIN_NAME}@")),
            version: Some(active.version.clone()),
            details: active
                .project_path
                .as_ref()
                .map(|path| format!("{} [{}] {}", active.id, active.scope, path))
                .unwrap_or_else(|| format!("{} [{}]", active.id, active.scope)),
            scope: Some(active.scope.clone()),
            legacy: active
                .id
                .starts_with(&format!("{LEGACY_CLAUDE_PLUGIN_NAME}@")),
        });
    }

    Ok(PluginInstallStatus {
        installed: false,
        version: None,
        details: "not found in user scope".to_string(),
        scope: None,
        legacy: false,
    })
}

fn droid_plugin_install_status() -> Result<PluginInstallStatus> {
    let output = run_host_command(InitTarget::Droid, &["plugin", "list", "-s", "user"])?;
    if !output.status.success() {
        return Ok(PluginInstallStatus {
            installed: false,
            version: None,
            details: "failed to read plugin list".to_string(),
            scope: None,
            legacy: false,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("No plugins") {
            continue;
        }
        if trimmed.contains(PLUGIN_NAME) {
            return Ok(PluginInstallStatus {
                installed: true,
                version: extract_version_token(trimmed),
                details: trimmed.to_string(),
                scope: Some("user".to_string()),
                legacy: false,
            });
        }
    }

    Ok(PluginInstallStatus {
        installed: false,
        version: None,
        details: "not found in user scope".to_string(),
        scope: None,
        legacy: false,
    })
}

fn claude_plugins() -> Result<Vec<ClaudePluginRecord>> {
    let output = run_host_command(InitTarget::Claude, &["plugin", "list", "--json"])?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let plugins: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("Failed to parse `claude plugin list --json` output")?;
    let Some(items) = plugins.as_array() else {
        return Ok(Vec::new());
    };
    let mut records = Vec::new();
    for item in items {
        if let Ok(record) = serde_json::from_value::<ClaudePluginRecord>(item.clone()) {
            records.push(record);
        }
    }
    Ok(records)
}

fn active_claude_plugin<'a>(plugins: &'a [ClaudePluginRecord]) -> Option<&'a ClaudePluginRecord> {
    let cwd = std::env::current_dir().ok();

    let mut scoped = plugins
        .iter()
        .filter(|plugin| {
            plugin.id.starts_with(&format!("{PLUGIN_NAME}@"))
                || plugin
                    .id
                    .starts_with(&format!("{LEGACY_CLAUDE_PLUGIN_NAME}@"))
        })
        .collect::<Vec<_>>();

    scoped.sort_by_key(|plugin| {
        plugin
            .project_path
            .as_ref()
            .map(|path| path.len())
            .unwrap_or_default()
    });
    scoped.reverse();

    if let Some(cwd) = cwd {
        for plugin in &scoped {
            if matches!(plugin.scope.as_str(), "local" | "project")
                && let Some(project_path) = plugin.project_path.as_ref()
                && cwd.starts_with(project_path)
            {
                return Some(plugin);
            }
        }
    }

    scoped.into_iter().find(|plugin| plugin.scope == "user")
}

fn maybe_remove_legacy_claude_user_plugin() -> Result<()> {
    for plugin in claude_plugins()? {
        if plugin.scope == "user"
            && plugin
                .id
                .starts_with(&format!("{LEGACY_CLAUDE_PLUGIN_NAME}@"))
        {
            let _ = run_host_command(
                InitTarget::Claude,
                &["plugin", "uninstall", "-s", "user", &plugin.id],
            )?;
        }
    }
    Ok(())
}

fn detect_plugin_source(target: InitTarget) -> Result<String> {
    if let Some(source) = std::env::var_os("HUMANIZE_PLUGIN_SOURCE") {
        return Ok(PathBuf::from(source).display().to_string());
    }

    if let Some(root) = find_local_plugin_repo() {
        if let (Ok(canonical_root), Ok(host_dir)) =
            (fs::canonicalize(&root), resolve_host_dir(target))
        {
            if host_dir.starts_with(&canonical_root) {
                return Ok(DEFAULT_PLUGIN_SOURCE.to_string());
            }
        }
        return Ok(root.display().to_string());
    }

    Ok(DEFAULT_PLUGIN_SOURCE.to_string())
}

fn find_local_plugin_repo() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    for dir in cwd.ancestors() {
        if dir
            .join(".claude-plugin")
            .join("marketplace.json")
            .is_file()
            && dir.join(".claude-plugin").join("plugin.json").is_file()
        {
            return Some(dir.to_path_buf());
        }
    }
    None
}

fn run_host_command(target: InitTarget, args: &[&str]) -> Result<Output> {
    Command::new(host_binary(target))
        .args(args)
        .output()
        .with_context(|| format!("Failed to run `{}`", shell_preview(target, args)))
}

fn render_output(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    format!(
        "exit={}\nstdout:\n{}\nstderr:\n{}",
        output.status.code().unwrap_or(-1),
        if stdout.is_empty() {
            "(empty)"
        } else {
            &stdout
        },
        if stderr.is_empty() {
            "(empty)"
        } else {
            &stderr
        },
    )
}

fn shell_preview(target: InitTarget, args: &[&str]) -> String {
    let mut parts = vec![host_binary(target).to_string()];
    parts.extend(args.iter().map(|arg| (*arg).to_string()));
    parts.join(" ")
}

fn current_cli_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
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

fn stamp_path(target: InitTarget) -> Result<PathBuf> {
    Ok(resolve_host_dir(target)?.join(STAMP_FILE))
}

fn read_stamp(target: InitTarget) -> Result<Option<PluginSyncStamp>> {
    let path = stamp_path(target)?;
    if !path.exists() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let stamp = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(stamp))
}

fn write_stamp(target: InitTarget, stamp: &PluginSyncStamp) -> Result<()> {
    let path = stamp_path(target)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content =
        serde_json::to_string_pretty(stamp).context("Failed to serialize plugin sync stamp")?;
    fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))
}

fn host_binary(target: InitTarget) -> &'static str {
    match target {
        InitTarget::Claude => "claude",
        InitTarget::Droid => "droid",
    }
}

fn host_display_name(target: InitTarget) -> &'static str {
    match target {
        InitTarget::Claude => "Claude Code",
        InitTarget::Droid => "Droid",
    }
}

fn target_name(target: InitTarget) -> &'static str {
    match target {
        InitTarget::Claude => "claude",
        InitTarget::Droid => "droid",
    }
}

fn target_init_suffix(target: InitTarget) -> &'static str {
    match target {
        InitTarget::Claude => "",
        InitTarget::Droid => " --target droid",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_claude_marketplaces_extracts_name_and_source() {
        let output = r#"
Configured marketplaces:

  ❯ humania-rs
    Source: Directory (/tmp/humanize)

  ❯ remote
    Source: GitHub (Cupnfish/humanize-rs)
"#;
        let marketplaces = parse_claude_marketplaces(output);
        assert_eq!(marketplaces.len(), 2);
        assert_eq!(marketplaces[0].name, "humania-rs");
        assert!(marketplaces[0].source.contains("/tmp/humanize"));
        assert_eq!(marketplaces[1].name, "remote");
    }

    #[test]
    fn parse_droid_marketplaces_extracts_name_and_source() {
        let output = r#"
Registered marketplaces:
  humanize  (1 plugins)  local:/tmp/humanize
  remote  (4 plugins)  git:https://github.com/Cupnfish/humanize-rs.git
"#;
        let marketplaces = parse_droid_marketplaces(output);
        assert_eq!(marketplaces.len(), 2);
        assert_eq!(marketplaces[0].name, "humanize");
        assert_eq!(marketplaces[0].source, "local:/tmp/humanize");
        assert_eq!(marketplaces[1].name, "remote");
    }

    #[test]
    fn source_matches_github_slug() {
        assert!(source_matches(
            "GitHub (Cupnfish/humanize-rs)",
            "https://github.com/Cupnfish/humanize-rs.git"
        ));
    }

    #[test]
    fn parse_claude_marketplace_name_from_add_output() {
        let output = Output {
            status: exit_status(0),
            stdout: b"Adding marketplace...\n\xe2\x88\x9a Marketplace 'humania-rs' already on disk \xe2\x80\x94 declared in user settings\n".to_vec(),
            stderr: Vec::new(),
        };
        assert_eq!(
            parse_marketplace_name_from_add_output(InitTarget::Claude, &output).as_deref(),
            Some("humania-rs")
        );
    }

    #[test]
    fn parse_droid_marketplace_name_from_add_output() {
        let output = Output {
            status: exit_status(0),
            stdout: b"Successfully added marketplace: humanize\n".to_vec(),
            stderr: Vec::new(),
        };
        assert_eq!(
            parse_marketplace_name_from_add_output(InitTarget::Droid, &output).as_deref(),
            Some("humanize")
        );
    }

    #[cfg(unix)]
    fn exit_status(code: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code << 8)
    }
}
