#![cfg(windows)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct InitTestEnv {
    _tempdir: TempDir,
    project_dir: PathBuf,
    bin_dir: PathBuf,
    host_dir: PathBuf,
    state_file: PathBuf,
    log_file: PathBuf,
}

impl InitTestEnv {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let project_dir = root.join("project");
        let bin_dir = root.join("bin");
        let host_dir = root.join("claude-home");
        let state_file = root.join("mock-claude-state.json");
        let log_file = root.join("mock-claude.log");

        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&host_dir).unwrap();
        fs::create_dir_all(host_dir.join("commands")).unwrap();
        fs::write(
            host_dir.join("commands").join("humanize-gen-plan.md"),
            "stale command wrapper\n",
        )
        .unwrap();

        fs::write(
            &state_file,
            serde_json::json!({
                "marketplaces": [
                    {
                        "name": "humania-rs",
                        "source": "Git (https://github.com/Cupnfish/humanize-rs.git)"
                    }
                ],
                "plugins": [
                    {
                        "id": "humanize@humania-rs",
                        "version": "0.2.9",
                        "scope": "user"
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();

        fs::write(
            bin_dir.join("claude.cmd"),
            "@echo off\r\npwsh -NoProfile -ExecutionPolicy Bypass -File \"%~dp0mock-claude.ps1\" %*\r\n",
        )
        .unwrap();
        fs::write(
            bin_dir.join("codex.cmd"),
            "@echo off\r\necho codex mock\r\n",
        )
        .unwrap();
        fs::write(bin_dir.join("mock-claude.ps1"), mock_claude_script()).unwrap();

        Self {
            _tempdir: tempdir,
            project_dir,
            bin_dir,
            host_dir,
            state_file,
            log_file,
        }
    }

    fn path_env(&self) -> OsString {
        let mut paths = vec![self.bin_dir.clone()];
        paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
        env::join_paths(paths).unwrap()
    }

    fn command(&self) -> Command {
        let mut command = Command::new(bin());
        command.current_dir(&self.project_dir);
        command.env("PATH", self.path_env());
        command.env("HUMANIZE_CLAUDE_BIN", self.bin_dir.join("claude.cmd"));
        command.env("HUMANIZE_CLAUDE_DIR", &self.host_dir);
        command.env("MOCK_CLAUDE_STATE_FILE", &self.state_file);
        command.env("MOCK_CLAUDE_LOG_FILE", &self.log_file);
        command.env("CLAUDE_PROJECT_DIR", &self.project_dir);
        command
    }

    fn state(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(&self.state_file).unwrap()).unwrap()
    }

    fn compat_command(&self, name: &str) -> PathBuf {
        self.host_dir.join("commands").join(name)
    }

    fn project_stamp(&self) -> PathBuf {
        self.project_dir
            .join(".humanize")
            .join("humanize-plugin-sync.json")
    }
}

#[test]
fn init_global_replaces_legacy_claude_plugin_and_doctor_sees_marketplace() {
    let env = InitTestEnv::new();
    let current_version = env!("CARGO_PKG_VERSION");

    let init = env.command().args(["init", "--global"]).output().unwrap();
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let stdout = String::from_utf8_lossy(&init.stdout);
    assert!(stdout.contains("Humanize plugin installed for Claude Code."));
    assert!(stdout.contains("Plugin:      humanize-rs@humania-rs"));
    assert!(stdout.contains(&format!("CLI Version: {}", current_version)));

    let state = env.state();
    let plugins = state.get("plugins").and_then(Value::as_array).unwrap();
    let plugin_ids = plugins
        .iter()
        .filter_map(|plugin| plugin.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(
        plugin_ids,
        vec!["humanize-rs@humania-rs"],
        "mock log:\n{}",
        fs::read_to_string(&env.log_file).unwrap_or_default()
    );
    assert_eq!(
        plugins[0].get("id").and_then(Value::as_str),
        Some("humanize-rs@humania-rs")
    );
    assert_eq!(
        plugins[0].get("version").and_then(Value::as_str),
        Some(current_version)
    );
    assert_eq!(
        plugins[0].get("scope").and_then(Value::as_str),
        Some("user")
    );

    let stamp_path = env.host_dir.join("humanize-plugin-sync.json");
    let stamp: Value = serde_json::from_str(&fs::read_to_string(stamp_path).unwrap()).unwrap();
    assert_eq!(
        stamp.get("plugin_spec").and_then(Value::as_str),
        Some("humanize-rs@humania-rs")
    );
    assert_eq!(stamp.get("scope").and_then(Value::as_str), Some("user"));
    assert_eq!(
        stamp.get("cli_version").and_then(Value::as_str),
        Some(current_version)
    );

    let doctor = env
        .command()
        .args(["doctor", "--target", "claude"])
        .output()
        .unwrap();
    assert!(
        doctor.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&doctor.stderr)
    );

    let doctor_stdout = String::from_utf8_lossy(&doctor.stdout);
    assert!(doctor_stdout.contains(
        "Marketplace:      humania-rs (Git (https://github.com/Cupnfish/humanize-rs.git))"
    ));
    assert!(doctor_stdout.contains("User Plugin:    yes"));
    assert!(doctor_stdout.contains("User Action:    No action needed."));
    assert!(doctor_stdout.contains("Windows Codex:"));
    assert!(doctor_stdout.contains("Override:         not set"));
    assert!(doctor_stdout.contains("Resolution:       cmd-shim"));
    assert!(doctor_stdout.contains("codex.cmd"));

    assert!(!env.compat_command("humanize-gen-plan.md").exists());
}

#[test]
fn init_project_scope_writes_project_stamp_and_refreshes_compat_commands() {
    let env = InitTestEnv::new();
    let current_version = env!("CARGO_PKG_VERSION");

    let init = env.command().args(["init"]).output().unwrap();
    assert!(
        init.status.success(),
        "project init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let stdout = String::from_utf8_lossy(&init.stdout);
    assert!(stdout.contains("Scope:       project"));
    assert!(stdout.contains("Project:     "));

    let state = env.state();
    let plugins = state.get("plugins").and_then(Value::as_array).unwrap();
    let project_plugin = plugins.iter().find(|plugin| {
        plugin.get("scope").and_then(Value::as_str) == Some("project")
            && plugin.get("id").and_then(Value::as_str) == Some("humanize-rs@humania-rs")
    });
    assert!(project_plugin.is_some(), "state: {}", state);
    assert_eq!(
        project_plugin
            .and_then(|plugin| plugin.get("projectPath"))
            .and_then(Value::as_str),
        Some(env.project_dir.to_string_lossy().as_ref())
    );

    let stamp: Value =
        serde_json::from_str(&fs::read_to_string(env.project_stamp()).unwrap()).unwrap();
    assert_eq!(stamp.get("scope").and_then(Value::as_str), Some("project"));
    assert_eq!(
        stamp.get("cli_version").and_then(Value::as_str),
        Some(current_version)
    );

    assert!(!env.compat_command("humanize-gen-plan.md").exists());
}

#[test]
fn uninstall_global_removes_host_plugin_stamp_and_compat_commands() {
    let env = InitTestEnv::new();

    let init = env.command().args(["init", "--global"]).output().unwrap();
    assert!(
        init.status.success(),
        "global init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    fs::write(
        env.compat_command("humanize-start-rlcr-loop.md"),
        "stale compat wrapper\n",
    )
    .unwrap();

    let uninstall = env
        .command()
        .args(["uninstall", "--global"])
        .output()
        .unwrap();
    assert!(
        uninstall.status.success(),
        "global uninstall failed: {}",
        String::from_utf8_lossy(&uninstall.stderr)
    );

    let stdout = String::from_utf8_lossy(&uninstall.stdout);
    assert!(stdout.contains("Humanize host integration removed from Claude Code user scope."));

    let state = env.state();
    let plugins = state.get("plugins").and_then(Value::as_array).unwrap();
    assert!(
        plugins.is_empty(),
        "expected all Humanize plugins removed, state: {state}"
    );
    assert!(!env.host_dir.join("humanize-plugin-sync.json").exists());
    assert!(!env.compat_command("humanize-start-rlcr-loop.md").exists());
}

#[test]
fn uninstall_project_scope_removes_project_plugin_and_stamp() {
    let env = InitTestEnv::new();

    let init = env.command().args(["init"]).output().unwrap();
    assert!(
        init.status.success(),
        "project init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let uninstall = env.command().args(["uninstall"]).output().unwrap();
    assert!(
        uninstall.status.success(),
        "project uninstall failed: {}",
        String::from_utf8_lossy(&uninstall.stderr)
    );

    let stdout = String::from_utf8_lossy(&uninstall.stdout);
    assert!(stdout.contains("Humanize host integration removed from Claude Code project scope."));

    let state = env.state();
    let plugins = state.get("plugins").and_then(Value::as_array).unwrap();
    assert!(
        plugins.iter().all(|plugin| {
            plugin.get("scope").and_then(Value::as_str) != Some("project")
                || plugin.get("id").and_then(Value::as_str) != Some("humanize-rs@humania-rs")
        }),
        "expected project-scoped Humanize plugin removed, state: {state}"
    );
    assert!(!env.project_stamp().exists());
}

fn mock_claude_script() -> String {
    let version = env!("CARGO_PKG_VERSION");
    r#"
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Args
)

$ErrorActionPreference = 'Stop'
$statePath = $env:MOCK_CLAUDE_STATE_FILE
$logPath = $env:MOCK_CLAUDE_LOG_FILE

function Log-Call([string]$message) {
    if ($logPath) {
        Add-Content -Path $logPath -Value $message
    }
}

function Load-State {
    return Get-Content $statePath -Raw | ConvertFrom-Json -AsHashtable
}

function Save-State($state) {
    $state | ConvertTo-Json -Depth 8 | Set-Content -Path $statePath -Encoding utf8
}

function Plugin-Matches($plugin, [string]$scope, [string]$candidate) {
    if ($plugin.scope -ne $scope) {
        return $false
    }

    $id = [string]$plugin.id
    if ($candidate.Contains('@')) {
        return $id -eq $candidate
    }

    return $id.StartsWith("$candidate@")
}

function Remove-MatchingPlugins($plugins, [string]$scope, [string]$candidate) {
    $remaining = @()
    $removed = $false

    foreach ($plugin in @($plugins)) {
        if (Plugin-Matches $plugin $scope $candidate) {
            $removed = $true
            continue
        }
        $remaining += $plugin
    }

    return @{
        removed = $removed
        plugins = $remaining
    }
}

function Ensure-CacheCommands([string]$version) {
    $cacheRoot = Join-Path $env:HUMANIZE_CLAUDE_DIR "plugins\\cache\\humania-rs\\humanize-rs\\$version\\commands"
    New-Item -ItemType Directory -Force -Path $cacheRoot | Out-Null
    $commands = @(
        'cancel-pr-loop',
        'cancel-rlcr-loop',
        'gen-plan',
        'start-pr-loop',
        'start-rlcr-loop'
    )
    foreach ($command in $commands) {
        Set-Content -Path (Join-Path $cacheRoot "$command.md") -Value "MOCK $command $version`n" -Encoding utf8
    }
}

Log-Call ($Args -join ' ')

if ($Args.Count -ge 3 -and $Args[0] -eq 'plugin' -and $Args[1] -eq 'list' -and $Args[2] -eq '--json') {
    $state = Load-State
    ConvertTo-Json -InputObject @($state.plugins) -Depth 8
    exit 0
}

if ($Args.Count -ge 3 -and $Args[0] -eq 'plugin' -and $Args[1] -eq 'marketplace' -and $Args[2] -eq 'list') {
    $state = Load-State
    Write-Output 'Configured marketplaces:'
    Write-Output ''
    foreach ($marketplace in $state.marketplaces) {
        Write-Output ("  > {0}" -f $marketplace.name)
        Write-Output ("    Source: {0}" -f $marketplace.source)
        Write-Output ''
    }
    exit 0
}

if ($Args.Count -ge 4 -and $Args[0] -eq 'plugin' -and $Args[1] -eq 'marketplace' -and $Args[2] -eq 'add') {
    $source = $Args[3]
    $state = Load-State
    $existing = @($state.marketplaces | Where-Object { $_.source -eq "Git ($source)" })
    if ($existing.Count -eq 0) {
        $state.marketplaces = @($state.marketplaces) + @{
            name = 'humania-rs'
            source = "Git ($source)"
        }
        Save-State $state
    }
    Write-Output 'Adding marketplace...'
    Write-Output "√ Marketplace 'humania-rs' already on disk — declared in user settings"
    exit 0
}

if ($Args.Count -ge 5 -and $Args[0] -eq 'plugin' -and $Args[1] -eq 'uninstall' -and $Args[2] -eq '-s') {
    $scope = $Args[3]
    $candidate = $Args[4]
    $state = Load-State
    $result = Remove-MatchingPlugins $state.plugins $scope $candidate
    if (-not $result.removed) {
        Write-Error "Plugin not installed: $candidate"
        exit 1
    }

    $state.plugins = @($result.plugins)
    Save-State $state
    Write-Output "Uninstalled $candidate"
    exit 0
}

if ($Args.Count -ge 5 -and $Args[0] -eq 'plugin' -and $Args[1] -eq 'update' -and $Args[2] -eq '-s') {
    $scope = $Args[3]
    $plugin = $Args[4]
    $state = Load-State
    $existing = @($state.plugins | Where-Object { $_.scope -eq $scope -and $_.id -eq $plugin })
    if ($existing.Count -eq 0) {
        Write-Error "Plugin not installed: $plugin"
        exit 1
    }

    foreach ($item in $state.plugins) {
        if ($item.scope -eq $scope -and $item.id -eq $plugin) {
            $item.version = '__VERSION__'
            if ($scope -eq 'project') {
                $item.projectPath = $env:CLAUDE_PROJECT_DIR
            }
        }
    }
    Save-State $state
    Ensure-CacheCommands '__VERSION__'
    Write-Output "Updated $plugin"
    exit 0
}

if ($Args.Count -ge 5 -and $Args[0] -eq 'plugin' -and $Args[1] -eq 'install' -and $Args[2] -eq '-s') {
    $scope = $Args[3]
    $plugin = $Args[4]
    $state = Load-State
    $result = Remove-MatchingPlugins $state.plugins $scope $plugin
    $state.plugins = @($result.plugins)
    $pluginRecord = @{
        id = $plugin
        version = '__VERSION__'
        scope = $scope
    }
    if ($scope -eq 'project') {
        $pluginRecord.projectPath = $env:CLAUDE_PROJECT_DIR
    }
    $state.plugins += $pluginRecord
    Save-State $state
    Ensure-CacheCommands '__VERSION__'
    Write-Output "Installed $plugin"
    exit 0
}

Write-Error ("Unsupported mock claude invocation: {0}" -f ($Args -join ' '))
exit 1
"#
    .replace("__VERSION__", version)
}
