use catppuccin::{FlavorColors, PALETTE};
use ratatui::style::{Color, Modifier, Style};

/// Application theme backed by a Catppuccin flavor.
///
/// Detected once at startup (before raw mode) and stored on [`App`].
pub struct Theme {
    colors: &'static FlavorColors,
}

impl Theme {
    /// Detect the appropriate theme flavor.
    ///
    /// Priority:
    /// 1. `SWARMIT_THEME` env var (latte / frappe / macchiato / mocha)
    /// 2. Terminal background luminance via `terminal-colorsaurus`
    /// 3. Default: Mocha (dark)
    pub fn detect() -> Self {
        let flavor = if let Ok(name) = std::env::var("SWARMIT_THEME") {
            match name.to_lowercase().as_str() {
                "latte" => &PALETTE.latte,
                "frappe" => &PALETTE.frappe,
                "macchiato" => &PALETTE.macchiato,
                "mocha" => &PALETTE.mocha,
                _ => &PALETTE.mocha,
            }
        } else {
            match terminal_colorsaurus::theme_mode(terminal_colorsaurus::QueryOptions::default()) {
                Ok(terminal_colorsaurus::ThemeMode::Light) => &PALETTE.latte,
                _ => &PALETTE.mocha,
            }
        };
        Theme { colors: &flavor.colors }
    }

    // ── Raw color accessors ──────────────────────────────────────────────

    pub fn text(&self) -> Color {
        self.colors.text.into()
    }

    pub fn muted_color(&self) -> Color {
        self.colors.overlay0.into()
    }

    pub fn primary(&self) -> Color {
        self.colors.blue.into()
    }

    pub fn secondary(&self) -> Color {
        self.colors.sapphire.into()
    }

    pub fn success(&self) -> Color {
        self.colors.green.into()
    }

    pub fn warning(&self) -> Color {
        self.colors.yellow.into()
    }

    pub fn error(&self) -> Color {
        self.colors.red.into()
    }

    pub fn surface(&self) -> Color {
        self.colors.surface1.into()
    }

    pub fn mantle(&self) -> Color {
        self.colors.mantle.into()
    }

    pub fn subtext(&self) -> Color {
        self.colors.subtext0.into()
    }

    pub fn peach(&self) -> Color {
        self.colors.peach.into()
    }

    // ── Style builders ───────────────────────────────────────────────────

    pub fn title_style(&self) -> Style {
        Style::default()
            .fg(self.primary())
            .add_modifier(Modifier::BOLD)
    }

    pub fn selected_style(&self) -> Style {
        Style::default()
            .bg(self.surface())
            .fg(self.text())
            .add_modifier(Modifier::BOLD)
    }

    pub fn header_style(&self) -> Style {
        Style::default()
            .fg(self.secondary())
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    pub fn muted_style(&self) -> Style {
        Style::default().fg(self.muted_color())
    }

    pub fn normal_style(&self) -> Style {
        Style::default().fg(self.text())
    }

    pub fn status_bar_style(&self) -> Style {
        Style::default().bg(self.mantle()).fg(self.text())
    }

    pub fn status_bar_hint_style(&self) -> Style {
        Style::default().fg(self.text())
    }

    pub fn help_key_style(&self) -> Style {
        Style::default()
            .fg(self.warning())
            .add_modifier(Modifier::BOLD)
    }

    pub fn border_style(&self) -> Style {
        Style::default().fg(self.surface())
    }

    pub fn modal_border_style(&self) -> Style {
        Style::default().fg(self.primary())
    }

    pub fn focus_style(&self) -> Style {
        Style::default()
            .fg(self.warning())
            .add_modifier(Modifier::BOLD)
    }

    pub fn unfocused_label_style(&self) -> Style {
        Style::default().fg(self.subtext())
    }

    // ── Parameterized color lookups ──────────────────────────────────────

    pub fn status_color(&self, status: &str) -> Color {
        match status {
            "Todo" => self.subtext(),
            "In Progress" => self.primary(),
            "Done" => self.success(),
            "Blocked" => self.error(),
            "Cancelled" => self.muted_color(),
            _ => self.text(),
        }
    }

    pub fn priority_color(&self, priority: &str) -> Color {
        match priority {
            "Urgent" => self.error(),
            "High" => self.peach(),
            "Medium" => self.text(),
            "Low" => self.subtext(),
            _ => self.text(),
        }
    }
}
