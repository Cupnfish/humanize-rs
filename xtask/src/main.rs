use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use serde_json::ser::{PrettyFormatter, Serializer};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

fn main() {
    if let Err(err) = run() {
        eprintln!("xtask error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(cmd) = args.next() else {
        bail!("usage: cargo xtask <sync-version|verify-version-sync>");
    };

    match cmd.as_str() {
        "sync-version" => sync_version(),
        "verify-version-sync" => verify_version_sync(),
        other => bail!("unknown xtask command: {other}"),
    }
}

fn sync_version() -> Result<()> {
    let root = workspace_root()?;
    let version = workspace_version(&root)?;

    sync_root_cargo_dependency_version(&root.join("Cargo.toml"), &version)?;
    sync_plugin_json(&root.join(".claude-plugin").join("plugin.json"), &version)?;
    sync_marketplace_json(
        &root.join(".claude-plugin").join("marketplace.json"),
        &version,
    )?;

    println!("Synced plugin manifests to version {version}");
    Ok(())
}

fn verify_version_sync() -> Result<()> {
    let root = workspace_root()?;
    let version = workspace_version(&root)?;

    verify_root_cargo_dependency_version(&root.join("Cargo.toml"), &version)?;
    verify_plugin_json(&root.join(".claude-plugin").join("plugin.json"), &version)?;
    verify_marketplace_json(
        &root.join(".claude-plugin").join("marketplace.json"),
        &version,
    )?;

    println!("Version sync OK: {version}");
    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live under the workspace root")
}

fn workspace_version(root: &Path) -> Result<String> {
    let cargo_toml =
        fs::read_to_string(root.join("Cargo.toml")).context("failed to read root Cargo.toml")?;
    let doc = cargo_toml
        .parse::<DocumentMut>()
        .context("failed to parse root Cargo.toml")?;
    doc["workspace"]["package"]["version"]
        .as_str()
        .map(ToOwned::to_owned)
        .context("missing [workspace.package].version in root Cargo.toml")
}

fn sync_plugin_json(path: &Path, version: &str) -> Result<()> {
    let mut value = read_json(path)?;
    value["version"] = Value::String(version.to_string());
    write_pretty_json(path, &value)
}

fn sync_root_cargo_dependency_version(path: &Path, version: &str) -> Result<()> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut doc = content
        .parse::<DocumentMut>()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    doc["workspace"]["dependencies"]["humanize-core"]["version"] = toml_edit::value(version);
    fs::write(path, doc.to_string()).with_context(|| format!("failed to write {}", path.display()))
}

fn sync_marketplace_json(path: &Path, version: &str) -> Result<()> {
    let mut value = read_json(path)?;
    let plugins = value
        .get_mut("plugins")
        .and_then(Value::as_array_mut)
        .context("marketplace.json is missing plugins array")?;
    for plugin in plugins {
        if let Some(obj) = plugin.as_object_mut() {
            obj.insert("version".to_string(), Value::String(version.to_string()));
        }
    }
    write_pretty_json(path, &value)
}

fn verify_plugin_json(path: &Path, version: &str) -> Result<()> {
    let value = read_json(path)?;
    let actual = value
        .get("version")
        .and_then(Value::as_str)
        .context("plugin.json is missing version")?;
    if actual != version {
        bail!(
            "plugin.json version mismatch: expected {version}, found {actual}. Run `cargo xtask sync-version`."
        );
    }
    Ok(())
}

fn verify_root_cargo_dependency_version(path: &Path, version: &str) -> Result<()> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let doc = content
        .parse::<DocumentMut>()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let actual = doc["workspace"]["dependencies"]["humanize-core"]["version"]
        .as_str()
        .context("root Cargo.toml is missing workspace.dependencies.humanize-core.version")?;
    if actual != version {
        bail!(
            "root Cargo.toml humanize-core version mismatch: expected {version}, found {actual}. Run `cargo xtask sync-version`."
        );
    }
    Ok(())
}

fn verify_marketplace_json(path: &Path, version: &str) -> Result<()> {
    let value = read_json(path)?;
    let plugins = value
        .get("plugins")
        .and_then(Value::as_array)
        .context("marketplace.json is missing plugins array")?;
    for plugin in plugins {
        let name = plugin
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let actual = plugin
            .get("version")
            .and_then(Value::as_str)
            .context("marketplace.json plugin entry is missing version")?;
        if actual != version {
            bail!(
                "marketplace.json version mismatch for {name}: expected {version}, found {actual}. Run `cargo xtask sync-version`."
            );
        }
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
}

fn write_pretty_json(path: &Path, value: &Value) -> Result<()> {
    let mut out = Vec::new();
    let formatter = PrettyFormatter::with_indent(b"    ");
    let mut ser = Serializer::with_formatter(&mut out, formatter);
    value
        .serialize(&mut ser)
        .with_context(|| format!("failed to serialize {}", path.display()))?;
    out.push(b'\n');
    fs::write(path, out).with_context(|| format!("failed to write {}", path.display()))
}
