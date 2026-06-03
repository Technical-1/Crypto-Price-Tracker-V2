//! Input mapping: crossterm key events -> high-level `Action`s.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    NextView,
    PrevView,
    CycleMethod,
    CycleSort,
    ToggleGrouping,
    NextYear,
    PrevYear,
    ToggleStrategy,
    Refresh,
    Export,
    SelectNext,
    SelectPrev,
    ToggleHelp,
}

pub fn map_key(key: KeyEvent) -> Option<Action> {
    Some(match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Tab | KeyCode::Right => Action::NextView,
        KeyCode::BackTab | KeyCode::Left => Action::PrevView,
        KeyCode::Char('m') => Action::CycleMethod,
        KeyCode::Char('s') => Action::CycleSort,
        KeyCode::Char('g') => Action::ToggleGrouping,
        KeyCode::Char(']') => Action::NextYear,
        KeyCode::Char('[') => Action::PrevYear,
        KeyCode::Char('t') => Action::ToggleStrategy,
        KeyCode::Char('r') => Action::Refresh,
        KeyCode::Char('e') => Action::Export,
        KeyCode::Down => Action::SelectNext,
        KeyCode::Up => Action::SelectPrev,
        KeyCode::Char('?') => Action::ToggleHelp,
        _ => return None,
    })
}

/// Apply an action to app state. Returns `true` if a price refresh was requested.
pub fn apply(app: &mut App, action: Action) -> bool {
    match action {
        Action::Quit => app.should_quit = true,
        Action::NextView => app.next_view(),
        Action::PrevView => app.prev_view(),
        Action::CycleMethod => app.cycle_method(),
        Action::CycleSort => app.cycle_sort(),
        Action::ToggleGrouping => app.toggle_grouping(),
        Action::NextYear => app.set_year(1),
        Action::PrevYear => app.set_year(-1),
        Action::ToggleStrategy => app.toggle_strategy(),
        Action::Refresh => {
            app.loading = true;
            return true;
        }
        Action::Export => app.status.message = "export requested".into(),
        Action::SelectNext => app.select_next(),
        Action::SelectPrev => app.select_prev(),
        Action::ToggleHelp => app.toggle_help(),
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn maps_quit_keys() {
        assert_eq!(map_key(key('q')), Some(Action::Quit));
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn maps_navigation_and_method() {
        assert_eq!(map_key(key('m')), Some(Action::CycleMethod));
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::NextView)
        );
        assert_eq!(map_key(key('[')), Some(Action::PrevYear));
        assert_eq!(map_key(key(']')), Some(Action::NextYear));
    }

    #[test]
    fn unmapped_key_returns_none() {
        assert_eq!(map_key(key('z')), None);
    }
}
