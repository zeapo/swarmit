use bat::assets::HighlightingAssets;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use syntect::easy::HighlightLines;
use syntect::util::LinesWithEndings;

use swarmit_core::models::ItemId;

use crate::app::App;
use crate::components::tree_list::DashboardRow;
use crate::events::Focus;

// Bat's bundled highlighting assets, loaded once per thread on first use.
// `HighlightingAssets` uses `unsync::OnceCell` internally so it isn't `Sync`;
// `thread_local!` avoids that requirement.
thread_local! {
    static ASSETS: HighlightingAssets = HighlightingAssets::from_binary();
}

/// Force-load the syntax highlighting assets on the calling thread.
///
/// `HighlightingAssets` uses an internal `OnceCell`; calling this at startup
/// moves the ~989 KB bincode deserialization cost to a predictable moment
/// rather than the first time a task detail is opened.
pub fn warm_up_syntax() {
    let _guard = crate::prof_guard!("warm_up_syntax");
    ASSETS.with(|a| {
        let _ = a.get_syntax_set();
    });
}

/// Render `content` as syntax-highlighted markdown using bat's bundled
/// syntax definitions and themes. Falls back to plain text on any error.
pub(crate) fn highlight_markdown(content: &str, bat_theme: &str) -> Text<'static> {
    let _guard = crate::prof_guard!("highlight_markdown");
    ASSETS.with(|assets| {
        let ss = match assets.get_syntax_set() {
            Ok(ss) => ss,
            Err(_) => return Text::from(content.to_owned()),
        };
        let theme = assets.get_theme(bat_theme);

        let syntax = ss
            .find_syntax_by_extension("md")
            .unwrap_or_else(|| ss.find_syntax_plain_text());

        let mut h = HighlightLines::new(syntax, theme);
        let mut lines: Vec<Line<'static>> = Vec::new();

        for line_str in LinesWithEndings::from(content) {
            match h.highlight_line(line_str, ss) {
                Ok(ranges) => {
                    let spans: Vec<Span<'static>> = ranges
                        .iter()
                        .map(|(style, text)| {
                            let owned = text
                                .trim_end_matches(|c: char| c == '\n' || c == '\r')
                                .to_owned();
                            let fg = Color::Rgb(
                                style.foreground.r,
                                style.foreground.g,
                                style.foreground.b,
                            );
                            Span::styled(owned, Style::default().fg(fg))
                        })
                        .filter(|s| !s.content.is_empty())
                        .collect();
                    lines.push(Line::from(spans));
                }
                Err(_) => lines.push(Line::from(
                    line_str
                        .trim_end_matches(|c: char| c == '\n' || c == '\r')
                        .to_owned(),
                )),
            }
        }

        Text::from(lines)
    })
}

/// Renders the 1-row context breadcrumb above the detail pane.
/// Shows the path to the selected item: "EPIC-001 › TASK-003 · Title" or "EPIC-001 · Title".
pub fn render_breadcrumb(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let text = match app.dashboard_rows.get(app.selected_index) {
        None => "  No selection".to_string(),
        Some(DashboardRow::Task { id }) => {
            let Some(task) = app.state.tasks.get(id) else {
                return;
            };
            if let Some(epic_id) = &task.epic_id {
                format!("  {} › {} · {}", epic_id, task.id, task.title)
            } else {
                format!("  {} · {}", task.id, task.title)
            }
        }
        Some(DashboardRow::Epic { id }) => {
            let Some(epic) = app.state.epics.get(id) else {
                return;
            };
            format!("  {} · {}", epic.id, epic.title)
        }
    };

    let line = Line::from(Span::styled(text, theme.header_style()));
    let para = Paragraph::new(line).style(theme.header_style());
    f.render_widget(para, area);
}

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let _guard = crate::prof_guard!("detail_pane::render");
    let theme = &app.theme;

    let selected = app.dashboard_rows.get(app.selected_index);

    match selected {
        None => {
            let border = if app.focus == Focus::Detail {
                theme.focused_border_style()
            } else {
                theme.border_style()
            };
            let msg = Paragraph::new(Span::styled("No item selected.", theme.muted_style()))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border),
                );
            f.render_widget(msg, area);
        }
        Some(DashboardRow::Task { id }) => render_task(f, app, area, id),
        Some(DashboardRow::Epic { id }) => render_epic(f, app, area, id),
    }
}

fn render_task(f: &mut Frame, app: &App, area: Rect, task_id: &ItemId) {
    let _guard = crate::prof_guard!("detail_pane::render_task");
    let theme = &app.theme;
    let border = if app.focus == Focus::Detail {
        theme.focused_border_style()
    } else {
        theme.border_style()
    };

    let Some(task) = app.state.tasks.get(task_id) else {
        let msg = Paragraph::new("Task not found").style(theme.muted_style());
        f.render_widget(msg, area);
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(3)])
        .split(area);

    // ── Metadata ─────────────────────────────────────────────────────────
    let status_str = task.status.to_string();
    let priority_str = task.priority.to_string();

    let mut meta_lines = vec![
        Line::from(vec![
            Span::styled(" ID:       ", theme.muted_style()),
            Span::styled(task.id.to_string(), theme.title_style()),
        ]),
        Line::from(vec![
            Span::styled(" Title:    ", theme.muted_style()),
            Span::styled(task.title.clone(), theme.normal_style()),
        ]),
        Line::from(vec![
            Span::styled(" Status:   ", theme.muted_style()),
            Span::styled(
                status_str.clone(),
                Style::default().fg(theme.status_color(&status_str)),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Priority: ", theme.muted_style()),
            Span::styled(
                priority_str.clone(),
                Style::default().fg(theme.priority_color(&priority_str)),
            ),
        ]),
    ];

    if let Some(epic_id) = &task.epic_id {
        meta_lines.push(Line::from(vec![
            Span::styled(" Epic:     ", theme.muted_style()),
            Span::styled(epic_id.to_string(), theme.normal_style()),
        ]));
    }

    if let Some(assignee) = &task.assignee {
        meta_lines.push(Line::from(vec![
            Span::styled(" Assignee: ", theme.muted_style()),
            Span::styled(
                format!("@{}", assignee),
                Style::default().fg(theme.primary()),
            ),
        ]));
    }

    let rels = app.state.relationships_for(task_id);
    if !rels.is_empty() {
        meta_lines.push(Line::from(""));
        for r in &rels {
            meta_lines.push(Line::from(vec![
                Span::styled("   ", theme.normal_style()),
                Span::styled(r.rel_type.to_string(), Style::default().fg(theme.warning())),
                Span::styled(format!(" → {}", r.to), theme.normal_style()),
            ]));
        }
    }

    let meta = Paragraph::new(meta_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(format!(" {} ", task.id), theme.title_style()))
                .border_style(border),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(meta, chunks[0]);

    // ── Description (full width, scrollable, bat-highlighted markdown) ───
    // Prefer the pre-computed cache to avoid per-frame re-highlighting.
    // Only use the cache when it holds an actual description (Some(_)); if
    // the description is None the "No description." placeholder is rendered
    // via the fallback path below.
    let desc_content: Text<'static> = if let Some((_, Some(_), ref cached)) = app.highlight_cache {
        cached.clone()
    } else if let Some(raw) = task.description.as_deref() {
        Text::from(raw.to_owned())
    } else {
        Text::from(Line::from(Span::styled(
            "No description.",
            theme.muted_style(),
        )))
    };
    let desc = Paragraph::new(desc_content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Description ", theme.title_style()))
                .border_style(border),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll as u16, 0));
    f.render_widget(desc, chunks[1]);
}

fn render_epic(f: &mut Frame, app: &App, area: Rect, epic_id: &ItemId) {
    let _guard = crate::prof_guard!("detail_pane::render_epic");
    let theme = &app.theme;
    let border = if app.focus == Focus::Detail {
        theme.focused_border_style()
    } else {
        theme.border_style()
    };

    let Some(epic) = app.state.epics.get(epic_id) else {
        let msg = Paragraph::new("Epic not found").style(theme.muted_style());
        f.render_widget(msg, area);
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(3)])
        .split(area);

    // ── Metadata ─────────────────────────────────────────────────────────
    let status_str = epic.status.to_string();
    let priority_str = epic.priority.to_string();

    let mut meta_lines = vec![
        Line::from(vec![
            Span::styled(" ID:       ", theme.muted_style()),
            Span::styled(epic.id.to_string(), theme.title_style()),
        ]),
        Line::from(vec![
            Span::styled(" Title:    ", theme.muted_style()),
            Span::styled(epic.title.clone(), theme.normal_style()),
        ]),
        Line::from(vec![
            Span::styled(" Status:   ", theme.muted_style()),
            Span::styled(
                status_str.clone(),
                Style::default().fg(theme.status_color(&status_str)),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Priority: ", theme.muted_style()),
            Span::styled(
                priority_str.clone(),
                Style::default().fg(theme.priority_color(&priority_str)),
            ),
        ]),
    ];

    if let Some(assignee) = &epic.assignee {
        meta_lines.push(Line::from(vec![
            Span::styled(" Assignee: ", theme.muted_style()),
            Span::styled(
                format!("@{}", assignee),
                Style::default().fg(theme.primary()),
            ),
        ]));
    }

    let meta = Paragraph::new(meta_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(format!(" {} ", epic.id), theme.title_style()))
                .border_style(border),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(meta, chunks[0]);

    // ── Task list (scrollable) ────────────────────────────────────────────
    let task_lines: Vec<Line> = if epic.task_ids.is_empty() {
        vec![Line::from(Span::styled("No tasks.", theme.muted_style()))]
    } else {
        epic.task_ids
            .iter()
            .filter_map(|tid| {
                let task = app.state.tasks.get(tid)?;
                let status_str = task.status.to_string();
                Some(Line::from(vec![
                    Span::styled(format!("  {}", task.id), theme.muted_style()),
                    Span::styled("  ", theme.normal_style()),
                    Span::styled(task.title.clone(), theme.normal_style()),
                    Span::styled("  ", theme.normal_style()),
                    Span::styled(
                        status_str.clone(),
                        Style::default().fg(theme.status_color(&status_str)),
                    ),
                ]))
            })
            .collect()
    };

    let tasks_widget = Paragraph::new(task_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    format!(" Tasks ({}) ", epic.task_ids.len()),
                    theme.title_style(),
                ))
                .border_style(border),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll as u16, 0));
    f.render_widget(tasks_widget, chunks[1]);
}
