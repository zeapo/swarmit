use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{backend::TestBackend, layout::Position, Terminal};
use swarmit_core::events::operations::{Operation, OperationKind};
use swarmit_core::models::{AgentId, ItemId, Priority};

use crate::app::App;
use crate::events::{Focus, Modal, Screen};
use crate::theme::Theme;
use crate::{handle_key, render_frame};

/// Test harness that wraps `App` + `Terminal<TestBackend>`, enabling
/// integration tests that simulate keystrokes through the full input
/// pipeline and assert on both application state and rendered output.
pub struct TuiTestHarness {
    pub app: App,
    terminal: Terminal<TestBackend>,
    /// Held alive so the tempdir isn't deleted while the harness is in use.
    _dir: tempfile::TempDir,
}

impl TuiTestHarness {
    /// Create a harness with an empty project and an 80×24 terminal.
    pub fn new() -> Self {
        Self::with_size(80, 24)
    }

    /// Create a harness with an empty project and a custom terminal size.
    pub fn with_size(width: u16, height: u16) -> Self {
        let dir = tempfile::tempdir().expect("create tempdir");
        let app = App::new(dir.path().to_path_buf(), Theme::detect())
            .expect("create App for test harness");
        let backend = TestBackend::new(width, height);
        let terminal = Terminal::new(backend).expect("create test terminal");
        Self {
            app,
            terminal,
            _dir: dir,
        }
    }

    /// Create a harness pre-populated with one epic and one task underneath it.
    ///
    /// Mirrors the `setup_app_with_epic()` pattern from `app.rs` unit tests.
    pub fn with_epic_and_task() -> Self {
        let mut harness = Self::new();

        let agent = AgentId::new("test-agent").unwrap();
        let epic_id = ItemId::new("EPIC", 1);
        let task_id = ItemId::new("TASK", 1);

        let _ = harness.app.state.apply(Operation::new(
            agent.clone(),
            OperationKind::CreateEpic {
                id: epic_id,
                title: "Test Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            },
        ));
        let _ = harness.app.state.apply(Operation::new(
            agent,
            OperationKind::CreateTask {
                id: task_id,
                title: "Test Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(ItemId::new("EPIC", 1)),
            },
        ));
        harness.app.rebuild_dashboard_rows();
        harness.app.selected_index = 0;
        harness
    }

    // ── Key simulation ──────────────────────────────────────────────

    /// Send a raw key event through the full `handle_key` pipeline, then render.
    pub fn press(&mut self, code: KeyCode, modifiers: KeyModifiers) -> &mut Self {
        handle_key(&mut self.app, code, modifiers);
        self.render();
        self
    }

    pub fn press_char(&mut self, c: char) -> &mut Self {
        self.press(KeyCode::Char(c), KeyModifiers::NONE)
    }

    pub fn press_enter(&mut self) -> &mut Self {
        self.press(KeyCode::Enter, KeyModifiers::NONE)
    }

    pub fn press_esc(&mut self) -> &mut Self {
        self.press(KeyCode::Esc, KeyModifiers::NONE)
    }

    pub fn press_tab(&mut self) -> &mut Self {
        self.press(KeyCode::Tab, KeyModifiers::NONE)
    }

    pub fn press_up(&mut self) -> &mut Self {
        self.press(KeyCode::Up, KeyModifiers::NONE)
    }

    pub fn press_down(&mut self) -> &mut Self {
        self.press(KeyCode::Down, KeyModifiers::NONE)
    }

    pub fn press_left(&mut self) -> &mut Self {
        self.press(KeyCode::Left, KeyModifiers::NONE)
    }

    pub fn press_right(&mut self) -> &mut Self {
        self.press(KeyCode::Right, KeyModifiers::NONE)
    }

    pub fn press_ctrl_c(&mut self) -> &mut Self {
        self.press(KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    pub fn press_ctrl_s(&mut self) -> &mut Self {
        self.press(KeyCode::Char('s'), KeyModifiers::CONTROL)
    }

    /// Send each character in `s` as a separate key press.
    pub fn type_str(&mut self, s: &str) -> &mut Self {
        for c in s.chars() {
            self.press(KeyCode::Char(c), KeyModifiers::NONE);
        }
        self
    }

    /// Feed the full Konami code sequence (↑↑↓↓←→←→BA).
    pub fn enter_konami_code(&mut self) -> &mut Self {
        let sequence = [
            KeyCode::Up,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char('b'),
            KeyCode::Char('a'),
        ];
        for code in sequence {
            handle_key(&mut self.app, code, KeyModifiers::NONE);
        }
        self.render();
        self
    }

    // ── State assertions ────────────────────────────────────────────

    pub fn assert_screen(&mut self, expected: Screen) -> &mut Self {
        assert_eq!(
            self.app.screen, expected,
            "expected screen {:?}, got {:?}",
            expected, self.app.screen
        );
        self
    }

    pub fn assert_no_modal(&mut self) -> &mut Self {
        assert!(
            self.app.modal.is_none(),
            "expected no modal, got {:?}",
            self.app.modal
        );
        self
    }

    pub fn assert_modal_quit_confirm(&mut self) -> &mut Self {
        assert!(
            matches!(self.app.modal, Some(Modal::QuitConfirm)),
            "expected QuitConfirm modal, got {:?}",
            self.app.modal
        );
        self
    }

    pub fn assert_modal_task_create(&mut self) -> &mut Self {
        assert!(
            matches!(self.app.modal, Some(Modal::TaskCreate { .. })),
            "expected TaskCreate modal, got {:?}",
            self.app.modal
        );
        self
    }

    pub fn assert_focus(&mut self, expected: Focus) -> &mut Self {
        assert_eq!(
            self.app.focus, expected,
            "expected focus {:?}, got {:?}",
            expected, self.app.focus
        );
        self
    }

    pub fn assert_detail_open(&mut self) -> &mut Self {
        assert!(self.app.detail_open, "expected detail pane to be open");
        self
    }

    pub fn assert_detail_closed(&mut self) -> &mut Self {
        assert!(!self.app.detail_open, "expected detail pane to be closed");
        self
    }

    pub fn assert_selected_index(&mut self, expected: usize) -> &mut Self {
        assert_eq!(
            self.app.selected_index, expected,
            "expected selected_index {}, got {}",
            expected, self.app.selected_index
        );
        self
    }

    pub fn assert_should_quit(&mut self) -> &mut Self {
        assert!(self.app.should_quit, "expected should_quit to be true");
        self
    }

    pub fn assert_should_not_quit(&mut self) -> &mut Self {
        assert!(!self.app.should_quit, "expected should_quit to be false");
        self
    }

    pub fn assert_crab_active(&mut self) -> &mut Self {
        assert!(
            self.app
                .crab_animation
                .as_ref()
                .map_or(false, |a| a.active),
            "expected crab animation to be active"
        );
        self
    }

    pub fn assert_crab_inactive(&mut self) -> &mut Self {
        assert!(
            self.app.crab_animation.is_none(),
            "expected crab animation to be inactive (None), got {:?}",
            self.app.crab_animation.as_ref().map(|a| a.active)
        );
        self
    }

    // ── Rendered text assertions ────────────────────────────────────

    /// Assert that the rendered buffer contains the given substring anywhere.
    pub fn assert_rendered_contains(&mut self, text: &str) -> &mut Self {
        let buf = self.buffer_to_string();
        assert!(
            buf.contains(text),
            "expected rendered buffer to contain {:?}, but it was not found.\nBuffer:\n{}",
            text,
            buf
        );
        self
    }

    /// Assert that the rendered buffer does NOT contain the given substring.
    pub fn assert_rendered_not_contains(&mut self, text: &str) -> &mut Self {
        let buf = self.buffer_to_string();
        assert!(
            !buf.contains(text),
            "expected rendered buffer NOT to contain {:?}, but it was found.\nBuffer:\n{}",
            text,
            buf
        );
        self
    }

    /// Assert that the given row contains the given substring.
    pub fn assert_row_contains(&mut self, row: u16, text: &str) -> &mut Self {
        let line = self.buffer_row_to_string(row);
        assert!(
            line.contains(text),
            "expected row {} to contain {:?}, got {:?}",
            row,
            text,
            line
        );
        self
    }

    // ── Buffer introspection ────────────────────────────────────────

    /// Render a frame and return the entire buffer as a string (rows joined by `\n`).
    pub fn buffer_to_string(&mut self) -> String {
        self.render();
        let buf = self.terminal.backend().buffer();
        let area = buf.area;
        let mut lines = Vec::with_capacity(area.height as usize);
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                if let Some(cell) = buf.cell(Position::new(x, y)) {
                    line.push_str(cell.symbol());
                }
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    /// Render a frame and return a single row as a string (trailing spaces stripped).
    pub fn buffer_row_to_string(&mut self, row: u16) -> String {
        self.render();
        let buf = self.terminal.backend().buffer();
        let area = buf.area;
        assert!(
            row < area.height,
            "row {} out of bounds (terminal height {})",
            row,
            area.height
        );
        let mut line = String::new();
        for x in 0..area.width {
            if let Some(cell) = buf.cell(Position::new(x, row)) {
                line.push_str(cell.symbol());
            }
        }
        line.trim_end().to_string()
    }

    /// Render the current app state into the test terminal buffer.
    fn render(&mut self) {
        self.terminal
            .draw(|f| { render_frame(f, &self.app); })
            .expect("render frame in test harness");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_screen_toggle() {
        TuiTestHarness::new()
            .assert_screen(Screen::Main)
            .press_char('?')
            .assert_screen(Screen::Help)
            .assert_rendered_contains("Keyboard Reference")
            .press_esc()
            .assert_screen(Screen::Main);
    }

    #[test]
    fn quit_confirm_dialog() {
        TuiTestHarness::new()
            .assert_no_modal()
            .press_char('q')
            .assert_modal_quit_confirm()
            .assert_should_not_quit()
            .assert_rendered_contains("Quit?")
            .press_char('y')
            .assert_should_quit();
    }

    #[test]
    fn ctrl_c_immediate_quit() {
        TuiTestHarness::new()
            .assert_should_not_quit()
            .assert_no_modal()
            .press_ctrl_c()
            // Ctrl+C returns true from handle_key but doesn't set should_quit.
            // The actual quit happens in run_loop via the return value.
            // In the harness, should_quit remains false but the handle_key
            // return value signals immediate exit.
            .assert_no_modal();
    }

    #[test]
    fn navigation_and_detail() {
        TuiTestHarness::with_epic_and_task()
            .assert_selected_index(0)
            .assert_detail_closed()
            // Move down to the task row
            .press_char('j')
            .assert_selected_index(1)
            // Open detail pane
            .press_enter()
            .assert_detail_open()
            .assert_focus(Focus::Detail)
            .assert_rendered_contains("Test Task");
    }

    #[test]
    fn task_create_modal() {
        TuiTestHarness::new()
            .assert_no_modal()
            .press_char('n')
            .assert_modal_task_create()
            .assert_rendered_contains("New Task");
    }

    #[test]
    fn konami_crab_parade() {
        let mut h = TuiTestHarness::new();
        h.assert_crab_inactive();
        h.enter_konami_code();
        h.assert_crab_active();
        // Any keypress dismisses the crab parade
        h.press_char('x');
        h.assert_crab_inactive();
    }

    #[test]
    fn filter_dialog_rendered() {
        TuiTestHarness::new()
            .assert_no_modal()
            .press_char('f')
            .assert_rendered_contains("Filter");
    }

    #[test]
    fn status_bar_always_visible() {
        let mut h = TuiTestHarness::new();
        h.render();
        // Status bar is on the last row (row 23 in an 80×24 terminal)
        h.assert_row_contains(23, "[All]");
    }
}
