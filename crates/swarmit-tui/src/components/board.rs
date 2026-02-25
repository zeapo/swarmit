use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame,
};

use swarmit_core::models::{ItemId, Status};

use crate::app::App;
use crate::theme::Theme;

pub fn render(f: &mut Frame, app: &App, area: Rect, epic_id: &ItemId) {
    // Split into kanban columns: Todo | In Progress | Done | Blocked
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);

    let statuses = [
        (Status::Todo, "Todo", 0),
        (Status::InProgress, "In Progress", 1),
        (Status::Done, "Done", 2),
        (Status::Blocked, "Blocked", 3),
    ];

    // Collect all tasks for this epic, ordered by status
    let all_tasks: Vec<_> = app
        .state
        .tasks
        .values()
        .filter(|t| t.epic_id.as_ref() == Some(epic_id))
        .collect();

    // Compute global selected index across all tasks
    let all_task_ids: Vec<_> = all_tasks.iter().map(|t| t.id.clone()).collect();

    for (status, label, col_idx) in &statuses {
        let col_tasks: Vec<_> = all_tasks.iter().filter(|t| &t.status == status).collect();

        let header = Row::new(vec!["ID", "TITLE"])
            .style(Theme::header())
            .height(1);

        let rows: Vec<Row> = col_tasks
            .iter()
            .map(|task| {
                // Check if this task is globally selected
                let global_pos = all_task_ids
                    .iter()
                    .position(|id| id == &task.id)
                    .unwrap_or(usize::MAX);
                let is_selected = global_pos == app.selected_index;

                let style = if is_selected {
                    Theme::selected()
                } else {
                    Theme::normal()
                };

                let assignee = task
                    .assignee
                    .as_ref()
                    .map(|a| format!(" @{}", a))
                    .unwrap_or_default();

                Row::new(vec![
                    Cell::from(task.id.to_string()),
                    Cell::from(format!("{}{}", task.title, assignee)),
                ])
                .style(style)
            })
            .collect();

        let col_color = Theme::status_color(&status.to_string());
        let title = format!(" {} ({}) ", label, col_tasks.len());

        let widths = [Constraint::Length(10), Constraint::Min(10)];

        let table = Table::new(rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(title, Style::default().fg(col_color)))
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .row_highlight_style(Theme::selected());

        let mut table_state = TableState::default();
        f.render_stateful_widget(table, columns[*col_idx], &mut table_state);
    }
}
