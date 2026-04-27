mod app;
mod config;
mod fs;
mod ui;

use std::{
    error::Error,
    io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use app::{App, FocusArea};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};

enum AppAction {
    None,
    Quit,
    OpenFile(PathBuf),
    OpenShell,
    ShowManual,
}

fn terminal_err<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::other(err.to_string())
}

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        show_manual_cli()?;
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let start_dir = std::env::current_dir()?;
    let app = App::new(start_dir)?;
    let result = run_app(&mut terminal, app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor().map_err(terminal_err)?;

    result?;
    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        terminal
            .draw(|frame| ui::render(frame, &app))
            .map_err(terminal_err)?;

        if event::poll(Duration::from_millis(250))? {
            let evt = event::read()?;
            if let Event::Key(key_event) = evt {
                if key_event.kind != KeyEventKind::Press {
                    continue;
                }
                match handle_key(&mut app, key_event)? {
                    AppAction::Quit => break,
                    AppAction::OpenFile(path) => open_file_in_editor(terminal, &mut app, &path)?,
                    AppAction::OpenShell => open_shell(terminal, &mut app)?,
                    AppAction::ShowManual => show_manual_in_app(terminal, &mut app)?,
                    AppAction::None => {}
                }
            }
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key_event: KeyEvent) -> io::Result<AppAction> {
    let modifiers = key_event.modifiers;
    if matches!(key_event.code, KeyCode::Char('c') | KeyCode::Char('C'))
        && modifiers.contains(KeyModifiers::CONTROL)
    {
        return Ok(AppAction::Quit);
    }
    if app.is_rename_modal_open() {
        match key_event.code {
            KeyCode::Esc => app.cancel_rename_modal(),
            KeyCode::Enter => app.submit_rename()?,
            KeyCode::Backspace => app.rename_input_backspace(),
            KeyCode::Left => app.rename_move_cursor_left(),
            KeyCode::Right => app.rename_move_cursor_right(),
            KeyCode::Char(c) if modifiers.is_empty() => app.rename_input_char(c),
            _ => {}
        }
        return Ok(AppAction::None);
    }
    if app.is_favorites_popup_open() {
        match key_event.code {
            KeyCode::Esc => app.close_favorites_popup(),
            KeyCode::Up => app.favorites_popup_move_up(),
            KeyCode::Down => app.favorites_popup_move_down(),
            KeyCode::Enter => app.activate_favorites_popup_selection()?,
            KeyCode::Delete => app.remove_selected_favorite(),
            _ => {}
        }
        return Ok(AppAction::None);
    }
    if app.is_history_popup_open() {
        match key_event.code {
            KeyCode::Esc => app.close_history_popup(),
            KeyCode::Up => app.history_popup_move_up(),
            KeyCode::Down => app.history_popup_move_down(),
            KeyCode::Enter => app.activate_history_popup_selection()?,
            _ => {}
        }
        return Ok(AppAction::None);
    }
    if app.is_move_mode_active() {
        match key_event.code {
            KeyCode::Esc => {
                app.cancel_move_mode();
                return Ok(AppAction::None);
            }
            KeyCode::Enter => {
                if app.focus == FocusArea::Right && app.is_current_dir_row_selected() {
                    app.execute_move_to_current_dir()?;
                } else {
                    app.status = String::from("宛先の '.' を選んで Enter してください");
                }
                return Ok(AppAction::None);
            }
            _ => {}
        }
    }
    if app.is_search_input_open() {
        return handle_search_input_key(app, key_event);
    }
    if app.is_search_results_open() {
        return handle_search_results_key(app, key_event);
    }
    match key_event.code {
        KeyCode::Tab => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                app.focus_prev();
            } else {
                app.focus_next();
            }
        }
        KeyCode::BackTab => app.focus_prev(),
        KeyCode::Up => {
            if app.focus == FocusArea::Right && modifiers.contains(KeyModifiers::SHIFT) {
                app.move_right_with_shift(true)?;
            } else {
                app.move_up()?;
            }
        }
        KeyCode::Down => {
            if app.focus == FocusArea::Right && modifiers.contains(KeyModifiers::SHIFT) {
                app.move_right_with_shift(false)?;
            } else {
                app.move_down()?;
            }
        }
        KeyCode::Enter => match app.focus {
            FocusArea::Path => app.apply_path_input()?,
            FocusArea::Right => app.open_selected()?,
            FocusArea::Left => {}
        },
        KeyCode::Backspace => match app.focus {
            FocusArea::Path => app.path_input_backspace(),
            FocusArea::Right => app.go_parent()?,
            FocusArea::Left => {}
        },
        KeyCode::Delete => {
            app.delete_selected_entry()?;
        }
        KeyCode::F(2) => {
            app.open_rename_modal()?;
        }
        KeyCode::Char(' ') if app.focus == FocusArea::Path && app.path_cursor_on_star => {
            app.toggle_favorite();
        }
        KeyCode::Char('s') | KeyCode::Char('S') if app.focus != FocusArea::Path => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                app.toggle_sort_key();
            } else {
                app.toggle_sort_order();
            }
        }
        KeyCode::Char('e') | KeyCode::Char('E') if app.focus != FocusArea::Path => {
            if app.is_current_dir_row_selected() {
                app.create_new_text_file()?;
            } else if let Some(path) = app.selected_file_path() {
                return Ok(AppAction::OpenFile(path));
            } else {
                app.status = String::from("ファイルを選択してください");
            }
        }
        KeyCode::F(6) if app.focus == FocusArea::Right => {
            return Ok(AppAction::OpenShell);
        }
        KeyCode::Char('k') | KeyCode::Char('K') if app.focus != FocusArea::Path => {
            app.create_new_folder()?;
        }
        KeyCode::Char('j') | KeyCode::Char('J') if app.focus == FocusArea::Right => {
            app.open_favorites_popup();
        }
        KeyCode::Char('h') | KeyCode::Char('H') if app.focus == FocusArea::Right => {
            app.open_history_popup();
        }
        KeyCode::Char('m') | KeyCode::Char('M') if app.focus == FocusArea::Right => {
            app.start_move_mode()?;
        }
        KeyCode::Char('c') | KeyCode::Char('C') if app.focus == FocusArea::Right => {
            app.copy_selected_entry()?;
        }
        KeyCode::Char('f') | KeyCode::Char('F') if app.focus == FocusArea::Right => {
            app.open_search_modal();
        }
        KeyCode::F(1) => {
            return Ok(AppAction::ShowManual);
        }
        KeyCode::Char(c)
            if app.focus == FocusArea::Path
                && modifiers.is_empty()
                && !(c == ' ' && app.path_cursor_on_star) =>
        {
            app.path_input_char(c);
        }
        KeyCode::Right => match app.focus {
            FocusArea::Path => app.move_path_cursor_right(),
            FocusArea::Left => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    app.scroll_active_pane_horizontal(true);
                } else {
                    app.expand_selected_dir()?;
                }
            }
            FocusArea::Right => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    app.scroll_active_pane_horizontal(true);
                } else {
                    app.history_forward()?;
                }
            }
        },
        KeyCode::Left => {
            if app.focus == FocusArea::Path {
                app.move_path_cursor_left();
            } else if app.focus == FocusArea::Left {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    app.scroll_active_pane_horizontal(false);
                } else {
                    app.collapse_selected_dir()?;
                }
            } else if app.focus == FocusArea::Right {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    app.scroll_active_pane_horizontal(false);
                } else {
                    app.history_back()?;
                }
            }
        }
        _ => {}
    }
    Ok(AppAction::None)
}

fn handle_search_input_key(app: &mut App, key_event: KeyEvent) -> io::Result<AppAction> {
    let modifiers = key_event.modifiers;
    match key_event.code {
        KeyCode::Esc => {
            app.close_search_modal();
        }
        KeyCode::Enter => {
            app.submit_search()?;
        }
        KeyCode::Backspace => {
            app.search_input_backspace();
        }
        KeyCode::Left => {
            app.search_move_cursor_left();
        }
        KeyCode::Right => {
            app.search_move_cursor_right();
        }
        KeyCode::Char(c) if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            app.search_input_char(c);
        }
        _ => {}
    }
    Ok(AppAction::None)
}

fn handle_search_results_key(app: &mut App, key_event: KeyEvent) -> io::Result<AppAction> {
    let modifiers = key_event.modifiers;
    match key_event.code {
        KeyCode::Esc => {
            app.close_search_modal();
        }
        KeyCode::Enter => {
            app.activate_search_selection()?;
        }
        KeyCode::Up => {
            app.search_results_move_up();
        }
        KeyCode::Down => {
            app.search_results_move_down();
        }
        KeyCode::Left if modifiers.contains(KeyModifiers::SHIFT) => {
            app.scroll_search_results_horizontal(false);
        }
        KeyCode::Right if modifiers.contains(KeyModifiers::SHIFT) => {
            app.scroll_search_results_horizontal(true);
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                app.toggle_sort_key();
            } else {
                app.toggle_sort_order();
            }
        }
        _ => {}
    }
    Ok(AppAction::None)
}

fn open_file_in_editor<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    path: &PathBuf,
) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor().map_err(terminal_err)?;

    let status = Command::new("vim").arg(path).status();

    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.hide_cursor().map_err(terminal_err)?;
    terminal.clear().map_err(terminal_err)?;

    match status {
        Ok(exit) => {
            if exit.success() {
                app.status = format!("vimを閉じました: {}", path.display());
            } else {
                app.status = format!("vim終了コード: {}", exit);
            }
        }
        Err(err) => {
            app.status = format!("vim起動に失敗: {err}");
        }
    }
    app.refresh_and_select(path)?;
    Ok(())
}

fn open_shell<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor().map_err(terminal_err)?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("/bin/bash"));
    let status = Command::new(shell).current_dir(&app.current_dir).status();

    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.hide_cursor().map_err(terminal_err)?;
    terminal.clear().map_err(terminal_err)?;

    match status {
        Ok(exit) => {
            if exit.success() {
                app.status = String::from("ターミナルを終了しました");
            } else {
                app.status = format!("シェル終了コード: {}", exit);
            }
        }
        Err(err) => {
            app.status = format!("シェル起動に失敗: {err}");
        }
    }
    app.refresh()?;
    app.sync_tree_to_current_dir();
    Ok(())
}

fn show_manual_in_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor().map_err(terminal_err)?;

    let man_path = manual_path()?;
    let result = run_man(&man_path);

    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.hide_cursor().map_err(terminal_err)?;
    terminal.clear().map_err(terminal_err)?;

    match &result {
        Ok(()) => app.status = String::from("ヘルプを閉じました"),
        Err(err) => app.status = format!("man 表示に失敗: {err}"),
    }
    result
}

fn show_manual_cli() -> Result<(), Box<dyn Error>> {
    let man_path = manual_path()?;
    run_man(&man_path)?;
    Ok(())
}

fn manual_path() -> io::Result<PathBuf> {
    let base = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    Ok(base.join("man").join("tact.1"))
}

fn run_man(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Manual file not found: {}", path.display()),
        ));
    }
    let status = Command::new("man").arg("-l").arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("man exited with {}", status)))
    }
}
