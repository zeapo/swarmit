use ratatui::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::theme::Theme;

use super::help::centered_rect;

/// Renders a small centered quit confirmation dialog.
pub fn render(f: &mut Frame, theme: &Theme, area: Rect) {
    let popup = centered_rect(40, 7, area);
    f.render_widget(Clear, popup);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Quit swarmit?", theme.title_style())),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", theme.normal_style()),
            Span::styled("y", Style::default().fg(theme.success())),
            Span::styled(" — yes, quit   ", theme.muted_style()),
            Span::styled("n", Style::default().fg(theme.error())),
            Span::styled(" / Esc — cancel", theme.muted_style()),
        ]),
        Line::from(""),
    ];

    let para = Paragraph::new(lines).alignment(Alignment::Left).block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Quit? ", theme.title_style()))
            .border_style(theme.modal_border_style()),
    );

    f.render_widget(para, popup);
}
