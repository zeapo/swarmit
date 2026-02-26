use ratatui::{layout::Rect, style::Style, text::Span, widgets::Paragraph, Frame};

use crate::app::CrabAnimation;
use crate::theme::Theme;

/// Render the crab parade easter egg overlay.
///
/// Each 🦀 is placed at its `(x, y)` position within `area`.
/// Crabs whose x coordinate would exceed the right edge are skipped
/// (the `update()` step wraps them back to 0 before they go off-screen).
/// If the terminal is too small to be useful, a single crab is rendered centered.
pub fn render(f: &mut Frame, animation: &CrabAnimation, area: Rect, _theme: &Theme) {
    if area.width < 10 {
        // Terminal too small — show a single crab centered.
        let x = area.x + area.width / 2;
        let y = area.y + area.height / 2;
        let crab_rect = Rect {
            x,
            y,
            width: 2.min(area.width),
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Span::styled("🦀", Style::default())),
            crab_rect,
        );
        return;
    }

    for row in &animation.rows {
        let screen_y = area.y + row.y;
        if screen_y >= area.bottom() {
            continue;
        }
        for crab in &row.crabs {
            // x is kept in [0, term_width) by `update()`, but guard against
            // edge cases where f32 precision puts it at exactly term_width.
            let crab_x = (crab.x as u16) % area.width.max(1);
            let screen_x = area.x + crab_x;
            // 🦀 occupies 2 terminal columns — skip if it would overflow the area.
            if screen_x + 2 > area.right() {
                continue;
            }
            let crab_rect = Rect {
                x: screen_x,
                y: screen_y,
                width: 2,
                height: 1,
            };
            f.render_widget(
                Paragraph::new(Span::styled("🦀", Style::default())),
                crab_rect,
            );
        }
    }
}
