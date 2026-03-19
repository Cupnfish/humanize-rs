use super::pr::*;
use super::*;

#[derive(Debug, Clone, Deserialize)]
struct GenPlanAnalysis {
    #[serde(default)]
    issues: Vec<GenPlanIssue>,
    #[serde(default)]
    metrics: Vec<GenPlanMetric>,
    #[serde(default)]
    mixed_languages: bool,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GenPlanIssue {
    question: String,
    why: String,
    #[serde(default)]
    options: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GenPlanMetric {
    text: String,
    #[serde(default)]
    question: String,
    #[serde(default)]
    suggested_default: String,
}

struct PreparedGenPlan {
    project_root: PathBuf,
    draft: String,
    template: String,
    output_path: PathBuf,
}

pub(super) fn handle_gen_plan(input: &str, output: &str, prepare_only: bool) -> Result<()> {
    if prepare_only {
        prepare_gen_plan_output(input, output)?;
        return Ok(());
    }

    gen_plan_native(input, output)
}

fn gen_plan_native(input: &str, output: &str) -> Result<()> {
    let prepared = prepare_gen_plan_output(input, output)?;
    let project_root = prepared.project_root;
    let draft = prepared.draft;
    let template = prepared.template;
    let output_path = prepared.output_path;
    ensure_command_exists("codex", "Error: gen-plan requires codex to be installed")?;

    let mut options = humanize_core::codex::CodexOptions::from_env(&project_root);
    options.model = "gpt-5.4".to_string();
    options.effort = "xhigh".to_string();
    options.timeout_secs = 3600;

    let repo_context = build_repo_context(&project_root)?;
    let relevance = run_gen_plan_relevance_check(&draft, &repo_context, &options)?;
    if let Some(reason) = relevance.strip_prefix("NOT_RELEVANT:") {
        bail!(
            "The draft content does not appear to be related to this repository.\n{}",
            reason.trim()
        );
    }
    if !relevance.starts_with("RELEVANT:") {
        bail!(
            "gen-plan relevance check returned invalid output: {}",
            relevance
        );
    }

    let analysis = run_gen_plan_analysis(&draft, &repo_context, &options)?;
    let clarifications = collect_gen_plan_issue_answers(&analysis)?;
    let metric_answers = collect_gen_plan_metric_answers(&analysis)?;
    let prompt = build_gen_plan_generation_prompt(
        &template,
        &draft,
        &repo_context,
        &clarifications,
        &metric_answers,
        &analysis.notes,
    );

    let result = humanize_core::codex::run_exec(&prompt, &options)
        .map_err(|err| anyhow::anyhow!("gen-plan Codex generation failed: {}", err))?;
    let mut content = strip_markdown_fence(&result.stdout).trim().to_string();
    if !content.contains("--- Original Design Draft Start ---") {
        content.push_str(&format!(
            "\n\n--- Original Design Draft Start ---\n\n{}\n\n--- Original Design Draft End ---\n",
            draft.trim_end()
        ));
    }

    if should_offer_language_unification(&analysis, &content) {
        if let Some(language) = prompt_language_unification()? {
            content = run_gen_plan_language_unification(&content, &language, &options)?;
        }
    }

    fs::write(&output_path, format!("{}\n", content.trim_end()))?;
    Ok(())
}

fn prepare_gen_plan_output(input: &str, output: &str) -> Result<PreparedGenPlan> {
    let input_path = PathBuf::from(input);
    let output_path = PathBuf::from(output);

    if !input_path.is_file() {
        bail!("Input file not found: {}", input_path.display());
    }

    let draft = fs::read_to_string(&input_path)?;
    if draft.trim().is_empty() {
        bail!("Input file is empty: {}", input_path.display());
    }

    let output_dir = output_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Output directory does not exist"))?;
    if !output_dir.is_dir() {
        bail!("Output directory does not exist: {}", output_dir.display());
    }
    if output_path.exists() {
        bail!("Output file already exists: {}", output_path.display());
    }

    let template = embedded_template_contents("plan/gen-plan-template.md")
        .context("Plan template file not found")?
        .to_string();
    let project_root = resolve_project_root()?;

    let scaffold = format!(
        "{}\n\n--- Original Design Draft Start ---\n\n{}\n\n--- Original Design Draft End ---\n",
        template.trim_end(),
        draft.trim_end()
    );
    fs::write(&output_path, scaffold)?;

    Ok(PreparedGenPlan {
        project_root,
        draft,
        template,
        output_path,
    })
}

fn strip_markdown_fence(content: &str) -> String {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let mut lines = trimmed.lines();
    let _ = lines.next();
    let collected = lines.collect::<Vec<_>>();
    if let Some(last) = collected.last() {
        if last.trim() == "```" {
            return collected[..collected.len().saturating_sub(1)].join("\n");
        }
    }
    trimmed.to_string()
}

fn build_repo_context(project_root: &Path) -> Result<String> {
    let mut parts = Vec::new();
    parts.push(format!("Project root: {}", project_root.display()));

    let mut top_entries = fs::read_dir(project_root)?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    top_entries.sort();
    parts.push(format!("Top-level entries: {}", top_entries.join(", ")));

    for candidate in ["README.md", "CLAUDE.md", "docs/README.md"] {
        let path = project_root.join(candidate);
        if path.is_file() {
            let content = fs::read_to_string(&path).unwrap_or_default();
            let snippet = content.lines().take(80).collect::<Vec<_>>().join("\n");
            parts.push(format!("## {}\n{}", candidate, snippet));
        }
    }

    Ok(parts.join("\n\n"))
}

fn run_gen_plan_relevance_check(
    draft: &str,
    repo_context: &str,
    options: &humanize_core::codex::CodexOptions,
) -> Result<String> {
    let prompt = format!(
        "You are checking whether a draft is relevant to the current repository.\n\nRepository context:\n{}\n\nDraft:\n{}\n\nReturn exactly one line:\n- `RELEVANT: <brief explanation>`\n- `NOT_RELEVANT: <brief explanation>`\n\nBe lenient. Only return NOT_RELEVANT when the draft is clearly unrelated.",
        repo_context.trim(),
        draft.trim()
    );
    let result = humanize_core::codex::run_exec(&prompt, options)
        .map_err(|err| anyhow::anyhow!("gen-plan relevance check failed: {}", err))?;
    Ok(strip_markdown_fence(&result.stdout)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string())
}

fn run_gen_plan_analysis(
    draft: &str,
    repo_context: &str,
    options: &humanize_core::codex::CodexOptions,
) -> Result<GenPlanAnalysis> {
    let prompt = format!(
        "Analyze the draft for gen-plan.\n\nRepository context:\n{}\n\nDraft:\n{}\n\nReturn JSON only with this shape:\n{{\n  \"issues\": [{{\"question\": \"...\", \"why\": \"...\", \"options\": [\"...\", \"...\"]}}],\n  \"metrics\": [{{\"text\": \"...\", \"question\": \"...\", \"suggested_default\": \"hard|trend\"}}],\n  \"mixed_languages\": true,\n  \"language_candidates\": [\"English\", \"Chinese\"],\n  \"notes\": [\"...\"]\n}}\n\nRules:\n- `issues` should only include clarifications that materially affect the plan.\n- `metrics` should include each quantitative target or numeric threshold that needs hard-vs-trend confirmation.\n- If there are no issues or metrics, return empty arrays.\n- Be conservative: avoid spurious issues.\n- JSON only. No markdown fences.",
        repo_context.trim(),
        draft.trim()
    );
    let result = humanize_core::codex::run_exec(&prompt, options)
        .map_err(|err| anyhow::anyhow!("gen-plan analysis failed: {}", err))?;
    let json = strip_markdown_fence(&result.stdout);
    serde_json::from_str(json.trim())
        .map_err(|err| anyhow::anyhow!("gen-plan analysis returned invalid JSON: {}", err))
}

fn interactive_stdin() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn prompt_user_input(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn collect_gen_plan_issue_answers(analysis: &GenPlanAnalysis) -> Result<Vec<(String, String)>> {
    if analysis.issues.is_empty() {
        return Ok(Vec::new());
    }
    if !interactive_stdin() {
        let questions = analysis
            .issues
            .iter()
            .enumerate()
            .map(|(idx, issue)| format!("{}. {} ({})", idx + 1, issue.question, issue.why))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "gen-plan requires user clarification before continuing.\nRun this command in an interactive terminal and answer:\n{}",
            questions
        );
    }

    let mut answers = Vec::new();
    for (idx, issue) in analysis.issues.iter().enumerate() {
        eprintln!("\ngen-plan clarification {}:", idx + 1);
        eprintln!("Question: {}", issue.question);
        eprintln!("Why it matters: {}", issue.why);
        if !issue.options.is_empty() {
            eprintln!("Suggested options:");
            for option in &issue.options {
                eprintln!("- {}", option);
            }
        }
        let answer = prompt_user_input("Your answer: ")?;
        if answer.is_empty() {
            bail!("Clarification answer cannot be empty.");
        }
        answers.push((issue.question.clone(), answer));
    }
    Ok(answers)
}

fn collect_gen_plan_metric_answers(analysis: &GenPlanAnalysis) -> Result<Vec<(String, String)>> {
    if analysis.metrics.is_empty() {
        return Ok(Vec::new());
    }
    if !interactive_stdin() {
        let questions = analysis
            .metrics
            .iter()
            .enumerate()
            .map(|(idx, metric)| format!("{}. {}", idx + 1, metric.text))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "gen-plan requires metric confirmation before continuing.\nRun this command in an interactive terminal and classify each metric as `hard` or `trend`:\n{}",
            questions
        );
    }

    let mut answers = Vec::new();
    for (idx, metric) in analysis.metrics.iter().enumerate() {
        let default_hint = if metric.suggested_default.is_empty() {
            "hard/trend"
        } else {
            metric.suggested_default.as_str()
        };
        eprintln!("\ngen-plan metric confirmation {}:", idx + 1);
        eprintln!("Metric: {}", metric.text);
        if !metric.question.is_empty() {
            eprintln!("{}", metric.question);
        }
        let answer = prompt_user_input(&format!("Interpretation [{}]: ", default_hint))?;
        let normalized = if answer.is_empty() {
            metric.suggested_default.clone()
        } else {
            answer.to_ascii_lowercase()
        };
        if normalized != "hard" && normalized != "trend" {
            bail!("Metric interpretation must be `hard` or `trend`.");
        }
        answers.push((metric.text.clone(), normalized));
    }
    Ok(answers)
}

fn build_gen_plan_generation_prompt(
    template: &str,
    draft: &str,
    repo_context: &str,
    clarifications: &[(String, String)],
    metric_answers: &[(String, String)],
    notes: &[String],
) -> String {
    let clarifications_block = if clarifications.is_empty() {
        "None.".to_string()
    } else {
        clarifications
            .iter()
            .map(|(question, answer)| format!("- {} => {}", question, answer))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let metrics_block = if metric_answers.is_empty() {
        "None.".to_string()
    } else {
        metric_answers
            .iter()
            .map(|(metric, answer)| format!("- {} => {}", metric, answer))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let notes_block = if notes.is_empty() {
        "None.".to_string()
    } else {
        notes
            .iter()
            .map(|note| format!("- {}", note))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "Read and execute below with ultrathink.\n\nYou are generating a complete implementation plan for the current repository.\n\nRepository context:\n{}\n\nOriginal draft:\n{}\n\nClarifications from the user:\n{}\n\nMetric interpretations:\n{}\n\nAdditional analysis notes:\n{}\n\nRequirements:\n- Preserve ALL meaningful information from the draft.\n- Treat clarifications as additive, not replacements.\n- Write the final answer as raw markdown only, with no code fences.\n- Keep the final `--- Original Design Draft Start ---` and `--- Original Design Draft End ---` section at the bottom.\n- Follow the plan structure and headings from the template exactly unless a heading is clearly inapplicable.\n- Acceptance criteria must use AC-X or AC-X.Y naming.\n- Do not include time estimates.\n- Do not reference code line numbers.\n- Include positive and negative tests for each acceptance criterion.\n- Path boundaries must describe acceptable upper/lower bounds and allowed choices.\n- For each quantitative metric, reflect whether it is a hard requirement or an optimization trend.\n- Include implementation notes telling engineers not to use plan-specific markers like AC-, Milestone, Step, or Phase inside production code/comments.\n\nTemplate skeleton:\n\n{}\n",
        repo_context.trim(),
        draft.trim(),
        clarifications_block,
        metrics_block,
        notes_block,
        template.trim_end(),
    )
}

fn contains_cjk(text: &str) -> bool {
    text.chars()
        .any(|ch| ('\u{4E00}'..='\u{9FFF}').contains(&ch))
}

fn should_offer_language_unification(analysis: &GenPlanAnalysis, content: &str) -> bool {
    analysis.mixed_languages
        || (contains_cjk(content) && content.chars().any(|ch| ch.is_ascii_alphabetic()))
}

fn prompt_language_unification() -> Result<Option<String>> {
    if !interactive_stdin() {
        return Ok(None);
    }
    eprintln!("\ngen-plan detected mixed-language content.");
    eprintln!("Choose language handling:");
    eprintln!("- keep");
    eprintln!("- english");
    eprintln!("- chinese");
    let answer = prompt_user_input("Language choice [keep]: ")?;
    let normalized = if answer.is_empty() {
        "keep".to_string()
    } else {
        answer.to_ascii_lowercase()
    };
    match normalized.as_str() {
        "keep" => Ok(None),
        "english" | "chinese" => Ok(Some(normalized)),
        _ => bail!("Language choice must be keep, english, or chinese."),
    }
}

fn run_gen_plan_language_unification(
    content: &str,
    language: &str,
    options: &humanize_core::codex::CodexOptions,
) -> Result<String> {
    let language_label = match language {
        "english" => "English",
        "chinese" => "Chinese",
        _ => language,
    };
    let prompt = format!(
        "Translate the following plan into {} while preserving exact meaning, structure, markdown formatting, and all technical identifiers.\n\nReturn raw markdown only.\n\n{}",
        language_label,
        content.trim()
    );
    let result = humanize_core::codex::run_exec(&prompt, options)
        .map_err(|err| anyhow::anyhow!("gen-plan language unification failed: {}", err))?;
    Ok(strip_markdown_fence(&result.stdout).trim().to_string())
}
