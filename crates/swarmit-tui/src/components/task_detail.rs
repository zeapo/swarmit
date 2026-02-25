use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use swarmit_core::models::ItemId;

use crate::app::App;
use crate::theme::Theme;

pub fn render(f: &mut Frame, app: &App, area: Rect, task_id: &ItemId) {
    let Some(task) = app.state.tasks.get(task_id) else {
        let msg = Paragraph::new("Task not found").style(Theme::muted());
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
            Span::styled(" ID:       ", Theme::muted()),
            Span::styled(task.id.to_string(), Theme::title()),
        ]),
        Line::from(vec![
            Span::styled(" Title:    ", Theme::muted()),
            Span::styled(task.title.clone(), Theme::normal()),
        ]),
        Line::from(vec![
            Span::styled(" Status:   ", Theme::muted()),
            Span::styled(
                status_str.clone(),
                Style::default().fg(Theme::status_color(&status_str)),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Priority: ", Theme::muted()),
            Span::styled(
                priority_str.clone(),
                Style::default().fg(Theme::priority_color(&priority_str)),
            ),
        ]),
    ];

    if let Some(epic_id) = &task.epic_id {
        meta_lines.push(Line::from(vec![
            Span::styled(" Epic:     ", Theme::muted()),
            Span::styled(epic_id.to_string(), Theme::normal()),
        ]));
    }

    if let Some(assignee) = &task.assignee {
        meta_lines.push(Line::from(vec![
            Span::styled(" Assignee: ", Theme::muted()),
            Span::styled(
                format!("@{}", assignee),
                Style::default().fg(Color::Cyan),
            ),
        ]));
    }

    // Relationships
    let rels = app.state.relationships_for(task_id);
    if !rels.is_empty() {
        meta_lines.push(Line::from(""));
        meta_lines.push(Line::from(Span::styled(" Relationships:", Theme::header())));
        for r in &rels {
            meta_lines.push(Line::from(vec![
                Span::styled("   ", Theme::normal()),
                Span::styled(r.rel_type.to_string(), Style::default().fg(Color::Yellow)),
                Span::styled(format!(" → {}", r.to), Theme::normal()),
            ]));
        }
    }

    let meta = Paragraph::new(meta_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Task Details ", Theme::title()))
                .border_style(Style::default().fg(Color::DarkGray)),
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
                .title(Span::styled(" Description ", Theme::title()))
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(desc, body_chunks[0]);

    // Comments
    let comments = app.state.comments_for(task_id);
    let comment_lines: Vec<Line> = if comments.is_empty() {
        vec![Line::from(Span::styled("No comments.", Theme::muted()))]
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
                            Style::default().fg(Color::Cyan),
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
                    Theme::title(),
                ))
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(comments_widget, body_chunks[1]);
}
