use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use swarmit_core::models::{ItemId, Status};

use crate::app::App;

/// A row in the flattened dashboard tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardRow {
    /// An epic header row (expandable/collapsible).
    Epic { id: ItemId },
    /// A task row (either under an epic or orphan).
    Task { id: ItemId },
}

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Summary header
            Constraint::Min(0),     // Tree view
        ])
        .split(area);

    render_summary(f, app, chunks[0]);
    render_tree(f, app, chunks[1]);
}

fn render_summary(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

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
            Span::styled(format!(" {}", project_name), theme.title_style()),
        ]),
        Line::from(vec![
            Span::styled(
                format!(
                    " {} epics  {}  {} tasks  {}  {} done  {}  {} in progress  {}  {} blocked",
                    epics,
                    Span::styled("│", theme.muted_style()).content,
                    tasks,
                    Span::styled("│", theme.muted_style()).content,
                    done,
                    Span::styled("│", theme.muted_style()).content,
                    in_progress,
                    Span::styled("│", theme.muted_style()).content,
                    blocked
                ),
                theme.muted_style(),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " Enter:open  Space:expand/collapse",
            Style::default().fg(theme.muted_color()).add_modifier(Modifier::ITALIC),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Overview ", theme.title_style()))
        .border_style(theme.border_style());

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, area);
}

fn render_tree(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    // Column widths: indicator+ID, title, status, priority, meta
    let widths = [
        Constraint::Length(14),  // ID (with prefix for indent/indicator)
        Constraint::Min(24),     // Title
        Constraint::Length(13),  // Status
        Constraint::Length(10),  // Priority
        Constraint::Length(16),  // Task count / assignee
    ];

    let header = Row::new(vec!["", "TITLE", "STATUS", "PRIORITY", ""])
        .style(theme.header_style())
        .height(1);

    let rows: Vec<Row> = app
        .dashboard_rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let is_selected = i == app.selected_index;

            match row {
                DashboardRow::Epic { id } => {
                    let epic = match app.state.epics.get(id) {
                        Some(e) => e,
                        None => return Row::new(vec![Cell::from("")]),
                    };
                    let collapsed = app.collapsed_epics.contains(id);
                    let indicator = if collapsed { "▶" } else { "▼" };
                    let task_count = epic.task_ids.len();

                    let id_cell = format!("{} {}", indicator, epic.id);
                    let status_str = epic.status.to_string();
                    let priority_str = epic.priority.to_string();
                    let meta = format!("{} task{}", task_count, if task_count == 1 { "" } else { "s" });

                    let style = if is_selected {
                        theme.selected_style()
                    } else {
                        theme.normal_style().add_modifier(Modifier::BOLD)
                    };

                    Row::new(vec![
                        Cell::from(id_cell),
                        Cell::from(epic.title.clone()),
                        Cell::from(status_str.clone())
                            .style(Style::default().fg(theme.status_color(&status_str))),
                        Cell::from(priority_str.clone())
                            .style(Style::default().fg(theme.priority_color(&priority_str))),
                        Cell::from(meta).style(theme.muted_style()),
                    ])
                    .style(style)
                }

                DashboardRow::Task { id } => {
                    let task = match app.state.tasks.get(id) {
                        Some(t) => t,
                        None => return Row::new(vec![Cell::from("")]),
                    };

                    let id_cell = format!("    {}", task.id);
                    let status_str = task.status.to_string();
                    let priority_str = task.priority.to_string();
                    let assignee = task
                        .assignee
                        .as_ref()
                        .map(|a| format!("@{}", a))
                        .unwrap_or_default();

                    let style = if is_selected {
                        theme.selected_style()
                    } else {
                        theme.normal_style()
                    };

                    Row::new(vec![
                        Cell::from(id_cell),
                        Cell::from(task.title.clone()),
                        Cell::from(status_str.clone())
                            .style(Style::default().fg(theme.status_color(&status_str))),
                        Cell::from(priority_str.clone())
                            .style(Style::default().fg(theme.priority_color(&priority_str))),
                        Cell::from(assignee).style(theme.muted_style()),
                    ])
                    .style(style)
                }

            }
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Dashboard ", theme.title_style()))
                .border_style(theme.border_style()),
        )
        .row_highlight_style(theme.selected_style());

    let mut state = TableState::default();
    if !app.dashboard_rows.is_empty() {
        state.select(Some(app.selected_index));
    }

    f.render_stateful_widget(table, area, &mut state);
}
