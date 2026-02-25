use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use swarmit_core::models::{ItemId, Status};

use crate::app::App;

/// Column definitions for the global board swimlane view.
const COLUMNS: &[(Status, &str, usize)] = &[
    (Status::Todo, "Todo", 0),
    (Status::InProgress, "In Progress", 1),
    (Status::Done, "Done", 2),
    (Status::Blocked, "Blocked", 3),
];

/// Renders the global board view: all tasks across all epics, organised as
/// swimlane rows (one per epic, plus "No Epic" for orphans) and kanban
/// columns (Todo / In Progress / Done / Blocked).
///
/// Selection model (set by TASK-028):
///   - `app.selected_column` -- which kanban column (0-3) is active.
///   - `app.selected_index`  -- position within that column across all swimlanes.
///
/// NOTE: `Screen::GlobalBoard` and `app.selected_column` are introduced by
/// the parallel TASK-028 branch and will not compile until that branch is
/// merged.  Write the code correctly here; compilation resolves after merge.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    // Split into 4 equal horizontal kanban columns.
    let col_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);

    // Build the ordered list of swimlanes.
    //
    // Swimlane 0: "No Epic" (tasks with epic_id == None)
    // Swimlanes 1..: one per epic in BTreeMap order (deterministic).

    // Collect orphan tasks once.
    let orphan_tasks: Vec<_> = app
        .state
        .tasks
        .values()
        .filter(|t| t.epic_id.is_none())
        .collect();

    // Build a Vec of (Option<&ItemId>, label) for each swimlane.
    // We use None for the "No Epic" lane and Some(epic_id) for real epics.
    // Only include swimlanes that have at least one task.
    let mut swimlanes: Vec<(Option<&ItemId>, String)> = Vec::new();

    if !orphan_tasks.is_empty() {
        swimlanes.push((None, "No Epic".to_string()));
    }

    for (epic_id, epic) in &app.state.epics {
        let has_tasks = app
            .state
            .tasks
            .values()
            .any(|t| t.epic_id.as_ref() == Some(epic_id));
        if has_tasks {
            swimlanes.push((Some(epic_id), format!("{} — {}", epic.id, epic.title)));
        }
    }

    // Count all tasks per column for the header labels.
    let mut col_task_counts = [0usize; 4];
    for (status, _, col_idx) in COLUMNS {
        col_task_counts[*col_idx] = app
            .state
            .tasks
            .values()
            .filter(|t| &t.status == status)
            .count();
    }

    // Render each column.
    for (status, label, col_idx) in COLUMNS {
        let col_area = col_areas[*col_idx];
        let col_color = theme.status_color(&status.to_string());

        // Column header with status label and total task count.
        let header_title = format!(" {} ({}) ", label, col_task_counts[*col_idx]);

        // Build lines: for each swimlane emit a divider row then task cards.
        let mut lines: Vec<Line> = Vec::new();

        // Track the per-column position across swimlanes (for selection).
        let mut col_position: usize = 0;

        for (epic_id_opt, lane_label) in &swimlanes {
            // Swimlane divider row -- bold, styled with primary color.
            lines.push(Line::from(Span::styled(
                format!(" {} ", lane_label),
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            )));

            // Tasks for this swimlane + column.
            let lane_col_tasks: Vec<_> = match epic_id_opt {
                None => orphan_tasks
                    .iter()
                    .copied()
                    .filter(|t| &t.status == status)
                    .collect(),
                Some(eid) => app
                    .state
                    .tasks
                    .values()
                    .filter(|t| t.epic_id.as_ref() == Some(*eid) && &t.status == status)
                    .collect(),
            };

            if lane_col_tasks.is_empty() {
                lines.push(Line::from(Span::styled("   (none)", theme.muted_style())));
            } else {
                for task in &lane_col_tasks {
                    // A task is selected when the column matches and its global
                    // position within this column equals selected_index.
                    let is_selected = *col_idx == app.selected_column
                        && col_position == app.selected_index;

                    // Build task card: " ID: title" truncated to column width.
                    let id_str = task.id.to_string();
                    let prefix = format!(" {}: ", id_str);
                    // Available width for the title: column width minus borders (2) and prefix.
                    let avail = (col_area.width as usize)
                        .saturating_sub(prefix.len() + 2);
                    let title = if task.title.len() > avail && avail > 1 {
                        format!("{}…", &task.title[..avail.saturating_sub(1)])
                    } else {
                        task.title.clone()
                    };

                    let card_text = format!("{}{}", prefix, title);

                    let style = if is_selected {
                        theme.selected_style()
                    } else {
                        theme.normal_style()
                    };

                    lines.push(Line::from(Span::styled(card_text, style)));
                    col_position += 1;
                }
            }
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(header_title, Style::default().fg(col_color)))
            .border_style(theme.border_style());

        let para = Paragraph::new(lines).block(block);
        f.render_widget(para, col_area);
    }
}
