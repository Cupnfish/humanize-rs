use super::pr::resolve_project_root;
use super::*;
use serde::Serialize;
use serde_json::{Map, Value, json};

const DEFAULT_CONFIG_JSON: &str = r#"{
  "alternative_plan_language": "",
  "gen_plan_mode": "discussion"
}"#;

#[derive(Debug, Serialize)]
struct MergedConfigEnvelope {
    merged: Value,
    explicit_user_keys: Vec<String>,
    explicit_project_keys: Vec<String>,
    user_config_path: String,
    user_config_loaded: bool,
    project_config_path: String,
    project_config_loaded: bool,
}

pub(super) fn handle_config(cmd: ConfigCommands) -> Result<()> {
    match cmd {
        ConfigCommands::Merged { with_meta, json } => handle_config_merged(with_meta, json),
    }
}

fn handle_config_merged(with_meta: bool, json_output: bool) -> Result<()> {
    let project_root = resolve_project_root()?;
    let user_config_path = resolve_user_config_path();
    let project_config_path = resolve_project_config_path(&project_root);

    let default_value: Value =
        serde_json::from_str(DEFAULT_CONFIG_JSON).context("invalid built-in default config")?;
    let default_value = strip_nulls(default_value);

    let (user_value, explicit_user_keys, user_loaded) =
        load_optional_config_layer(&user_config_path, "user config")?;
    let (project_value, explicit_project_keys, project_loaded) =
        load_optional_config_layer(&project_config_path, "project config")?;

    let mut merged = default_value;
    merge_value(&mut merged, user_value);
    merge_value(&mut merged, project_value);

    if with_meta {
        let envelope = MergedConfigEnvelope {
            merged,
            explicit_user_keys,
            explicit_project_keys,
            user_config_path: user_config_path.display().to_string(),
            user_config_loaded: user_loaded,
            project_config_path: project_config_path.display().to_string(),
            project_config_loaded: project_loaded,
        };
        if json_output {
            println!("{}", serde_json::to_string(&envelope)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&envelope)?);
        }
    } else if json_output {
        println!("{}", serde_json::to_string(&merged)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&merged)?);
    }

    Ok(())
}

fn resolve_user_config_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("humanize").join("config.json")
    } else if let Some(dir) = dirs::config_dir() {
        dir.join("humanize").join("config.json")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".config")
            .join("humanize")
            .join("config.json")
    } else {
        PathBuf::from(".config")
            .join("humanize")
            .join("config.json")
    }
}

fn resolve_project_config_path(project_root: &Path) -> PathBuf {
    if let Ok(path) = std::env::var("HUMANIZE_CONFIG") {
        PathBuf::from(path)
    } else {
        project_root.join(".humanize").join("config.json")
    }
}

fn load_optional_config_layer(path: &Path, label: &str) -> Result<(Value, Vec<String>, bool)> {
    if !path.is_file() {
        return Ok((json!({}), Vec::new(), false));
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}: {}", label, path.display()))?;
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => {
            eprintln!(
                "Warning: ignoring malformed {} (must be a JSON object): {}",
                label,
                path.display()
            );
            return Ok((json!({}), Vec::new(), false));
        }
    };

    let stripped = strip_nulls(parsed);
    let object = match stripped {
        Value::Object(object) => object,
        _ => {
            eprintln!(
                "Warning: ignoring malformed {} (must be a JSON object): {}",
                label,
                path.display()
            );
            return Ok((json!({}), Vec::new(), false));
        }
    };

    let mut explicit_keys = object.keys().cloned().collect::<Vec<_>>();
    explicit_keys.sort();
    Ok((Value::Object(object), explicit_keys, true))
}

fn strip_nulls(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let cleaned = map
                .into_iter()
                .filter_map(|(key, value)| {
                    if value.is_null() {
                        None
                    } else {
                        Some((key, strip_nulls(value)))
                    }
                })
                .collect::<Map<_, _>>();
            Value::Object(cleaned)
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .filter(|item| !item.is_null())
                .map(strip_nulls)
                .collect(),
        ),
        other => other,
    }
}

fn merge_value(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                match base_map.get_mut(&key) {
                    Some(base_value) => merge_value(base_value, overlay_value),
                    None => {
                        base_map.insert(key, overlay_value);
                    }
                }
            }
        }
        (base_value, overlay_value) => *base_value = overlay_value,
    }
}
