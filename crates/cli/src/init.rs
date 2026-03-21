use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::{InitTarget, commands::resolve_project_root};

const DEFAULT_PLUGIN_SOURCE: &str = "https://github.com/Cupnfish/humanize-rs.git";
const MARKETPLACE_NAME: &str = "humania-rs";
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum InstallScope {
    User,
    Project,
}

#[derive(Debug, Clone)]
struct PluginInstallStatus {
    installed: bool,
    version: Option<String>,
    details: String,
    scope: Option<String>,
    plugin_id: Option<String>,
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

#[derive(Debug, Clone)]
struct HostPluginRecord {
    id: String,
    version: Option<String>,
    scope: String,
    details: String,
    legacy: bool,
}

impl InstallScope {
    fn from_global(global: bool) -> Self {
        if global { Self::User } else { Self::Project }
    }

    fn flag(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }

    fn command_flag(self) -> &'static str {
        match self {
            Self::User => " --global",
            Self::Project => "",
        }
    }
}

pub fn run(target: InitTarget, global: bool, show: bool, uninstall: bool) -> Result<()> {
    if show && uninstall {
        bail!("`--show` and `--uninstall` cannot be used together.");
    }

    let scope = InstallScope::from_global(global);
    let project_root = project_root_for_scope(scope)?;

    if show {
        return show_config(target, scope, project_root.as_deref());
    }

    if uninstall {
        return uninstall_plugin(target, scope, project_root.as_deref());
    }

    install_plugin(target, scope, project_root.as_deref())
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

    print_windows_codex_doctor();

    Ok(())
}

fn maybe_warn_target_mismatch(target: InitTarget) -> Result<()> {
    let current = current_cli_version().to_string();
    let project_root = resolve_project_root().ok();
    let install_status = plugin_install_status(target, project_root.as_deref())?;
    let stamp = install_status
        .scope
        .as_deref()
        .and_then(scope_from_host_scope)
        .map(|scope| read_stamp(target, scope, project_root.as_deref()))
        .transpose()?
        .flatten();
    let repair_scope = install_status
        .scope
        .as_deref()
        .and_then(scope_from_host_scope)
        .unwrap_or(InstallScope::Project);

    if let Some(stamp) = stamp.as_ref()
        && stamp.cli_version != current
    {
        eprintln!(
            "Warning: Humanize {} plugin was synced with CLI {} but current CLI is {}. Run `{}`.",
            host_display_name(target),
            stamp.cli_version,
            current,
            init_command(target, repair_scope)
        );
        return Ok(());
    }

    if install_status.legacy {
        eprintln!(
            "Warning: Humanize {} is using legacy plugin {}. Run `{}`.",
            host_display_name(target),
            install_status.details,
            init_command(target, repair_scope)
        );
        return Ok(());
    }

    if let Some(version) = install_status.version.as_deref()
        && version != current
    {
        eprintln!(
            "Warning: Humanize {} plugin version is {} but current CLI is {}. Run `{}`.",
            host_display_name(target),
            version,
            current,
            init_command(target, repair_scope)
        );
    }
    Ok(())
}

fn install_plugin(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
) -> Result<()> {
    let source = detect_plugin_source(target)?;
    migrate_local_marketplace_install(target, scope, project_root, &source)?;
    let marketplace_name = ensure_marketplace(target, &source)?;
    let plugin_spec = format!("{PLUGIN_NAME}@{marketplace_name}");

    let current_version = current_cli_version();
    let previous_stamp = read_stamp(target, scope, project_root)?;
    let existing = plugin_install_status_for_scope(target, scope, project_root)?;
    let active_project_root = project_root
        .map(Path::to_path_buf)
        .or_else(|| resolve_project_root().ok());
    let resolved = plugin_install_status(target, active_project_root.as_deref())?;
    let needs_migration =
        stamp_needs_migration(previous_stamp.as_ref(), scope, &source, &plugin_spec)
            || existing.legacy
            || (existing.installed && previous_stamp.is_none())
            || existing
                .plugin_id
                .as_deref()
                .map(|id| id != plugin_spec)
                .unwrap_or(false);
    if needs_migration {
        clear_scope_install(
            target,
            scope,
            project_root,
            previous_stamp.as_ref(),
            &existing,
        )?;
    }

    remove_legacy_plugin(target, scope, project_root)?;
    let action = ensure_plugin_installed(target, scope, project_root, &plugin_spec)?;
    let existing = plugin_install_status_for_scope(target, scope, project_root)?;

    let stamp = PluginSyncStamp {
        target: target_name(target).to_string(),
        cli_version: current_version.to_string(),
        plugin_name: PLUGIN_NAME.to_string(),
        plugin_spec: plugin_spec.clone(),
        marketplace_name: marketplace_name.clone(),
        source: source.clone(),
        scope: scope.flag().to_string(),
        installed_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    if matches!(target, InitTarget::Claude) {
        let installed_version = existing.version.as_deref().unwrap_or(current_version);
        remove_claude_compat_commands(&marketplace_name, installed_version)?;
    }
    write_stamp(target, scope, project_root, &stamp)?;

    println!(
        "\nHumanize plugin {} for {}.\n",
        action,
        host_display_name(target)
    );
    println!("  Source:      {}", source);
    println!("  Marketplace: {}", marketplace_name);
    println!("  Plugin:      {}", plugin_spec);
    println!("  CLI Version: {}", current_version);
    println!("  Scope:       {}", scope.flag());
    if let Some(project_root) = project_root {
        println!("  Project:     {}", project_root.display());
    }
    if let Some(version) = existing.version.as_deref() {
        println!("  Installed:   {}", version);
    }
    if resolved.scope.as_deref() != Some(scope.flag()) && (resolved.installed || resolved.legacy) {
        println!(
            "  Note:        Another Humanize plugin may still be taking precedence: {}",
            resolved.details
        );
    }
    println!(
        "\n  Restart {}. If you upgrade the CLI later, rerun `{}`.",
        host_display_name(target),
        init_command(target, scope)
    );

    Ok(())
}

fn show_config(target: InitTarget, scope: InstallScope, project_root: Option<&Path>) -> Result<()> {
    println!(
        "Humanize {} plugin sync ({})\n",
        host_display_name(target),
        scope.flag()
    );

    let source = detect_plugin_source(target)?;
    println!("  Preferred Source: {}", source);
    println!("  CLI Version:      {}", current_cli_version());
    println!("  Scope:            {}", scope.flag());
    if let Some(project_root) = project_root {
        println!("  Project Root:     {}", project_root.display());
    }

    if let Some(stamp) = read_stamp(target, scope, project_root)? {
        println!("  Synced Via Init:  yes");
        println!("  Stamp Version:    {}", stamp.cli_version);
        println!("  Marketplace:      {}", stamp.marketplace_name);
        println!("  Plugin:           {}", stamp.plugin_spec);
        println!("  Synced At:        {}", stamp.installed_at);
    } else {
        println!("  Synced Via Init:  no");
    }

    let install_status = plugin_install_status_for_scope(target, scope, project_root)?;
    if install_status.installed || install_status.legacy {
        println!("  Installed Plugin: {}", install_status.details);
    } else {
        println!("  Installed Plugin: not detected in {} scope", scope.flag());
    }
    if install_status.legacy {
        println!("  Legacy Plugin:    yes");
    }

    println!("\nUsage:");
    println!("  humanize init");
    println!("  humanize init --global");
    println!("  humanize init --target droid");
    println!("  humanize init --global --target droid");
    println!("  humanize init --show");
    println!("  humanize init --global --show");
    println!("  humanize init --uninstall");
    println!("  humanize init --global --uninstall");

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

    let project_root = resolve_project_root()?;
    let install_status = plugin_install_status(target, Some(&project_root))?;
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

    let user_status = plugin_install_status_for_scope(target, InstallScope::User, None)?;
    let user_stamp = read_stamp(target, InstallScope::User, None)?;
    let project_status =
        plugin_install_status_for_scope(target, InstallScope::Project, Some(&project_root))?;
    let project_stamp = read_stamp(target, InstallScope::Project, Some(&project_root))?;

    print_scope_doctor(
        target,
        InstallScope::Project,
        Some(&project_root),
        &project_status,
        project_stamp.as_ref(),
    );
    print_scope_doctor(
        target,
        InstallScope::User,
        None,
        &user_status,
        user_stamp.as_ref(),
    );

    Ok(())
}

fn uninstall_plugin(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
) -> Result<()> {
    let stamp = read_stamp(target, scope, project_root)?;
    let existing = plugin_install_status_for_scope(target, scope, project_root)?;
    clear_scope_install(target, scope, project_root, stamp.as_ref(), &existing)?;

    println!(
        "Humanize plugin uninstalled from {} {} scope.\n\n  Restart {} to apply changes.",
        host_display_name(target),
        scope.flag(),
        host_display_name(target)
    );
    Ok(())
}

fn ensure_plugin_installed(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
    plugin_spec: &str,
) -> Result<&'static str> {
    let update = run_scoped_host_command(
        target,
        scope,
        project_root,
        &["plugin", "update", "-s", scope.flag(), plugin_spec],
    )?;
    if update.status.success() {
        return Ok("synced");
    }

    let install = run_scoped_host_command(
        target,
        scope,
        project_root,
        &["plugin", "install", "-s", scope.flag(), plugin_spec],
    )?;
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
        let current = trimmed
            .strip_prefix('❯')
            .or_else(|| trimmed.strip_prefix('>'))
            .map(str::trim);
        if let Some(name) = current {
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

fn stamp_source_matches(stamp: Option<&PluginSyncStamp>, source: &str) -> bool {
    stamp
        .map(|stamp| source_matches(&stamp.source, source))
        .unwrap_or(false)
}

fn stamp_needs_migration(
    stamp: Option<&PluginSyncStamp>,
    scope: InstallScope,
    source: &str,
    plugin_spec: &str,
) -> bool {
    stamp
        .map(|stamp| {
            stamp.scope != scope.flag()
                || !stamp_source_matches(Some(stamp), source)
                || stamp.plugin_spec != plugin_spec
        })
        .unwrap_or(false)
}

fn migrate_local_marketplace_install(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
    desired_source: &str,
) -> Result<()> {
    let marketplaces = list_marketplaces(target).unwrap_or_default();
    let Some(record) = marketplaces
        .iter()
        .find(|record| record.name == MARKETPLACE_NAME)
    else {
        return Ok(());
    };

    if source_matches(&record.source, desired_source)
        || !marketplace_source_is_local(&record.source)
    {
        return Ok(());
    }

    let plugin_spec = format!("{PLUGIN_NAME}@{}", record.name);
    let uninstall = run_scoped_host_command(
        target,
        scope,
        project_root,
        &["plugin", "uninstall", "-s", scope.flag(), &plugin_spec],
    )?;
    if !uninstall.status.success() {
        let _ = run_scoped_host_command(
            target,
            scope,
            project_root,
            &["plugin", "uninstall", "-s", scope.flag(), PLUGIN_NAME],
        )?;
    }

    let remove = run_host_command(target, &["plugin", "marketplace", "remove", &record.name])?;
    if !remove.status.success() {
        bail!(
            "Failed to remove local Humanize marketplace from {}.\n{}",
            host_display_name(target),
            render_output(&remove),
        );
    }

    Ok(())
}

fn marketplace_source_is_local(source: &str) -> bool {
    let trimmed = source.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("local:")
        || lower.starts_with("directory (")
        || fs::canonicalize(trimmed).is_ok()
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

fn plugin_install_status(
    target: InitTarget,
    project_root: Option<&Path>,
) -> Result<PluginInstallStatus> {
    match target {
        InitTarget::Claude => claude_plugin_install_status(project_root),
        InitTarget::Droid => droid_plugin_install_status(project_root),
    }
}

fn plugin_install_status_for_scope(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
) -> Result<PluginInstallStatus> {
    let plugins = list_humanize_plugins_in_scope(target, scope, project_root)?;
    let current = plugins.iter().find(|plugin| !plugin.legacy);
    let legacy = plugins.iter().find(|plugin| plugin.legacy);

    if let Some(plugin) = current {
        return Ok(PluginInstallStatus {
            installed: true,
            version: plugin.version.clone(),
            details: plugin.details.clone(),
            scope: Some(plugin.scope.clone()),
            plugin_id: Some(plugin.id.clone()),
            legacy: legacy.is_some(),
        });
    }

    if let Some(plugin) = legacy {
        return Ok(PluginInstallStatus {
            installed: false,
            version: plugin.version.clone(),
            details: plugin.details.clone(),
            scope: Some(plugin.scope.clone()),
            plugin_id: Some(plugin.id.clone()),
            legacy: true,
        });
    }

    Ok(PluginInstallStatus {
        installed: false,
        version: None,
        details: format!("not found in {} scope", scope.flag()),
        scope: None,
        plugin_id: None,
        legacy: false,
    })
}

fn claude_plugin_install_status(project_root: Option<&Path>) -> Result<PluginInstallStatus> {
    let plugins = claude_plugins()?;
    if plugins.is_empty() {
        return Ok(PluginInstallStatus {
            installed: false,
            version: None,
            details: "not found in user or project scope".to_string(),
            scope: None,
            plugin_id: None,
            legacy: false,
        });
    }

    if let Some(active) = active_claude_plugin(&plugins, project_root) {
        return Ok(PluginInstallStatus {
            installed: active.id.starts_with(&format!("{PLUGIN_NAME}@")),
            version: Some(active.version.clone()),
            details: active
                .project_path
                .as_ref()
                .map(|path| format!("{} [{}] {}", active.id, active.scope, path))
                .unwrap_or_else(|| format!("{} [{}]", active.id, active.scope)),
            scope: Some(active.scope.clone()),
            plugin_id: Some(active.id.clone()),
            legacy: active
                .id
                .starts_with(&format!("{LEGACY_CLAUDE_PLUGIN_NAME}@")),
        });
    }

    Ok(PluginInstallStatus {
        installed: false,
        version: None,
        details: "not found in user or project scope".to_string(),
        scope: None,
        plugin_id: None,
        legacy: false,
    })
}

fn droid_plugin_install_status(project_root: Option<&Path>) -> Result<PluginInstallStatus> {
    if let Some(project_root) = project_root {
        let project = plugin_install_status_for_scope(
            InitTarget::Droid,
            InstallScope::Project,
            Some(project_root),
        )?;
        if project.installed || project.legacy {
            return Ok(project);
        }
    }

    let user = plugin_install_status_for_scope(InitTarget::Droid, InstallScope::User, None)?;
    if user.installed || user.legacy {
        return Ok(user);
    }

    Ok(PluginInstallStatus {
        installed: false,
        version: None,
        details: "not found in user or project scope".to_string(),
        scope: None,
        plugin_id: None,
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

fn active_claude_plugin<'a>(
    plugins: &'a [ClaudePluginRecord],
    project_root: Option<&Path>,
) -> Option<&'a ClaudePluginRecord> {
    let cwd = project_root
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());

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

fn list_humanize_plugins_in_scope(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
) -> Result<Vec<HostPluginRecord>> {
    match target {
        InitTarget::Claude => claude_plugins_in_scope(scope, project_root),
        InitTarget::Droid => droid_plugins_in_scope(scope, project_root),
    }
}

fn claude_plugins_in_scope(
    scope: InstallScope,
    project_root: Option<&Path>,
) -> Result<Vec<HostPluginRecord>> {
    let mut records = Vec::new();
    for plugin in claude_plugins()? {
        let id_matches = plugin.id.starts_with(&format!("{PLUGIN_NAME}@"))
            || plugin
                .id
                .starts_with(&format!("{LEGACY_CLAUDE_PLUGIN_NAME}@"));
        if !id_matches || !scope_matches(&plugin.scope, scope) {
            continue;
        }

        if scope == InstallScope::Project
            && !project_root_matches(project_root, plugin.project_path.as_deref())
        {
            continue;
        }

        let details = plugin
            .project_path
            .as_ref()
            .map(|path| format!("{} [{}] {}", plugin.id, plugin.scope, path))
            .unwrap_or_else(|| format!("{} [{}]", plugin.id, plugin.scope));
        records.push(HostPluginRecord {
            id: plugin.id.clone(),
            version: Some(plugin.version.clone()),
            scope: plugin.scope.clone(),
            details,
            legacy: plugin
                .id
                .starts_with(&format!("{LEGACY_CLAUDE_PLUGIN_NAME}@")),
        });
    }
    Ok(records)
}

fn droid_plugins_in_scope(
    scope: InstallScope,
    project_root: Option<&Path>,
) -> Result<Vec<HostPluginRecord>> {
    let output = run_scoped_host_command(
        InitTarget::Droid,
        scope,
        project_root,
        &["plugin", "list", "-s", scope.flag()],
    )?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut records = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("No plugins")
            || trimmed.starts_with("Installed plugins:")
        {
            continue;
        }

        let id = trimmed.split_whitespace().next().unwrap_or_default();
        if id.is_empty()
            || (!id.starts_with(PLUGIN_NAME) && !id.starts_with(LEGACY_CLAUDE_PLUGIN_NAME))
        {
            continue;
        }

        records.push(HostPluginRecord {
            id: id.to_string(),
            version: extract_version_token(trimmed),
            scope: scope.flag().to_string(),
            details: trimmed.to_string(),
            legacy: id.starts_with(LEGACY_CLAUDE_PLUGIN_NAME),
        });
    }

    Ok(records)
}

fn remove_legacy_plugin(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
) -> Result<()> {
    for plugin in list_humanize_plugins_in_scope(target, scope, project_root)? {
        if !plugin.legacy {
            continue;
        }
        let uninstall = run_scoped_host_command(
            target,
            scope,
            project_root,
            &["plugin", "uninstall", "-s", scope.flag(), &plugin.id],
        )?;
        if uninstall.status.success() {
            continue;
        }

        let fallback = run_scoped_host_command(
            target,
            scope,
            project_root,
            &[
                "plugin",
                "uninstall",
                "-s",
                scope.flag(),
                LEGACY_CLAUDE_PLUGIN_NAME,
            ],
        )?;
        if !fallback.status.success() {
            bail!(
                "Failed to remove legacy Humanize plugin from {} {} scope.\n{}\n{}",
                host_display_name(target),
                scope.flag(),
                render_output(&uninstall),
                render_output(&fallback),
            );
        }
    }

    Ok(())
}

fn clear_scope_install(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
    stamp: Option<&PluginSyncStamp>,
    existing: &PluginInstallStatus,
) -> Result<()> {
    let installed_plugins = list_humanize_plugins_in_scope(target, scope, project_root)?;
    let mut candidates = Vec::new();

    if let Some(stamp) = stamp {
        push_candidate(&mut candidates, stamp.plugin_spec.clone());
        push_candidate(&mut candidates, stamp.plugin_name.clone());
    }
    if let Some(plugin_id) = existing.plugin_id.as_ref() {
        push_candidate(&mut candidates, plugin_id.clone());
    }
    for plugin in installed_plugins.iter() {
        push_candidate(&mut candidates, plugin.id.clone());
    }
    push_candidate(&mut candidates, PLUGIN_NAME.to_string());
    push_candidate(&mut candidates, LEGACY_CLAUDE_PLUGIN_NAME.to_string());

    let mut attempts = Vec::new();
    let mut removed = false;
    for candidate in candidates {
        let output = run_scoped_host_command(
            target,
            scope,
            project_root,
            &["plugin", "uninstall", "-s", scope.flag(), &candidate],
        )?;
        if output.status.success() {
            removed = true;
            continue;
        }
        attempts.push(render_output(&output));
    }

    if removed || (!existing.installed && !existing.legacy && installed_plugins.is_empty()) {
        remove_stamp_file(target, scope, project_root)?;
        return Ok(());
    }

    bail!(
        "Failed to uninstall Humanize plugin from {} {} scope.\n{}",
        host_display_name(target),
        scope.flag(),
        attempts.join("\n"),
    );
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if candidate.is_empty() || candidates.iter().any(|item| item == &candidate) {
        return;
    }
    candidates.push(candidate);
}

fn scope_matches(host_scope: &str, expected: InstallScope) -> bool {
    match expected {
        InstallScope::User => host_scope == "user",
        InstallScope::Project => matches!(host_scope, "project" | "local"),
    }
}

fn project_root_matches(project_root: Option<&Path>, configured_path: Option<&str>) -> bool {
    let Some(project_root) = project_root else {
        return false;
    };
    let Some(configured_path) = configured_path else {
        return false;
    };

    let configured = Path::new(configured_path);
    if configured == project_root {
        return true;
    }

    match (fs::canonicalize(project_root), fs::canonicalize(configured)) {
        (Ok(lhs), Ok(rhs)) => lhs == rhs,
        _ => false,
    }
}

fn detect_plugin_source(_target: InitTarget) -> Result<String> {
    Ok(DEFAULT_PLUGIN_SOURCE.to_string())
}

fn run_host_command(target: InitTarget, args: &[&str]) -> Result<Output> {
    run_host_command_in_dir(target, args, None)
}

fn run_scoped_host_command(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
    args: &[&str],
) -> Result<Output> {
    let current_dir = match scope {
        InstallScope::User => None,
        InstallScope::Project => Some(
            project_root.context("Project root required for project-scoped plugin operation")?,
        ),
    };
    run_host_command_in_dir(target, args, current_dir)
}

fn run_host_command_in_dir(
    target: InitTarget,
    args: &[&str],
    current_dir: Option<&Path>,
) -> Result<Output> {
    let mut command = Command::new(host_binary(target));
    command.args(args);
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }

    command
        .output()
        .with_context(|| format!("Failed to run `{}`", shell_preview(target, args)))
}

fn project_root_for_scope(scope: InstallScope) -> Result<Option<PathBuf>> {
    match scope {
        InstallScope::User => Ok(None),
        InstallScope::Project => Ok(Some(resolve_project_root()?)),
    }
}

fn init_command(target: InitTarget, scope: InstallScope) -> String {
    format!(
        "humanize init{}{}",
        scope.command_flag(),
        target_init_suffix(target)
    )
}

fn scope_from_host_scope(scope: &str) -> Option<InstallScope> {
    match scope {
        "user" => Some(InstallScope::User),
        "project" | "local" => Some(InstallScope::Project),
        _ => None,
    }
}

fn print_scope_doctor(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
    install_status: &PluginInstallStatus,
    stamp: Option<&PluginSyncStamp>,
) {
    let label = match scope {
        InstallScope::User => "User",
        InstallScope::Project => "Project",
    };

    if let Some(project_root) = project_root {
        println!("  {label} Root:      {}", project_root.display());
    }
    if install_status.installed {
        println!(
            "  {label} Plugin:    yes{}",
            install_status
                .version
                .as_ref()
                .map(|version| format!(" ({version})"))
                .unwrap_or_default()
        );
    } else {
        println!("  {label} Plugin:    no");
    }
    if install_status.installed || install_status.legacy {
        println!("  {label} Detail:    {}", install_status.details);
    }
    if install_status.legacy {
        println!("  {label} Legacy:    yes");
    }

    match stamp {
        Some(stamp) => {
            println!("  {label} Stamp:     {}", stamp.cli_version);
            println!("  {label} Synced At: {}", stamp.installed_at);
        }
        None => println!("  {label} Stamp:     missing"),
    }

    println!(
        "  {label} Action:    {}",
        scope_recommendation(target, scope, install_status, stamp)
    );
}

fn print_windows_codex_doctor() {
    #[cfg(windows)]
    {
        use humanize_core::constants::ENV_CODEX_BIN;

        println!();
        println!("Windows Codex:");

        match std::env::var_os(ENV_CODEX_BIN) {
            Some(value) if !value.is_empty() => {
                println!("  Override:         {}", PathBuf::from(value).display());
            }
            _ => println!("  Override:         not set"),
        }

        match humanize_core::codex::detect_codex_binary() {
            Ok(resolution) => {
                println!("  Resolution:       {} ({})", resolution.launcher, resolution.path.display());
                if matches!(resolution.launcher, "cmd-shim" | "powershell-shim") {
                    println!("  Action:           No action needed. Humanize will invoke the shim automatically.");
                } else {
                    println!("  Action:           No action needed.");
                }
            }
            Err(err) => {
                println!("  Resolution:       missing");
                println!("  Detail:           {}", err);
                println!(
                    "  Action:           Install Codex on PATH or set {} to codex.exe/codex.cmd.",
                    ENV_CODEX_BIN
                );
            }
        }
    }
}

fn scope_recommendation(
    target: InitTarget,
    scope: InstallScope,
    install_status: &PluginInstallStatus,
    stamp: Option<&PluginSyncStamp>,
) -> String {
    if install_status.legacy {
        return format!(
            "Run `{}` to replace the legacy plugin.",
            init_command(target, scope)
        );
    }

    if !install_status.installed {
        return format!("Run `{}`.", init_command(target, scope));
    }

    if stamp
        .map(|stamp| {
            stamp.cli_version != current_cli_version()
                || stamp.scope != scope.flag()
                || stamp.plugin_name != PLUGIN_NAME
        })
        .unwrap_or(true)
    {
        return format!(
            "Run `{}` to refresh the install.",
            init_command(target, scope)
        );
    }

    "No action needed.".to_string()
}

fn remove_stamp_file(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
) -> Result<()> {
    let path = stamp_path(target, scope, project_root)?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("Failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn stamp_path(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
) -> Result<PathBuf> {
    match scope {
        InstallScope::User => Ok(resolve_host_dir(target)?.join(STAMP_FILE)),
        InstallScope::Project => Ok(project_root
            .context("Project root required for local install")?
            .join(".humanize")
            .join(STAMP_FILE)),
    }
}

fn read_stamp(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
) -> Result<Option<PluginSyncStamp>> {
    let path = stamp_path(target, scope, project_root)?;
    if !path.exists() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let stamp = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(stamp))
}

fn write_stamp(
    target: InitTarget,
    scope: InstallScope,
    project_root: Option<&Path>,
    stamp: &PluginSyncStamp,
) -> Result<()> {
    let path = stamp_path(target, scope, project_root)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content =
        serde_json::to_string_pretty(stamp).context("Failed to serialize plugin sync stamp")?;
    fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))
}

fn remove_claude_compat_commands(marketplace_name: &str, version: &str) -> Result<()> {
    let host_dir = resolve_host_dir(InitTarget::Claude)?;
    let source_dir = host_dir
        .join("plugins")
        .join("cache")
        .join(marketplace_name)
        .join(PLUGIN_NAME)
        .join(version)
        .join("commands");
    if !source_dir.is_dir() {
        return Ok(());
    }

    let destination_dir = host_dir.join("commands");
    if !destination_dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(&source_dir)
        .with_context(|| format!("Failed to read {}", source_dir.display()))?
    {
        let entry = entry.with_context(|| format!("Failed to read {}", source_dir.display()))?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        let file_name = entry.file_name();
        let destination = destination_dir.join(format!("humanize-{}", file_name.to_string_lossy()));
        if destination.exists() {
            fs::remove_file(&destination)
                .with_context(|| format!("Failed to remove {}", destination.display()))?;
        }
    }

    Ok(())
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
    let binary = host_binary(target);
    let mut parts = vec![binary.to_string_lossy().into_owned()];
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

fn host_binary(target: InitTarget) -> OsString {
    let env_key = match target {
        InitTarget::Claude => "HUMANIZE_CLAUDE_BIN",
        InitTarget::Droid => "HUMANIZE_DROID_BIN",
    };
    std::env::var_os(env_key).unwrap_or_else(|| match target {
        InitTarget::Claude => OsString::from("claude"),
        InitTarget::Droid => OsString::from("droid"),
    })
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
    fn parse_claude_marketplaces_extracts_windows_style_entries() {
        let output = r#"
Configured marketplaces:

  > claude-plugins-official
    Source: GitHub (anthropics/claude-plugins-official)

  > humania-rs
    Source: Git (https://github.com/Cupnfish/humanize-rs.git)
"#;
        let marketplaces = parse_claude_marketplaces(output);
        assert_eq!(marketplaces.len(), 2);
        assert_eq!(marketplaces[0].name, "claude-plugins-official");
        assert_eq!(
            marketplaces[1].source,
            "Git (https://github.com/Cupnfish/humanize-rs.git)"
        );
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
    fn stamp_source_matches_github_slug() {
        let stamp = PluginSyncStamp {
            target: "claude".to_string(),
            cli_version: "0.3.4".to_string(),
            plugin_name: PLUGIN_NAME.to_string(),
            plugin_spec: "humanize-rs@humania-rs".to_string(),
            marketplace_name: MARKETPLACE_NAME.to_string(),
            source: "GitHub (Cupnfish/humanize-rs)".to_string(),
            scope: "user".to_string(),
            installed_at: "2026-03-19T00:00:00Z".to_string(),
        };
        assert!(stamp_source_matches(
            Some(&stamp),
            "https://github.com/Cupnfish/humanize-rs.git"
        ));
    }

    #[test]
    fn marketplace_source_is_local_for_claude_directory_entries() {
        assert!(marketplace_source_is_local("Directory (/tmp/humanize)"));
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

    #[test]
    fn stamp_needs_migration_when_scope_changes() {
        let stamp = PluginSyncStamp {
            target: "claude".to_string(),
            cli_version: "0.3.5".to_string(),
            plugin_name: PLUGIN_NAME.to_string(),
            plugin_spec: "humanize-rs@humania-rs".to_string(),
            marketplace_name: MARKETPLACE_NAME.to_string(),
            source: DEFAULT_PLUGIN_SOURCE.to_string(),
            scope: "user".to_string(),
            installed_at: "2026-03-19T00:00:00Z".to_string(),
        };

        assert!(stamp_needs_migration(
            Some(&stamp),
            InstallScope::Project,
            DEFAULT_PLUGIN_SOURCE,
            "humanize-rs@humania-rs"
        ));
        assert!(!stamp_needs_migration(
            Some(&stamp),
            InstallScope::User,
            DEFAULT_PLUGIN_SOURCE,
            "humanize-rs@humania-rs"
        ));
    }

    #[test]
    fn project_stamp_path_uses_humanize_directory() {
        let root = Path::new("/tmp/example-project");
        let path = stamp_path(InitTarget::Claude, InstallScope::Project, Some(root)).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/tmp/example-project/.humanize/humanize-plugin-sync.json")
        );
    }

    #[cfg(unix)]
    fn exit_status(code: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn exit_status(code: i32) -> std::process::ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code as u32)
    }
}
