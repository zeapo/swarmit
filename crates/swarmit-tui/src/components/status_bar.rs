use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, DetailTab};
use crate::events::{Focus, Modal, Screen, TaskFormField};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let (screen_name, hints): (String, &str) = if let Some(modal) = &app.modal {
        match modal {
            Modal::QuitConfirm => ("Quit?".to_string(), "y:quit  n/Esc:cancel"),
            Modal::TaskCreate { focused_field, .. } => {
                let hints = if *focused_field == TaskFormField::Description {
                    "Tab:next  Enter:newline  Ctrl+S:save  Esc:cancel"
                } else {
                    "Tab:next  Enter:save  Ctrl+S:save  Esc:cancel"
                };
                ("New Task".to_string(), hints)
            }
            Modal::FilterSelect { .. } => {
                ("Tasks".to_string(), "j/k:move  Enter:select  Esc:cancel")
            }
            Modal::SortSelect { .. } => ("Tasks".to_string(), "j/k:move  Enter:select  Esc:cancel"),
            Modal::StatusSelect { .. } => {
                ("Status".to_string(), "j/k:move  Enter:select  Esc:cancel")
            }
            Modal::EpicSelect { .. } => {
                ("Epic".to_string(), "j/k:move  Enter:select  Esc:cancel")
            }
        }
    } else {
        match &app.screen {
            Screen::Main => {
                let filter_label = match &app.dashboard_filter {
                    None => "All".to_string(),
                    Some(s) => format!("{}", s),
                };
                let sort_label = app.dashboard_sort.label();
                let hints = if !app.detail_open {
                    "j/k:move  l:detail  Space:expand  f:filter  s:sort  n:new  ?:help  q:quit"
                } else if app.focus == Focus::Detail {
                    match app.detail_tab {
                        DetailTab::Description => {
                            "j/k:scroll  e:edit  S:status  E:epic  Tab:next  h:back"
                        }
                        DetailTab::Comments => {
                            "j/k:scroll  a:comment  S:status  E:epic  Tab:next  h:back"
                        }
                        DetailTab::Insights => {
                            "j/k:scroll  S:status  E:epic  Tab:next  h:back"
                        }
                    }
                } else {
                    "j/k:move  l:focus detail  h/Esc:close  Space:expand  f:filter  s:sort  n:new  ?:help  q:quit"
                };
                (format!("[{}] [{}]", filter_label, sort_label), hints)
            }
            Screen::Help => ("Help".to_string(), "Esc/?:close"),
        }
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
            theme.status_bar_style().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {} ", hints), theme.status_bar_hint_style()),
    ]);

    let para = Paragraph::new(line).style(theme.status_bar_style());
    f.render_widget(para, area);
}
