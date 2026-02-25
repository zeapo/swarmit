use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use swarmit_core::events::log::read_operations;

use crate::app::App;
use crate::theme::Theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let ops = read_operations(&app.log_path).unwrap_or_default();

    let items: Vec<ListItem> = ops
        .iter()
        .rev()
        .map(|op| {
            // Extract a short operation type name
            let kind_name = format!("{:?}", op.kind);
            let kind_short = kind_name
                .split('{')
                .next()
                .unwrap_or("Unknown")
                .trim()
                .to_string();

            let timestamp = op.timestamp.format("%m-%d %H:%M:%S").to_string();

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<14} ", op.agent),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(kind_short, Theme::normal()),
            ]))
        })
        .collect();

    let total = items.len();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    format!(" Activity Log ({} operations) ", total),
                    Theme::title(),
                ))
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .highlight_style(Theme::selected());

    let mut state = ListState::default();
    if total > 0 {
        let idx = app.selected_index.min(total.saturating_sub(1));
        state.select(Some(idx));
    }

    f.render_stateful_widget(list, area, &mut state);
}
