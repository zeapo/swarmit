pub mod app;
pub mod components;
pub mod events;
pub mod theme;

use std::io;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    Terminal,
};

use app::App;
use events::{Action, Modal, Screen};
use theme::Theme;

/// Entry point for the TUI.
pub fn run(project_root: &Path) -> Result<()> {
    // Detect theme BEFORE entering raw mode — terminal-colorsaurus sends OSC
    // escape sequences that require cooked mode for the terminal to respond.
    let theme = Theme::detect();

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Application state
    let mut app = App::new(project_root.to_path_buf(), theme).map_err(|e| {
        // Restore terminal before propagating error
        let _ = restore_terminal();
        e
    })?;

    let result = run_loop(&mut terminal, &mut app);

    // Always restore terminal
    restore_terminal()?;

    result
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        // Draw frame
        terminal.draw(|f| {
            let size = f.area();

            // Reserve bottom row for status bar
            let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(size);

            let main_area = chunks[0];
            let status_area = chunks[1];

            // Render current screen
            match &app.screen {
                Screen::Main => {
                    if app.detail_open {
                        let split = Layout::vertical([
                            Constraint::Percentage(50),
                            Constraint::Percentage(50),
                        ])
                        .split(main_area);
                        components::tree_list::render(f, app, split[0]);
                        components::detail_pane::render(f, app, split[1]);
                    } else {
                        components::tree_list::render(f, app, main_area);
                    }
                }
                Screen::Help => {
                    // Render tree underneath help overlay
                    components::tree_list::render(f, app, main_area);
                    components::help::render(f, &app.theme, main_area);
                }
            }

            // Render modal overlay (if any) on top of the current screen
            if let Some(modal) = &app.modal {
                match modal {
                    Modal::QuitConfirm => components::quit_confirm::render(f, &app.theme, main_area),
                    Modal::TaskCreate { .. } => components::task_create::render(f, app, main_area),
                    Modal::FilterSelect { selected_index } => {
                        components::filter_select::render(f, app, *selected_index, main_area)
                    }
                    Modal::SortSelect { selected_index } => {
                        components::sort_select::render(f, app, *selected_index, main_area)
                    }
                }
            }

            components::status_bar::render(f, app, status_area);
        })?;

        // Poll for keyboard events (100ms timeout = ~10fps)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Ctrl+C always quits immediately (no dialog)
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(());
                }
                // When a modal is open, route all input to the modal handler
                if app.modal.is_some() {
                    app.handle_modal_key(key.code, key.modifiers);
                } else {
                    let action = key_to_action(key.code, key.modifiers);
                    app.handle_action(action);
                }

                if app.should_quit {
                    return Ok(());
                }
            }
        }

        // Poll file watcher for live refresh
        app.poll_log_changes();
    }
}

fn key_to_action(code: KeyCode, _modifiers: KeyModifiers) -> Action {
    match code {
        KeyCode::Char('q') => Action::QuitRequest,
        KeyCode::Char('n') => Action::NewTask,
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Enter => Action::ToggleDetailPane,
        KeyCode::Char(' ') => Action::ToggleCollapse,
        KeyCode::Char('h') => Action::CollapseEpic,
        KeyCode::Char('l') => Action::ExpandEpic,
        KeyCode::Char('f') => Action::OpenFilterDialog,
        KeyCode::Char('s') => Action::OpenSortDialog,
        KeyCode::Esc => Action::Back,
        KeyCode::Char('?') => Action::Help,
        KeyCode::Char('/') => Action::Search,
        KeyCode::Char('r') => Action::Refresh,
        _ => Action::None,
    }
}
