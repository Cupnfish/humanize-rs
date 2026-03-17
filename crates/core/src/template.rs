//! Template loading and single-pass variable rendering for Humanize.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Errors that can occur while loading or rendering templates.
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("Template not found: {0}")]
    NotFound(String),

    #[error("Invalid template path: {0}")]
    InvalidPath(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Resolve the prompt-template directory from a plugin root.
pub fn template_dir(plugin_root: &Path) -> PathBuf {
    plugin_root.join("prompt-template")
}

/// Load a template file from a template root.
pub fn load_template(template_root: &Path, template_name: &str) -> Result<String, TemplateError> {
    let path = template_root.join(template_name);

    if path.is_absolute() && !path.starts_with(template_root) {
        return Err(TemplateError::InvalidPath(template_name.to_string()));
    }

    if !path.exists() {
        return Err(TemplateError::NotFound(path.display().to_string()));
    }

    Ok(std::fs::read_to_string(path)?)
}

/// Render a template using single-pass `{{VAR}}` substitution.
///
/// Missing variables are left intact, matching the shell template loader.
pub fn render_template(template: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut i = 0;

    while i < template.len() {
        let remaining = &template[i..];
        if !remaining.starts_with("{{") {
            let ch = remaining.chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }

        let after_open = i + 2;
        if let Some(close_rel) = template[after_open..].find("}}") {
            let close = after_open + close_rel;
            let key = &template[after_open..close];
            if let Some(value) = vars.get(key) {
                out.push_str(value);
            } else {
                out.push_str(&template[i..close + 2]);
            }
            i = close + 2;
        } else {
            out.push_str("{{");
            i += 2;
        }
    }

    out
}

/// Load and render a template file in one step.
pub fn load_and_render(
    template_root: &Path,
    template_name: &str,
    vars: &HashMap<String, String>,
) -> Result<String, TemplateError> {
    let template = load_template(template_root, template_name)?;
    Ok(render_template(&template, vars))
}

/// Load and render a template with a fallback string.
pub fn load_and_render_safe(
    template_root: &Path,
    template_name: &str,
    fallback: &str,
    vars: &HashMap<String, String>,
) -> String {
    match load_and_render(template_root, template_name, vars) {
        Ok(rendered) if !rendered.is_empty() => rendered,
        _ => render_template(fallback, vars),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_template_replaces_known_variables_and_keeps_unknown() {
        let mut vars = HashMap::new();
        vars.insert("PLAN_FILE".to_string(), "docs/plan.md".to_string());

        let rendered = render_template("Plan: {{PLAN_FILE}} {{UNKNOWN}}", &vars);
        assert_eq!(rendered, "Plan: docs/plan.md {{UNKNOWN}}");
    }

    #[test]
    fn render_template_is_single_pass() {
        let mut vars = HashMap::new();
        vars.insert("A".to_string(), "{{B}}".to_string());
        vars.insert("B".to_string(), "expanded".to_string());

        let rendered = render_template("Value: {{A}}", &vars);
        assert_eq!(rendered, "Value: {{B}}");
    }

    #[test]
    fn load_and_render_safe_falls_back_when_template_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vars = HashMap::new();
        vars.insert("FIELD_NAME".to_string(), "plan_tracked".to_string());

        let rendered = load_and_render_safe(
            tmp.path(),
            "block/schema-outdated.md",
            "Missing {{FIELD_NAME}}",
            &vars,
        );
        assert_eq!(rendered, "Missing plan_tracked");
    }
}
