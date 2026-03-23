use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const THREAD_ENV_CANDIDATES: &[&str] = &[
    "HUMANIZE_THREAD_ID",
    "CLAUDE_SESSION_ID",
    "CODEX_SESSION_ID",
    "DROID_SESSION_ID",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanningStatus {
    Collecting,
    Generating,
    Ready,
    Used,
    Superseded,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PlanningIndex {
    #[serde(default)]
    pub(crate) drafts: Vec<DraftIndexEntry>,
    #[serde(default)]
    pub(crate) plans: Vec<PlanIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ThreadPlanningState {
    pub(crate) thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) active_draft_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) active_plan_id: Option<String>,
    #[serde(default)]
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DraftIndexEntry {
    pub(crate) id: String,
    pub(crate) handle: String,
    pub(crate) thread_id: String,
    pub(crate) revision: u32,
    pub(crate) status: PlanningStatus,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlanIndexEntry {
    pub(crate) id: String,
    pub(crate) handle: String,
    pub(crate) thread_id: String,
    pub(crate) revision: u32,
    pub(crate) status: PlanningStatus,
    pub(crate) source_draft_id: String,
    pub(crate) source_draft_revision: u32,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DraftSource {
    pub(crate) kind: String,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DraftDocument {
    pub(crate) id: String,
    pub(crate) handle: String,
    pub(crate) thread_id: String,
    pub(crate) revision: u32,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) goal: String,
    #[serde(default)]
    pub(crate) problem: String,
    #[serde(default)]
    pub(crate) in_scope: Vec<String>,
    #[serde(default)]
    pub(crate) out_of_scope: Vec<String>,
    #[serde(default)]
    pub(crate) constraints: Vec<String>,
    #[serde(default)]
    pub(crate) acceptance_signals: Vec<String>,
    #[serde(default)]
    pub(crate) repo_touchpoints: Vec<String>,
    #[serde(default)]
    pub(crate) assumptions: Vec<String>,
    #[serde(default)]
    pub(crate) open_questions_blocking: Vec<String>,
    #[serde(default)]
    pub(crate) open_questions_non_blocking: Vec<String>,
    #[serde(default)]
    pub(crate) sources: Vec<DraftSource>,
    pub(crate) raw_markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DraftMeta {
    pub(crate) id: String,
    pub(crate) handle: String,
    pub(crate) thread_id: String,
    pub(crate) revision: u32,
    pub(crate) status: PlanningStatus,
    pub(crate) title: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlanMeta {
    pub(crate) id: String,
    pub(crate) handle: String,
    pub(crate) thread_id: String,
    pub(crate) revision: u32,
    pub(crate) status: PlanningStatus,
    pub(crate) source_draft_id: String,
    pub(crate) source_draft_revision: u32,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) converged: bool,
    pub(crate) ready_for_rlcr: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DraftArtifact {
    pub(crate) id: String,
    pub(crate) handle: String,
    pub(crate) thread_id: String,
    pub(crate) revision: u32,
    pub(crate) markdown: String,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct PlanArtifact {
    pub(crate) id: String,
    pub(crate) handle: String,
    pub(crate) thread_id: String,
    pub(crate) revision: u32,
    pub(crate) path: PathBuf,
}

pub(crate) struct PlanningStore {
    project_root: PathBuf,
    planning_root: PathBuf,
    index: PlanningIndex,
    thread_state: ThreadPlanningState,
}

impl PlanningStore {
    pub(crate) fn load(project_root: &Path) -> Result<Self> {
        let planning_root = project_root.join(".humanize").join("planning");
        fs::create_dir_all(planning_root.join("drafts"))?;
        fs::create_dir_all(planning_root.join("plans"))?;
        fs::create_dir_all(planning_root.join("threads"))?;

        let index_path = planning_root.join("index.json");
        let index = if index_path.is_file() {
            serde_json::from_str(
                &fs::read_to_string(&index_path)
                    .with_context(|| format!("Failed to read {}", index_path.display()))?,
            )
            .with_context(|| format!("Failed to parse {}", index_path.display()))?
        } else {
            PlanningIndex::default()
        };

        let thread_id = resolve_thread_id();
        let thread_path = planning_root
            .join("threads")
            .join(format!("{thread_id}.json"));
        let thread_state = if thread_path.is_file() {
            serde_json::from_str(
                &fs::read_to_string(&thread_path)
                    .with_context(|| format!("Failed to read {}", thread_path.display()))?,
            )
            .with_context(|| format!("Failed to parse {}", thread_path.display()))?
        } else {
            ThreadPlanningState {
                thread_id,
                active_draft_id: None,
                active_plan_id: None,
                updated_at: iso_now(),
            }
        };

        Ok(Self {
            project_root: project_root.to_path_buf(),
            planning_root,
            index,
            thread_state,
        })
    }

    pub(crate) fn create_draft(
        &mut self,
        markdown: &str,
        title_override: Option<&str>,
    ) -> Result<DraftArtifact> {
        let id = unique_artifact_id("dft");
        let title = title_override
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| infer_title(markdown));
        let handle = unique_handle(
            &slugify_handle(&title, "draft"),
            self.index.drafts.iter().map(|entry| entry.handle.as_str()),
        );
        let revision = 1;
        let created_at = iso_now();
        let draft_dir = self.planning_root.join("drafts").join(&id);
        fs::create_dir_all(&draft_dir)?;

        let markdown = format!("{}\n", markdown.trim_end());
        let draft_path = draft_dir.join("draft.md");
        let document = DraftDocument {
            id: id.clone(),
            handle: handle.clone(),
            thread_id: self.thread_state.thread_id.clone(),
            revision,
            title: title.clone(),
            goal: String::new(),
            problem: String::new(),
            in_scope: Vec::new(),
            out_of_scope: Vec::new(),
            constraints: Vec::new(),
            acceptance_signals: Vec::new(),
            repo_touchpoints: Vec::new(),
            assumptions: Vec::new(),
            open_questions_blocking: Vec::new(),
            open_questions_non_blocking: Vec::new(),
            sources: vec![DraftSource {
                kind: "user".to_string(),
                text: markdown.clone(),
            }],
            raw_markdown: markdown.clone(),
        };
        let meta = DraftMeta {
            id: id.clone(),
            handle: handle.clone(),
            thread_id: self.thread_state.thread_id.clone(),
            revision,
            status: PlanningStatus::Ready,
            title: title.clone(),
            created_at: created_at.clone(),
            updated_at: created_at.clone(),
        };

        write_json(draft_dir.join("draft.json"), &document)?;
        write_json(draft_dir.join("meta.json"), &meta)?;
        fs::write(&draft_path, &markdown)?;

        self.index.drafts.retain(|entry| entry.id != id);
        self.index.drafts.push(DraftIndexEntry {
            id: id.clone(),
            handle: handle.clone(),
            thread_id: self.thread_state.thread_id.clone(),
            revision,
            status: PlanningStatus::Ready,
            updated_at: created_at.clone(),
        });
        self.thread_state.active_draft_id = Some(id.clone());
        self.thread_state.updated_at = created_at;
        self.persist()?;

        Ok(DraftArtifact {
            id,
            handle,
            thread_id: self.thread_state.thread_id.clone(),
            revision,
            markdown,
            path: draft_path,
        })
    }

    pub(crate) fn resolve_draft_for_gen_plan(
        &mut self,
        handle: Option<&str>,
    ) -> Result<DraftArtifact> {
        let entry = if let Some(handle) = handle {
            self.index
                .drafts
                .iter()
                .find(|entry| entry.handle == handle)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Draft not found: {}", handle))?
        } else if let Some(active_id) = self.thread_state.active_draft_id.as_deref() {
            self.index
                .drafts
                .iter()
                .find(|entry| {
                    entry.id == active_id
                        && entry.thread_id == self.thread_state.thread_id
                        && entry.status == PlanningStatus::Ready
                        && self.is_draft_pending(entry)
                })
                .cloned()
                .or_else(|| self.latest_pending_draft_entry())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No draft pending plan generation in the current thread.\nRun `humanize gen-draft` first, or specify --draft <handle>."
                    )
                })?
        } else {
            self.latest_pending_draft_entry().ok_or_else(|| {
                anyhow::anyhow!(
                    "No draft pending plan generation in the current thread.\nRun `humanize gen-draft` first, or specify --draft <handle>."
                )
            })?
        };

        self.thread_state.active_draft_id = Some(entry.id.clone());
        self.thread_state.updated_at = iso_now();
        self.persist()?;
        self.load_draft(&entry.id)
    }

    pub(crate) fn create_plan(
        &mut self,
        draft: &DraftArtifact,
        content: &str,
        converged: bool,
    ) -> Result<PlanArtifact> {
        let id = unique_artifact_id("pln");
        let revision = 1;
        let created_at = iso_now();
        let handle = unique_handle(
            &draft.handle,
            self.index.plans.iter().map(|entry| entry.handle.as_str()),
        );
        let plan_dir = self.planning_root.join("plans").join(&id);
        fs::create_dir_all(&plan_dir)?;
        let plan_path = plan_dir.join("plan.md");
        fs::write(&plan_path, format!("{}\n", content.trim_end()))?;

        let meta = PlanMeta {
            id: id.clone(),
            handle: handle.clone(),
            thread_id: self.thread_state.thread_id.clone(),
            revision,
            status: PlanningStatus::Ready,
            source_draft_id: draft.id.clone(),
            source_draft_revision: draft.revision,
            created_at: created_at.clone(),
            updated_at: created_at.clone(),
            converged,
            ready_for_rlcr: true,
        };
        write_json(plan_dir.join("meta.json"), &meta)?;

        self.index.plans.retain(|entry| entry.id != id);
        self.index.plans.push(PlanIndexEntry {
            id: id.clone(),
            handle: handle.clone(),
            thread_id: self.thread_state.thread_id.clone(),
            revision,
            status: PlanningStatus::Ready,
            source_draft_id: draft.id.clone(),
            source_draft_revision: draft.revision,
            updated_at: created_at.clone(),
        });
        self.thread_state.active_draft_id = Some(draft.id.clone());
        self.thread_state.active_plan_id = Some(id.clone());
        self.thread_state.updated_at = created_at;
        self.persist()?;

        Ok(PlanArtifact {
            id,
            handle,
            thread_id: self.thread_state.thread_id.clone(),
            revision,
            path: plan_path,
        })
    }

    pub(crate) fn resolve_plan_for_setup(&mut self, handle: Option<&str>) -> Result<PlanArtifact> {
        let entry = if let Some(handle) = handle {
            self.index
                .plans
                .iter()
                .find(|entry| entry.handle == handle)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Plan not found: {}", handle))?
        } else if let Some(active_id) = self.thread_state.active_plan_id.as_deref() {
            self.index
                .plans
                .iter()
                .find(|entry| {
                    entry.id == active_id
                        && entry.thread_id == self.thread_state.thread_id
                        && entry.status == PlanningStatus::Ready
                        && self.is_plan_pending(entry)
                })
                .cloned()
                .or_else(|| self.latest_pending_plan_entry())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No plan pending RLCR startup in the current thread.\nRun `humanize gen-plan` first, or specify --plan <handle>."
                    )
                })?
        } else {
            self.latest_pending_plan_entry().ok_or_else(|| {
                anyhow::anyhow!(
                    "No plan pending RLCR startup in the current thread.\nRun `humanize gen-plan` first, or specify --plan <handle>."
                )
            })?
        };

        self.thread_state.active_plan_id = Some(entry.id.clone());
        self.thread_state.updated_at = iso_now();
        self.persist()?;
        self.load_plan(&entry.id)
    }

    pub(crate) fn mark_plan_used(
        &mut self,
        plan_id: &str,
        plan_revision: u32,
        session_id: Option<&str>,
    ) -> Result<()> {
        let now = iso_now();
        let Some(entry) = self
            .index
            .plans
            .iter_mut()
            .find(|entry| entry.id == plan_id)
        else {
            bail!("Plan not found: {}", plan_id);
        };
        entry.status = PlanningStatus::Used;
        entry.updated_at = now.clone();

        let plan_dir = self.planning_root.join("plans").join(plan_id);
        let meta_path = plan_dir.join("meta.json");
        let mut meta: PlanMeta = serde_json::from_str(
            &fs::read_to_string(&meta_path)
                .with_context(|| format!("Failed to read {}", meta_path.display()))?,
        )
        .with_context(|| format!("Failed to parse {}", meta_path.display()))?;
        meta.status = PlanningStatus::Used;
        meta.updated_at = now.clone();
        meta.ready_for_rlcr = false;
        write_json(&meta_path, &meta)?;

        self.thread_state.active_plan_id = Some(plan_id.to_string());
        self.thread_state.updated_at = now;
        self.persist()?;

        if let Some(session_id) = session_id {
            let marker = self
                .planning_root
                .join("plans")
                .join(plan_id)
                .join(format!("session-{}-rev-{}.txt", session_id, plan_revision));
            fs::write(marker, "")?;
        }
        Ok(())
    }

    pub(crate) fn project_relative_path(&self, absolute_path: &Path) -> Result<String> {
        let relative = absolute_path
            .strip_prefix(&self.project_root)
            .with_context(|| format!("Path is outside project: {}", absolute_path.display()))?;
        Ok(relative.to_string_lossy().into_owned())
    }

    fn latest_pending_draft_entry(&self) -> Option<DraftIndexEntry> {
        let mut entries = self
            .index
            .drafts
            .iter()
            .filter(|entry| {
                entry.thread_id == self.thread_state.thread_id
                    && entry.status == PlanningStatus::Ready
                    && self.is_draft_pending(entry)
            })
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        entries.into_iter().next()
    }

    fn latest_pending_plan_entry(&self) -> Option<PlanIndexEntry> {
        let mut entries = self
            .index
            .plans
            .iter()
            .filter(|entry| {
                entry.thread_id == self.thread_state.thread_id
                    && entry.status == PlanningStatus::Ready
                    && self.is_plan_pending(entry)
            })
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        entries.into_iter().next()
    }

    fn is_draft_pending(&self, draft: &DraftIndexEntry) -> bool {
        !self.index.plans.iter().any(|plan| {
            plan.source_draft_id == draft.id && plan.source_draft_revision == draft.revision
        })
    }

    fn is_plan_pending(&self, plan: &PlanIndexEntry) -> bool {
        if plan.status != PlanningStatus::Ready {
            return false;
        }

        let loop_base = self.project_root.join(".humanize").join("rlcr");
        if !loop_base.is_dir() {
            return true;
        }

        for entry in fs::read_dir(&loop_base).into_iter().flatten().flatten() {
            let loop_dir = entry.path();
            if !loop_dir.is_dir() {
                continue;
            }
            let Some(state_path) = humanize_core::state::resolve_any_state_file(&loop_dir) else {
                continue;
            };
            let Ok(state) = humanize_core::state::State::from_file(&state_path) else {
                continue;
            };
            if state.source_plan_id.as_deref() == Some(plan.id.as_str())
                && state.source_plan_revision == Some(plan.revision)
            {
                return false;
            }
        }
        true
    }

    fn load_draft(&self, draft_id: &str) -> Result<DraftArtifact> {
        let draft_dir = self.planning_root.join("drafts").join(draft_id);
        let meta_path = draft_dir.join("meta.json");
        let meta: DraftMeta = serde_json::from_str(
            &fs::read_to_string(&meta_path)
                .with_context(|| format!("Failed to read {}", meta_path.display()))?,
        )
        .with_context(|| format!("Failed to parse {}", meta_path.display()))?;
        let path = draft_dir.join("draft.md");
        let markdown = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        Ok(DraftArtifact {
            id: meta.id,
            handle: meta.handle,
            thread_id: meta.thread_id,
            revision: meta.revision,
            markdown,
            path,
        })
    }

    fn load_plan(&self, plan_id: &str) -> Result<PlanArtifact> {
        let plan_dir = self.planning_root.join("plans").join(plan_id);
        let meta_path = plan_dir.join("meta.json");
        let meta: PlanMeta = serde_json::from_str(
            &fs::read_to_string(&meta_path)
                .with_context(|| format!("Failed to read {}", meta_path.display()))?,
        )
        .with_context(|| format!("Failed to parse {}", meta_path.display()))?;
        Ok(PlanArtifact {
            id: meta.id,
            handle: meta.handle,
            thread_id: meta.thread_id,
            revision: meta.revision,
            path: plan_dir.join("plan.md"),
        })
    }

    fn persist(&self) -> Result<()> {
        write_json(self.planning_root.join("index.json"), &self.index)?;
        write_json(
            self.planning_root
                .join("threads")
                .join(format!("{}.json", self.thread_state.thread_id)),
            &self.thread_state,
        )?;
        Ok(())
    }
}

fn resolve_thread_id() -> String {
    for candidate in THREAD_ENV_CANDIDATES {
        if let Ok(value) = std::env::var(candidate) {
            let sanitized = sanitize_identifier(&value);
            if !sanitized.is_empty() {
                return sanitized;
            }
        }
    }
    "default".to_string()
}

fn sanitize_identifier(input: &str) -> String {
    input
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn slugify_handle(input: &str, fallback: &str) -> String {
    let slug = input
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        fallback.to_string()
    } else {
        slug
    }
}

fn unique_handle<'a>(base: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let existing = existing.collect::<Vec<_>>();
    if !existing.iter().any(|value| *value == base) {
        return base.to_string();
    }

    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !existing.iter().any(|value| *value == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn unique_artifact_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}_{:x}", nanos)
}

fn iso_now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn infer_title(markdown: &str) -> String {
    markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| {
            line.trim_start_matches('#')
                .trim_matches(|ch: char| ch.is_ascii_punctuation() || ch.is_whitespace())
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "Draft".to_string())
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)
        .with_context(|| format!("Failed to write {}", path.display()))
}
