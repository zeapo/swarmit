use catppuccin::{Color as CatColor, FlavorColors, PALETTE};
use ratatui::style::{Color, Modifier, Style};

fn cat(c: CatColor) -> Color {
    Color::Rgb(c.rgb.r, c.rgb.g, c.rgb.b)
}

/// Application theme backed by a Catppuccin flavor.
///
/// Detected once at startup (before raw mode) and stored on [`App`].
pub struct Theme {
    colors: &'static FlavorColors,
    /// Name of the matching bat syntax-highlighting theme.
    bat_theme: &'static str,
}

impl Theme {
    /// Detect the appropriate theme flavor.
    ///
    /// Priority:
    /// 1. `SWARMIT_THEME` env var (latte / frappe / macchiato / mocha)
    /// 2. Terminal background luminance via `terminal-colorsaurus`
    /// 3. Default: Mocha (dark)
    pub fn detect() -> Self {
        let (flavor, bat_theme) = if let Ok(name) = std::env::var("SWARMIT_THEME") {
            match name.to_lowercase().as_str() {
                "latte" => (&PALETTE.latte, "Catppuccin Latte"),
                "frappe" => (&PALETTE.frappe, "Catppuccin Frappe"),
                "macchiato" => (&PALETTE.macchiato, "Catppuccin Macchiato"),
                "mocha" => (&PALETTE.mocha, "Catppuccin Mocha"),
                _ => (&PALETTE.mocha, "Catppuccin Mocha"),
            }
        } else {
            match terminal_colorsaurus::theme_mode(terminal_colorsaurus::QueryOptions::default()) {
                Ok(terminal_colorsaurus::ThemeMode::Light) => (&PALETTE.latte, "Catppuccin Latte"),
                _ => (&PALETTE.mocha, "Catppuccin Mocha"),
            }
        };
        Theme {
            colors: &flavor.colors,
            bat_theme,
        }
    }

    // ── Raw color accessors ──────────────────────────────────────────────

    pub fn bat_theme(&self) -> &'static str {
        self.bat_theme
    }

    pub fn text(&self) -> Color {
        cat(self.colors.text)
    }

    pub fn muted_color(&self) -> Color {
        cat(self.colors.overlay0)
    }

    pub fn primary(&self) -> Color {
        cat(self.colors.blue)
    }

    pub fn secondary(&self) -> Color {
        cat(self.colors.sapphire)
    }

    pub fn success(&self) -> Color {
        cat(self.colors.green)
    }

    pub fn warning(&self) -> Color {
        cat(self.colors.yellow)
    }

    pub fn error(&self) -> Color {
        cat(self.colors.red)
    }

    pub fn surface(&self) -> Color {
        cat(self.colors.surface1)
    }

    pub fn mantle(&self) -> Color {
        cat(self.colors.mantle)
    }

    pub fn subtext(&self) -> Color {
        cat(self.colors.subtext0)
    }

    pub fn peach(&self) -> Color {
        cat(self.colors.peach)
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

    pub fn focused_border_style(&self) -> Style {
        Style::default()
            .fg(self.primary())
            .add_modifier(Modifier::BOLD)
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
