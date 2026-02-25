use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use swarmit_core::models::ItemId;

use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect, task_id: &ItemId) {
    let theme = &app.theme;

    let Some(task) = app.state.tasks.get(task_id) else {
        let msg = Paragraph::new("Task not found").style(theme.muted_style());
        f.render_widget(msg, area);
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Task metadata
            Constraint::Min(5),     // Description + comments
        ])
        .split(area);

    // ── Metadata panel ────────────────────────────────────────────────────
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

    // Relationships
    let rels = app.state.relationships_for(task_id);
    if !rels.is_empty() {
        meta_lines.push(Line::from(""));
        meta_lines.push(Line::from(Span::styled(" Relationships:", theme.header_style())));
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
                .title(Span::styled(" Task Details ", theme.title_style()))
                .border_style(theme.border_style()),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(meta, chunks[0]);

    // ── Description + Comments ───────────────────────────────────────────
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // Description
    let desc_text = task
        .description
        .as_deref()
        .unwrap_or("No description.");
    let desc = Paragraph::new(desc_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Description ", theme.title_style()))
                .border_style(theme.border_style()),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(desc, body_chunks[0]);

    // Comments
    let comments = app.state.comments_for(task_id);
    let comment_lines: Vec<Line> = if comments.is_empty() {
        vec![Line::from(Span::styled("No comments.", theme.muted_style()))]
    } else {
        comments
            .iter()
            .flat_map(|c| {
                vec![
                    Line::from(vec![
                        Span::styled(
                            format!(
                                " @{}  {}",
                                c.author,
                                c.created_at.format("%Y-%m-%d %H:%M")
                            ),
                            Style::default().fg(theme.primary()),
                        ),
                    ]),
                    Line::from(format!("  {}", c.body)),
                    Line::from(""),
                ]
            })
            .collect()
    };

    let comments_widget = Paragraph::new(comment_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    format!(" Comments ({}) ", comments.len()),
                    theme.title_style(),
                ))
                .border_style(theme.border_style()),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(comments_widget, body_chunks[1]);
}
