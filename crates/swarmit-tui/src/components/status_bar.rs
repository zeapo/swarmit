use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;
use crate::events::Screen;
use crate::theme::Theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let (screen_name, hints) = match &app.screen {
        Screen::Dashboard => (
            "Dashboard",
            "j/k:move  Enter:open  1:dashboard  2:backlog  3:activity  ?:help  q:quit",
        ),
        Screen::Board { .. } => (
            "Board",
            "j/k:move  Enter:detail  c:claim  s:status  Esc:back  ?:help",
        ),
        Screen::TaskDetail { .. } => ("Task Detail", "Esc:back  c:claim  s:status  ?:help"),
        Screen::Activity => ("Activity", "j/k:scroll  Esc:back  ?:help"),
        Screen::Help => ("Help", "Esc:close"),
    };

    let project_name = app
        .state
        .config
        .as_ref()
        .map(|c| c.name.as_str())
        .unwrap_or("swarmit");

    let line = Line::from(vec![
        Span::styled(
            format!(" swarmit | {} | {} ", project_name, screen_name),
            Theme::status_bar().add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(format!(" {} ", hints), Theme::muted()),
    ]);

    let para = Paragraph::new(line).style(Theme::status_bar());
    f.render_widget(para, area);
}
