use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use swarmit_core::models::Status;

use crate::app::App;
use crate::theme::Theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Summary header
            Constraint::Min(0),     // Epic table
        ])
        .split(area);

    render_summary(f, app, chunks[0]);
    render_epic_table(f, app, chunks[1]);
}

fn render_summary(f: &mut Frame, app: &App, area: Rect) {
    let epics = app.state.epics.len();
    let tasks = app.state.tasks.len();
    let done = app.state.tasks_by_status(Status::Done).len();
    let in_progress = app.state.tasks_by_status(Status::InProgress).len();
    let blocked = app.state.tasks_by_status(Status::Blocked).len();

    let project_name = app
        .state
        .config
        .as_ref()
        .map(|c| c.name.as_str())
        .unwrap_or("Unnamed Project");

    let lines = vec![
        Line::from(vec![
            Span::styled(format!(" {}", project_name), Theme::title()),
        ]),
        Line::from(vec![
            Span::styled(
                format!(
                    " {} epics  {}  {} tasks  {}  {} done  {}  {} in progress  {}  {} blocked",
                    epics,
                    Span::styled("│", Theme::muted()).content,
                    tasks,
                    Span::styled("│", Theme::muted()).content,
                    done,
                    Span::styled("│", Theme::muted()).content,
                    in_progress,
                    Span::styled("│", Theme::muted()).content,
                    blocked
                ),
                Theme::muted(),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " Press Enter on an epic to open its board",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Overview ", Theme::title()))
        .border_style(Style::default().fg(Color::DarkGray));

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, area);
}

fn render_epic_table(f: &mut Frame, app: &App, area: Rect) {
    let epics: Vec<_> = app.state.epics.values().collect();

    let header = Row::new(vec!["ID", "STATUS", "PRIORITY", "TASKS", "TITLE"])
        .style(Theme::header())
        .height(1);

    let rows: Vec<Row> = epics
        .iter()
        .enumerate()
        .map(|(i, epic)| {
            let status_str = epic.status.to_string();
            let priority_str = epic.priority.to_string();
            let task_count = epic.task_ids.len().to_string();

            let style = if i == app.selected_index {
                Theme::selected()
            } else {
                Theme::normal()
            };

            Row::new(vec![
                Cell::from(epic.id.to_string()),
                Cell::from(status_str.clone()).style(
                    Style::default().fg(Theme::status_color(&status_str)),
                ),
                Cell::from(priority_str.clone()).style(
                    Style::default().fg(Theme::priority_color(&priority_str)),
                ),
                Cell::from(task_count),
                Cell::from(epic.title.clone()),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(7),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Epics ", Theme::title()))
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .row_highlight_style(Theme::selected());

    let mut state = TableState::default();
    if !epics.is_empty() {
        state.select(Some(app.selected_index));
    }

    f.render_stateful_widget(table, area, &mut state);
}
