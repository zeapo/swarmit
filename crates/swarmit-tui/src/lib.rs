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
    layout::{Constraint, Direction, Layout},
    Terminal,
};

use app::App;
use events::{Action, Screen};

/// Entry point for the TUI.
pub fn run(project_root: &Path) -> Result<()> {
    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Application state
    let mut app = App::new(project_root.to_path_buf()).map_err(|e| {
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
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(size);

            let main_area = chunks[0];
            let status_area = chunks[1];

            // Render current screen
            match &app.screen.clone() {
                Screen::Dashboard => {
                    components::dashboard::render(f, app, main_area);
                }
                Screen::Board { epic_id } => {
                    let eid = epic_id.clone();
                    components::board::render(f, app, main_area, &eid);
                }
                Screen::TaskDetail { task_id } => {
                    let tid = task_id.clone();
                    components::task_detail::render(f, app, main_area, &tid);
                }
                Screen::Activity => {
                    components::activity::render(f, app, main_area);
                }
                Screen::Help => {
                    // Render dashboard underneath help overlay
                    components::dashboard::render(f, app, main_area);
                    components::help::render(f, main_area);
                }
            }

            components::status_bar::render(f, app, status_area);
        })?;

        // Poll for keyboard events (100ms timeout = ~10fps)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let action = key_to_action(key.code, key.modifiers, &app.screen);
                app.handle_action(action);

                if app.should_quit {
                    return Ok(());
                }
            }
        }

        // Poll file watcher for live refresh
        app.poll_log_changes();
    }
}

fn key_to_action(code: KeyCode, modifiers: KeyModifiers, screen: &Screen) -> Action {
    // Ctrl+C always quits
    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }

    match code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Enter => Action::Select,
        KeyCode::Esc => {
            if matches!(screen, Screen::Help) {
                Action::Back
            } else {
                Action::Back
            }
        }
        KeyCode::Char('?') => Action::Help,
        KeyCode::Char('/') => Action::Search,
        KeyCode::Char('c') => Action::ClaimTask,
        KeyCode::Char('s') => Action::ChangeStatus,
        KeyCode::Char('1') => Action::GotoDashboard,
        KeyCode::Char('2') => Action::GotoBacklog,
        KeyCode::Char('3') => Action::GotoActivity,
        KeyCode::Char('r') => Action::Refresh,
        _ => Action::None,
    }
}
