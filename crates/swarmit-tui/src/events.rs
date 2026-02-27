use crossterm::event::KeyCode;

const KONAMI_SEQUENCE: &[KeyCode] = &[
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

/// Tracks progress through the Konami code sequence (↑↑↓↓←→←→BA).
/// Returns `true` from `feed()` when the full sequence is entered.
pub struct KonamiTracker {
    progress: usize,
}

impl KonamiTracker {
    pub fn new() -> Self {
        Self { progress: 0 }
    }

    /// Feed a key press. Returns `true` if the full Konami sequence is now complete.
    pub fn feed(&mut self, key: KeyCode) -> bool {
        if KONAMI_SEQUENCE.get(self.progress) == Some(&key) {
            self.progress += 1;
            if self.progress == KONAMI_SEQUENCE.len() {
                self.progress = 0;
                return true;
            }
        } else {
            self.progress = 0;
            // The wrong key might still be the first key of the sequence.
            if KONAMI_SEQUENCE[0] == key {
                self.progress = 1;
            }
        }
        false
    }

    pub fn reset(&mut self) {
        self.progress = 0;
    }
}

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Main,
    Help,
}

/// Which pane currently has keyboard focus.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    List,
    Detail,
}

/// Whether the main split runs horizontally (list|detail) or vertically (list/detail).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplitDirection {
    #[default]
    Horizontal,
    Vertical,
}

/// User input actions, decoupled from raw key events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    FocusDetail,
    FocusListPane,
    Back,
    Quit,
    NewTask,
    Help,
    Search,
    Refresh,
    ToggleCollapse,
    TabBackward,
    TabForward,
    ToggleSplitDirection,
    OpenFilterDialog,
    FilterDialogMove(i8),
    FilterDialogConfirm,
    FilterDialogCancel,
    OpenSortDialog,
    SortDialogMove(i8),
    SortDialogConfirm,
    SortDialogCancel,
    ResizePane(i8),
    SwitchDetailTab,
    OpenStatusDialog,
    OpenEpicDialog,
    StatusDialogMove(i8),
    StatusDialogConfirm,
    StatusDialogCancel,
    EpicDialogMove(i8),
    EpicDialogConfirm,
    EpicDialogCancel,
    EditDescription,
    AddComment,
    None,
}

/// Which form field has focus in the task-creation modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskFormField {
    Title,
    Description,
    Epic,
    Priority,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_konami_tracker_sequence() {
        let mut tracker = KonamiTracker::new();
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
        for (i, &key) in sequence.iter().enumerate() {
            let result = tracker.feed(key);
            if i < sequence.len() - 1 {
                assert!(!result, "should not complete early at index {}", i);
            } else {
                assert!(result, "should complete on last key");
            }
        }
    }

    #[test]
    fn test_konami_tracker_resets_on_wrong_key() {
        let mut tracker = KonamiTracker::new();
        // Feed first two correct keys
        assert!(!tracker.feed(KeyCode::Up));
        assert!(!tracker.feed(KeyCode::Up));
        assert_eq!(tracker.progress, 2);
        // Wrong key — should reset
        assert!(!tracker.feed(KeyCode::Char('x')));
        assert_eq!(tracker.progress, 0);
    }

    #[test]
    fn test_konami_tracker_partial_then_wrong() {
        let mut tracker = KonamiTracker::new();
        // Feed first 4 keys correctly
        tracker.feed(KeyCode::Up);
        tracker.feed(KeyCode::Up);
        tracker.feed(KeyCode::Down);
        tracker.feed(KeyCode::Down);
        assert_eq!(tracker.progress, 4);
        // Feed wrong key — resets
        tracker.feed(KeyCode::Char('z'));
        assert_eq!(tracker.progress, 0);
        // Can still complete the full sequence from scratch
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
        for (i, &key) in sequence.iter().enumerate() {
            let done = tracker.feed(key);
            if i == sequence.len() - 1 {
                assert!(done);
            }
        }
    }
}

/// Active modal overlay (mutually exclusive with normal screen interaction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    QuitConfirm,
    TaskCreate {
        title: String,
        cursor_pos: usize,
        description: Vec<String>,
        desc_row: usize,
        desc_col: usize,
        epic_index: usize,
        priority_index: usize,
        focused_field: TaskFormField,
        error: Option<String>,
        /// True when Esc was pressed on a non-empty form — awaiting y/n confirmation.
        confirm_discard: bool,
    },
    FilterSelect {
        selected_index: usize,
    },
    SortSelect {
        selected_index: usize,
    },
    StatusSelect {
        selected_index: usize,
    },
    EpicSelect {
        selected_index: usize,
    },
}
