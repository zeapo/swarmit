use ratatui::style::{Color, Modifier, Style};

pub struct Theme;

impl Theme {
    // ── Brand colors ──────────────────────────────────────────────────────
    pub const PRIMARY: Color = Color::Cyan;
    pub const SECONDARY: Color = Color::Blue;
    pub const SUCCESS: Color = Color::Green;
    pub const WARNING: Color = Color::Yellow;
    pub const ERROR: Color = Color::Red;
    pub const MUTED: Color = Color::DarkGray;

    // ── Status colors ─────────────────────────────────────────────────────
    pub fn status_color(status: &str) -> Color {
        match status {
            "Todo" => Color::Gray,
            "In Progress" => Color::Cyan,
            "Done" => Color::Green,
            "Blocked" => Color::Red,
            "Cancelled" => Color::DarkGray,
            _ => Color::White,
        }
    }

    pub fn priority_color(priority: &str) -> Color {
        match priority {
            "Urgent" => Color::Red,
            "High" => Color::Yellow,
            "Medium" => Color::White,
            "Low" => Color::DarkGray,
            _ => Color::White,
        }
    }

    // ── Common styles ─────────────────────────────────────────────────────
    pub fn title() -> Style {
        Style::default()
            .fg(Self::PRIMARY)
            .add_modifier(Modifier::BOLD)
    }

    pub fn selected() -> Style {
        Style::default()
            .bg(Color::DarkGray)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    }

    pub fn header() -> Style {
        Style::default()
            .fg(Self::SECONDARY)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    pub fn muted() -> Style {
        Style::default().fg(Self::MUTED)
    }

    pub fn normal() -> Style {
        Style::default().fg(Color::White)
    }

    pub fn status_bar() -> Style {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    }

    pub fn help_key() -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }
}
