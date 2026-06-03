use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Screen};

pub fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return true;
    }

    if app.is_action_mode() {
        return handle_action_mode(app, key);
    }

    match app.screen {
        Screen::SourceList => handle_source_list(app, key),
        Screen::CategorySummary => handle_category_summary(app, key),
        Screen::EntryChecklist => handle_entry_checklist(app, key),
        Screen::PreviewCleanup => handle_preview_cleanup(app, key),
        Screen::ConfirmCleanup => handle_confirm_cleanup(app, key),
        Screen::CleanupResult => handle_cleanup_result(app, key),
    }
}

fn handle_source_list(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Up => app.move_up(),
        KeyCode::Down => app.move_down(),
        KeyCode::Enter => app.enter(),
        KeyCode::Tab => app.enter_action_mode(),
        KeyCode::Esc => app.back(),
        KeyCode::Char('q') => return true,
        _ => {}
    }

    false
}

fn handle_category_summary(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Up => app.move_up(),
        KeyCode::Down => app.move_down(),
        KeyCode::Enter => app.enter(),
        KeyCode::Tab => app.enter_action_mode(),
        KeyCode::Esc => app.back(),
        KeyCode::Char('q') => return true,
        _ => {}
    }

    false
}

fn handle_entry_checklist(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Up => app.move_up(),
        KeyCode::Down => app.move_down(),
        KeyCode::Char(' ') => app.toggle_selected_entry(),
        KeyCode::Tab => app.enter_action_mode(),
        KeyCode::Esc => app.back(),
        KeyCode::Char('q') => return true,
        _ => {}
    }

    false
}

fn handle_preview_cleanup(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Tab => app.enter_action_mode(),
        KeyCode::Esc => app.back(),
        KeyCode::Char('q') => return true,
        _ => {}
    }

    false
}

fn handle_confirm_cleanup(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('y') => app.execute_cleanup(),
        KeyCode::Char('n') | KeyCode::Esc => app.back(),
        KeyCode::Char('q') => return true,
        _ => {}
    }

    false
}

fn handle_cleanup_result(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => app.back(),
        KeyCode::Char('q') => return true,
        _ => {}
    }

    false
}

fn handle_action_mode(app: &mut App, key: KeyEvent) -> bool {
    match app.screen {
        Screen::SourceList => match key.code {
            KeyCode::Esc | KeyCode::Tab => app.cancel_action_mode(),
            KeyCode::Char('q') => return true,
            _ => {}
        },
        Screen::CategorySummary => match key.code {
            KeyCode::Char('c') => app.show_preview(),
            KeyCode::Char('r') => app.rescan_xcode(),
            KeyCode::Esc | KeyCode::Tab => app.cancel_action_mode(),
            KeyCode::Char('q') => return true,
            _ => {}
        },
        Screen::EntryChecklist => match key.code {
            KeyCode::Char('a') => app.mark_all_selected_category(true),
            KeyCode::Char('r') => app.mark_all_selected_category(false),
            KeyCode::Char('c') => app.show_preview(),
            KeyCode::Esc | KeyCode::Tab => app.cancel_action_mode(),
            KeyCode::Char('q') => return true,
            _ => {}
        },
        Screen::PreviewCleanup => match key.code {
            KeyCode::Char('c') => app.show_confirmation(),
            KeyCode::Esc | KeyCode::Tab => app.cancel_action_mode(),
            KeyCode::Char('q') => return true,
            _ => {}
        },
        Screen::ConfirmCleanup | Screen::CleanupResult => {}
    }

    false
}
