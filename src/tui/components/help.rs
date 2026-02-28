use ratatui::{
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::theme::Theme;

/// Renders a centered help overlay on top of the current screen.
pub fn render(f: &mut Frame, theme: &Theme, area: Rect) {
    let popup = centered_rect(70, 27, area);

    f.render_widget(Clear, popup);

    let keybindings: &[(&str, &str)] = &[
        ("hjkl / arrows", "Move ↕, switch tabs ↔ (scroll in detail)"),
        ("Enter", "Open + focus detail pane"),
        ("q/Esc/Bksp", "Back — close detail, then quit"),
        ("Shift+HJKL", "Switch focus between panes"),
        ("Space", "Toggle expand/collapse epic"),
        ("= / -", "Grow / shrink focused pane"),
        ("|", "Toggle horizontal/vertical split"),
        ("e", "Edit description in $EDITOR (Description tab)"),
        ("a", "Add comment in $EDITOR (Comments tab)"),
        ("S", "Change task status"),
        ("E", "Change task epic"),
        ("f", "Open filter dialog"),
        ("s", "Open sort dialog"),
        ("n", "Create new task"),
        ("/", "Search (coming soon)"),
        ("r", "Refresh"),
        ("?", "Toggle this help"),
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
        "  Press Esc or ? to close",
        theme.muted_style(),
    )));

    let para = Paragraph::new(lines).alignment(Alignment::Left).block(
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
