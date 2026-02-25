use ratatui::{
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::theme::Theme;

/// Renders a centered help overlay on top of the current screen.
pub fn render(f: &mut Frame, theme: &Theme, area: Rect) {
    // Center a 60×30 popup
    let popup = centered_rect(60, 30, area);

    // Clear the area behind the popup
    f.render_widget(Clear, popup);

    let keybindings: &[(&str, &str)] = &[
        ("j / ↓", "Move down"),
        ("k / ↑", "Move up"),
        ("Enter", "Select / drill into item"),
        ("Space", "Toggle expand/collapse epic (Dashboard)"),
        ("h", "Collapse epic (Dashboard)"),
        ("l", "Expand epic (Dashboard)"),
        ("f", "Open filter dialog (Dashboard)"),
        ("b", "Open global board (Dashboard)"),
        ("Esc", "Go back"),
        ("1", "Jump to Dashboard"),
        ("2", "Jump to Activity"),
        ("n", "Create new task"),
        ("c", "Claim selected task"),
        ("s", "Sort dashboard / Change status"),
        ("/", "Search (coming soon)"),
        ("?", "Toggle this help"),
        ("q", "Quit (with confirmation)"),
    ];

    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Swarmit Keyboard Reference",
            theme.title_style(),
        )),
        Line::from(""),
    ];

    for (key, desc) in keybindings {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<14}", key), theme.help_key_style()),
            Span::styled(format!("  {}", desc), theme.normal_style()),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ── Global Board ──",
        theme.muted_style(),
    )));
    let global_board_keys: &[(&str, &str)] = &[
        ("h / l", "Switch column"),
        ("j / k", "Move up/down within column"),
    ];
    for (key, desc) in global_board_keys {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<14}", key), theme.help_key_style()),
            Span::styled(format!("  {}", desc), theme.normal_style()),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press Esc or ? to close",
        theme.muted_style(),
    )));

    let para = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Help ", theme.title_style()))
                .border_style(theme.modal_border_style()),
        );

    f.render_widget(para, popup);
}

/// Returns a Rect centered in `r` with the given width and height.
pub fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + r.width.saturating_sub(width) / 2;
    let y = r.y + r.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(r.width),
        height: height.min(r.height),
    }
}
