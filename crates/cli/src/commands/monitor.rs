use super::pr::*;
use super::*;

#[derive(Debug, Clone)]
struct MonitorSnapshot {
    title: String,
    status: String,
    session_path: String,
    left_fields: Vec<(String, String)>,
    right_fields: Vec<(String, String)>,
    content_title: String,
    content_path: String,
    content: String,
    footer: String,
}

#[derive(Debug, Default)]
struct MonitorUiState {
    scroll: u16,
    follow: bool,
    last_content_key: String,
}

impl MonitorUiState {
    fn new() -> Self {
        Self {
            scroll: 0,
            follow: true,
            last_content_key: String::new(),
        }
    }
}

pub(super) fn handle_monitor(cmd: MonitorCommands) -> Result<()> {
    let project_root = resolve_project_root()?;
    match cmd {
        MonitorCommands::Rlcr {
            once,
            interval_secs,
        } => run_monitor_loop(once, interval_secs, || rlcr_monitor_snapshot(&project_root)),
        MonitorCommands::Pr {
            once,
            interval_secs,
        } => run_monitor_loop(once, interval_secs, || pr_monitor_snapshot(&project_root)),
        MonitorCommands::Skill {
            once,
            interval_secs,
        } => run_monitor_loop(once, interval_secs, || {
            skill_monitor_snapshot(&project_root)
        }),
    }
}

fn humanize_cache_base_dir(project_root: &Path) -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{}/.cache", h)))
        .unwrap_or_else(|| ".cache".to_string());
    PathBuf::from(base)
        .join("humanize")
        .join(sanitize_path_component(project_root))
}

fn newest_file_by_mtime(paths: Vec<PathBuf>) -> Option<PathBuf> {
    paths.into_iter().max_by_key(|path| {
        fs::metadata(path)
            .and_then(|meta| meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    })
}

fn read_monitor_content(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|_| format!("Unable to read {}", path.display()))
}

fn kv_block_lines(fields: &[(String, String)]) -> String {
    fields
        .iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_monitor_snapshot_text(snapshot: &MonitorSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&snapshot.title);
    out.push_str("\n\n");
    out.push_str(&format!("Session: {}\n", snapshot.session_path));
    out.push_str(&format!("Status: {}\n", snapshot.status));
    for (key, value) in snapshot
        .left_fields
        .iter()
        .chain(snapshot.right_fields.iter())
    {
        out.push_str(&format!("{key}: {value}\n"));
    }
    out.push_str(&format!(
        "Content: {} ({})\n",
        snapshot.content_title, snapshot.content_path
    ));
    out.push('\n');
    out.push_str(&snapshot.content);
    if !snapshot.content.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn monitor_status_color(status: &str) -> Color {
    match status {
        "active" | "running" | "success" | "complete" | "approve" | "merged" => Color::Green,
        "finalize" | "waiting" => Color::Yellow,
        "cancel" | "closed" | "error" | "timeout" | "maxiter" | "stop" => Color::Red,
        _ => Color::Cyan,
    }
}

fn collect_dir_files(dir: &Path, allowed_suffixes: &[&str]) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && allowed_suffixes.iter().any(|suffix| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| name.ends_with(suffix))
                        .unwrap_or(false)
                })
        })
        .collect()
}

fn rlcr_monitor_snapshot(project_root: &Path) -> Result<MonitorSnapshot> {
    let base_dir = project_root.join(".humanize/rlcr");
    let Some(loop_dir) = newest_session_dir(&base_dir) else {
        return Ok(MonitorSnapshot {
            title: "Humanize RLCR Monitor".to_string(),
            status: "idle".to_string(),
            session_path: "No RLCR sessions found".to_string(),
            left_fields: Vec::new(),
            right_fields: Vec::new(),
            content_title: "No Data".to_string(),
            content_path: "N/A".to_string(),
            content: "No RLCR sessions found.".to_string(),
            footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow"
                .to_string(),
        });
    };
    let state_file = humanize_core::state::resolve_any_state_file(&loop_dir)
        .ok_or_else(|| anyhow::anyhow!("No state file found in latest RLCR session"))?;
    let state = humanize_core::state::State::from_file(&state_file)?;
    let status = state_status_label(&state_file);
    let summary_path = if status == "finalize" || state_file.ends_with("finalize-state.md") {
        loop_dir.join("finalize-summary.md")
    } else {
        loop_dir.join(format!("round-{}-summary.md", state.current_round))
    };
    let summary_preview = fs::read_to_string(&summary_path)
        .map(|content| truncate_one_line(&content, 96))
        .unwrap_or_else(|_| "N/A".to_string());
    let cache_dir = humanize_cache_base_dir(project_root).join(
        loop_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown-loop"),
    );
    let mut candidates = collect_dir_files(
        &loop_dir,
        &[
            "-prompt.md",
            "-summary.md",
            "-review-result.md",
            "-review-prompt.md",
            "finalize-summary.md",
        ],
    );
    candidates.extend(collect_dir_files(&cache_dir, &[".log", ".out", ".cmd"]));
    let content_path = newest_file_by_mtime(candidates).unwrap_or_else(|| summary_path.clone());
    let content = read_monitor_content(&content_path);

    Ok(MonitorSnapshot {
        title: "Humanize RLCR Monitor".to_string(),
        status: status.clone(),
        session_path: loop_dir.display().to_string(),
        left_fields: vec![
            ("State File".to_string(), state_file.display().to_string()),
            (
                "Round".to_string(),
                format!("{} / {}", state.current_round, state.max_iterations),
            ),
            (
                "Plan File".to_string(),
                if state.plan_file.is_empty() {
                    "N/A".to_string()
                } else {
                    state.plan_file.clone()
                },
            ),
            (
                "Summary Preview".to_string(),
                if summary_preview.is_empty() {
                    "N/A".to_string()
                } else {
                    summary_preview
                },
            ),
        ],
        right_fields: vec![
            (
                "Start Branch".to_string(),
                if state.start_branch.is_empty() {
                    "N/A".to_string()
                } else {
                    state.start_branch.clone()
                },
            ),
            (
                "Base Branch".to_string(),
                if state.base_branch.is_empty() {
                    "N/A".to_string()
                } else {
                    state.base_branch.clone()
                },
            ),
            (
                "Model".to_string(),
                format!("{} ({})", state.codex_model, state.codex_effort),
            ),
            ("Timeout".to_string(), format!("{}s", state.codex_timeout)),
            (
                "Ask Codex".to_string(),
                state.ask_codex_question.to_string(),
            ),
            ("Agent Teams".to_string(), state.agent_teams.to_string()),
        ],
        content_title: "Latest RLCR Artifact".to_string(),
        content_path: content_path.display().to_string(),
        content,
        footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow".to_string(),
    })
}

fn pr_monitor_snapshot(project_root: &Path) -> Result<MonitorSnapshot> {
    let base_dir = project_root.join(".humanize/pr-loop");
    let Some(loop_dir) = newest_session_dir(&base_dir) else {
        return Ok(MonitorSnapshot {
            title: "Humanize PR Monitor".to_string(),
            status: "idle".to_string(),
            session_path: "No PR loop sessions found".to_string(),
            left_fields: Vec::new(),
            right_fields: Vec::new(),
            content_title: "No Data".to_string(),
            content_path: "N/A".to_string(),
            content: "No PR loop sessions found.".to_string(),
            footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow"
                .to_string(),
        });
    };
    let state_file = humanize_core::state::resolve_any_state_file(&loop_dir)
        .ok_or_else(|| anyhow::anyhow!("No state file found in latest PR session"))?;
    let state = humanize_core::state::State::from_file(&state_file)?;
    let configured_bots = state.configured_bots.clone().unwrap_or_default();
    let active_bots = state.active_bots.clone().unwrap_or_default();
    let resolve_path = loop_dir.join(format!("round-{}-pr-resolve.md", state.current_round));
    let comment_path = loop_dir.join(format!("round-{}-pr-comment.md", state.current_round + 1));
    let content_path = newest_file_by_mtime(collect_dir_files(
        &loop_dir,
        &[
            "-pr-feedback.md",
            "-pr-check.md",
            "-pr-comment.md",
            "-prompt.md",
            "-pr-resolve.md",
            "goal-tracker.md",
        ],
    ))
    .unwrap_or_else(|| resolve_path.clone());
    let content = read_monitor_content(&content_path);

    Ok(MonitorSnapshot {
        title: "Humanize PR Monitor".to_string(),
        status: state_status_label(&state_file),
        session_path: loop_dir.display().to_string(),
        left_fields: vec![
            ("State File".to_string(), state_file.display().to_string()),
            (
                "PR Number".to_string(),
                state
                    .pr_number
                    .map(|n| format!("#{n}"))
                    .unwrap_or_else(|| "N/A".to_string()),
            ),
            (
                "Branch".to_string(),
                if state.start_branch.is_empty() {
                    "N/A".to_string()
                } else {
                    state.start_branch.clone()
                },
            ),
            (
                "Round".to_string(),
                format!("{} / {}", state.current_round, state.max_iterations),
            ),
            (
                "Configured Bots".to_string(),
                join_list(&configured_bots, "none"),
            ),
            ("Active Bots".to_string(), join_list(&active_bots, "none")),
        ],
        right_fields: vec![
            (
                "Startup Case".to_string(),
                state
                    .startup_case
                    .clone()
                    .unwrap_or_else(|| "N/A".to_string()),
            ),
            (
                "Model".to_string(),
                format!("{} ({})", state.codex_model, state.codex_effort),
            ),
            ("Timeout".to_string(), format!("{}s", state.codex_timeout)),
            (
                "Poll".to_string(),
                format!(
                    "{}s / {}s",
                    state.poll_interval.unwrap_or(0),
                    state.poll_timeout.unwrap_or(0)
                ),
            ),
            (
                "Latest Commit".to_string(),
                state
                    .latest_commit_sha
                    .clone()
                    .unwrap_or_else(|| "N/A".to_string()),
            ),
            (
                "Last Trigger".to_string(),
                state
                    .last_trigger_at
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
            ),
            (
                "Trigger Comment ID".to_string(),
                state
                    .trigger_comment_id
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
            ),
            (
                "Resolve File".to_string(),
                resolve_path.display().to_string(),
            ),
            (
                "Next Comment File".to_string(),
                comment_path.display().to_string(),
            ),
        ],
        content_title: "Latest PR Artifact".to_string(),
        content_path: content_path.display().to_string(),
        content,
        footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow".to_string(),
    })
}

fn skill_monitor_snapshot(project_root: &Path) -> Result<MonitorSnapshot> {
    let skill_dir = project_root.join(".humanize/skill");
    if !skill_dir.is_dir() {
        return Ok(MonitorSnapshot {
            title: "Humanize Skill Monitor".to_string(),
            status: "idle".to_string(),
            session_path: "No ask-codex invocations found".to_string(),
            left_fields: Vec::new(),
            right_fields: Vec::new(),
            content_title: "No Data".to_string(),
            content_path: "N/A".to_string(),
            content: "No ask-codex skill invocations found.".to_string(),
            footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow"
                .to_string(),
        });
    }

    let mut dirs = fs::read_dir(&skill_dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.reverse();
    if dirs.is_empty() {
        return Ok(MonitorSnapshot {
            title: "Humanize Skill Monitor".to_string(),
            status: "idle".to_string(),
            session_path: "No ask-codex invocations found".to_string(),
            left_fields: Vec::new(),
            right_fields: Vec::new(),
            content_title: "No Data".to_string(),
            content_path: "N/A".to_string(),
            content: "No ask-codex skill invocations found.".to_string(),
            footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow"
                .to_string(),
        });
    }

    let total = dirs.len();
    let mut success = 0usize;
    let mut error = 0usize;
    let mut timeout = 0usize;
    let mut empty = 0usize;
    let mut running = 0usize;

    for dir in &dirs {
        let metadata = dir.join("metadata.md");
        if !metadata.exists() {
            running += 1;
            continue;
        }
        let state = fs::read_to_string(&metadata).unwrap_or_default();
        if state.contains("status: success") {
            success += 1;
        } else if state.contains("status: error") {
            error += 1;
        } else if state.contains("status: timeout") {
            timeout += 1;
        } else if state.contains("status: empty_response") {
            empty += 1;
        }
    }

    let latest = &dirs[0];
    let metadata = latest.join("metadata.md");
    let metadata_content = fs::read_to_string(&metadata).unwrap_or_default();
    let status = metadata_content
        .lines()
        .find_map(|line| line.strip_prefix("status: "))
        .unwrap_or(if metadata.exists() {
            "unknown"
        } else {
            "running"
        });
    let model = metadata_content
        .lines()
        .find_map(|line| line.strip_prefix("model: "))
        .unwrap_or("N/A");
    let effort = metadata_content
        .lines()
        .find_map(|line| line.strip_prefix("effort: "))
        .unwrap_or("N/A");
    let question = fs::read_to_string(latest.join("input.md"))
        .map(|content| truncate_one_line(&content, 96))
        .unwrap_or_else(|_| "N/A".to_string());
    let invocation_id = latest
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let cache_dir = humanize_cache_base_dir(project_root).join(format!("skill-{invocation_id}"));
    let watched_file = newest_file_by_mtime({
        let mut files = vec![
            latest.join("output.md"),
            latest.join("input.md"),
            cache_dir.join("codex-run.out"),
            cache_dir.join("codex-run.log"),
            cache_dir.join("codex-run.cmd"),
        ];
        files.retain(|path| path.is_file());
        files
    })
    .unwrap_or_else(|| latest.join("input.md"));
    let content = read_monitor_content(&watched_file);

    Ok(MonitorSnapshot {
        title: "Humanize Skill Monitor".to_string(),
        status: status.to_string(),
        session_path: latest.display().to_string(),
        left_fields: vec![
            ("Total".to_string(), total.to_string()),
            ("Success".to_string(), success.to_string()),
            ("Error".to_string(), error.to_string()),
            ("Timeout".to_string(), timeout.to_string()),
            ("Empty".to_string(), empty.to_string()),
            ("Running".to_string(), running.to_string()),
        ],
        right_fields: vec![
            (
                "Latest Invocation".to_string(),
                latest.display().to_string(),
            ),
            ("Status".to_string(), status.to_string()),
            ("Model".to_string(), format!("{model} ({effort})")),
            ("Question".to_string(), question),
        ],
        content_title: "Invocation Output".to_string(),
        content_path: watched_file.display().to_string(),
        content,
        footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow".to_string(),
    })
}

fn render_monitor_tui(
    snapshot: &MonitorSnapshot,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ui_state: &mut MonitorUiState,
) -> Result<()> {
    let content_key = format!("{}:{}", snapshot.content_path, snapshot.content.len());
    if ui_state.last_content_key != content_key {
        ui_state.last_content_key = content_key;
        if ui_state.follow {
            ui_state.scroll = u16::MAX;
        }
    }

    terminal.draw(|frame| {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(10),
                Constraint::Min(8),
                Constraint::Length(2),
            ])
            .split(area);

        frame.render_widget(Clear, area);

        let header = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    &snapshot.title,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("[{}]", snapshot.status),
                    Style::default()
                        .fg(monitor_status_color(&snapshot.status))
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                &snapshot.session_path,
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title("Session"));
        frame.render_widget(header, chunks[0]);

        let info_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

        let left = Paragraph::new(kv_block_lines(&snapshot.left_fields))
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Details"));
        frame.render_widget(left, info_chunks[0]);

        let right = Paragraph::new(kv_block_lines(&snapshot.right_fields))
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Context"));
        frame.render_widget(right, info_chunks[1]);

        let content_height = chunks[2].height.saturating_sub(2);
        let total_lines = snapshot.content.lines().count() as u16;
        let max_scroll = total_lines.saturating_sub(content_height);
        if ui_state.follow || ui_state.scroll > max_scroll {
            ui_state.scroll = max_scroll;
        }

        let content = Paragraph::new(snapshot.content.as_str())
            .scroll((ui_state.scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(snapshot.content_title.as_str())
                    .title_bottom(Line::from(Span::styled(
                        snapshot.content_path.as_str(),
                        Style::default().fg(Color::DarkGray),
                    ))),
            );
        frame.render_widget(content, chunks[2]);

        let footer = Paragraph::new(snapshot.footer.as_str())
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL).title("Controls"));
        frame.render_widget(footer, chunks[3]);
    })?;
    Ok(())
}

fn run_monitor_loop<F>(once: bool, interval_secs: u64, mut snapshotter: F) -> Result<()>
where
    F: FnMut() -> Result<MonitorSnapshot>,
{
    if once {
        let snapshot = snapshotter()?;
        println!("{}", render_monitor_snapshot_text(&snapshot));
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut ui_state = MonitorUiState::new();
    let mut snapshot = snapshotter()?;
    let tick_rate = Duration::from_secs(interval_secs.max(1));
    let mut last_tick = Instant::now();

    let run_result: Result<()> = (|| loop {
        render_monitor_tui(&snapshot, &mut terminal, &mut ui_state)?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        break Ok(());
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        ui_state.follow = false;
                        ui_state.scroll = ui_state.scroll.saturating_add(1);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        ui_state.follow = false;
                        ui_state.scroll = ui_state.scroll.saturating_sub(1);
                    }
                    KeyCode::PageDown => {
                        ui_state.follow = false;
                        ui_state.scroll = ui_state.scroll.saturating_add(10);
                    }
                    KeyCode::PageUp => {
                        ui_state.follow = false;
                        ui_state.scroll = ui_state.scroll.saturating_sub(10);
                    }
                    KeyCode::Char('g') => {
                        ui_state.follow = false;
                        ui_state.scroll = 0;
                    }
                    KeyCode::Char('G') | KeyCode::End => {
                        ui_state.follow = true;
                        ui_state.scroll = u16::MAX;
                    }
                    KeyCode::Char('f') => {
                        ui_state.follow = !ui_state.follow;
                        if ui_state.follow {
                            ui_state.scroll = u16::MAX;
                        }
                    }
                    KeyCode::Char('r') => {
                        snapshot = snapshotter()?;
                        last_tick = Instant::now();
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            snapshot = snapshotter()?;
            last_tick = Instant::now();
        }
    })();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    run_result
}
