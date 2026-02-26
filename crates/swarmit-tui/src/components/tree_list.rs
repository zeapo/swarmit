use chrono::Utc;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame,
};

use swarmit_core::models::ItemId;

use crate::app::App;

/// A row in the flattened tree list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardRow {
    /// An epic header row (expandable/collapsible).
    Epic { id: ItemId },
    /// A task row (either under an epic or orphan).
    Task { id: ItemId },
}

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    render_tree(f, app, area);
}

fn format_relative(dt: chrono::DateTime<Utc>) -> String {
    let now = Utc::now();
    let dur = now.signed_duration_since(dt);
    if dur.num_minutes() < 1 {
        "just now".to_string()
    } else if dur.num_minutes() < 60 {
        format!("{}m ago", dur.num_minutes())
    } else if dur.num_hours() < 24 {
        format!("{}h ago", dur.num_hours())
    } else {
        format!("{}d ago", dur.num_days())
    }
}

fn render_tree(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    // Column widths: indicator+ID, title, status, priority, updated, meta
    let widths = [
        Constraint::Length(14), // ID (with prefix for indent/indicator)
        Constraint::Min(24),    // Title
        Constraint::Length(13), // Status
        Constraint::Length(10), // Priority
        Constraint::Length(12), // Updated
        Constraint::Length(16), // Task count / assignee
    ];

    let header = Row::new(vec!["", "TITLE", "STATUS", "PRIORITY", "UPDATED", ""])
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
                        Cell::from(""), // no updated_at for epics
                        Cell::from(meta).style(theme.muted_style()),
                    ])
                    .style(style)
                }

                DashboardRow::Task { id } => {
                    let task = match app.state.tasks.get(id) {
                        Some(t) => t,
                        None => return Row::new(vec![Cell::from("")]),
                    };

                    let indent = if task.epic_id.is_some() { "    " } else { "  " };
                    let id_cell = format!("{}{}", indent, task.id);
                    let status_str = task.status.to_string();
                    let priority_str = task.priority.to_string();
                    let assignee = task
                        .assignee
                        .as_ref()
                        .map(|a| format!("@{}", a))
                        .unwrap_or_default();

                    let style = if is_selected {
                        theme.selected_style()
                    } else if task.epic_id.is_none() {
                        theme.normal_style().add_modifier(Modifier::BOLD)
                    } else {
                        theme.normal_style()
                    };

                    let updated = format_relative(task.updated_at);

                    Row::new(vec![
                        Cell::from(id_cell),
                        Cell::from(task.title.clone()),
                        Cell::from(status_str.clone())
                            .style(Style::default().fg(theme.status_color(&status_str))),
                        Cell::from(priority_str.clone())
                            .style(Style::default().fg(theme.priority_color(&priority_str))),
                        Cell::from(updated).style(theme.muted_style()),
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
                .title(Span::styled(" Tasks ", theme.title_style()))
                .border_style(theme.border_style()),
        )
        .row_highlight_style(theme.selected_style());

    let mut state = TableState::default();
    if !app.dashboard_rows.is_empty() {
        state.select(Some(app.selected_index));
    }

    f.render_stateful_widget(table, area, &mut state);
}
