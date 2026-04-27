use chrono::{DateTime, Local};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin},
    prelude::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use std::time::SystemTime;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{App, DirNode, FocusArea, NAME_COLUMN_WIDTH},
    fs::FsEntry,
};
const SEARCH_RESULTS_DATE_WIDTH: usize = 19;

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(frame.area());

    render_path_bar(frame, chunks[0], app);
    render_main_panes(frame, chunks[1], app);
    render_status_bar(frame, chunks[2], app);
    if app.is_favorites_popup_open() {
        render_favorites_modal(frame, app);
    }
    if app.is_history_popup_open() {
        render_history_modal(frame, app);
    }
    if app.is_rename_modal_open() {
        render_rename_modal(frame, app);
    }
    if app.is_search_input_open() {
        render_search_input_modal(frame, app);
    }
    if app.is_search_results_open() {
        render_search_results_modal(frame, app);
    }
}

fn render_path_bar(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let is_focused = app.focus == FocusArea::Path;
    let star = if app.display_path_is_favorite() {
        "★"
    } else {
        "☆"
    };
    let star_prefix = format!("{star} ");
    let base_text = if is_focused {
        app.path_input.clone()
    } else {
        app.current_dir.display().to_string()
    };
    let text = format!("{star_prefix}{base_text}");
    let border_style = if is_focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let block = Block::default()
        .title("Path")
        .borders(Borders::ALL)
        .border_style(border_style);
    let paragraph = Paragraph::new(text.clone()).block(block);
    frame.render_widget(paragraph, area);
    if is_focused && !app.is_favorites_popup_open() {
        let prefix_width = UnicodeWidthStr::width(star_prefix.as_str());
        let cursor_width = if app.path_cursor_on_star {
            0
        } else {
            let cursor_index = app.path_cursor.min(app.path_input.len());
            let slice = &app.path_input[..cursor_index];
            prefix_width + UnicodeWidthStr::width(slice)
        };
        let available = area.width.saturating_sub(2) as usize;
        let visible_offset = cursor_width.min(available);
        let cursor_x = area.x + 1 + visible_offset as u16;
        let cursor_y = area.y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn render_favorites_modal(frame: &mut Frame<'_>, app: &App) {
    let favorites = app.favorite_paths();
    if favorites.is_empty() {
        return;
    }
    let (start, end) = app.favorites_popup_window();
    if start >= end {
        return;
    }
    let visible_rows = app.favorites_popup_visible_rows();
    if visible_rows == 0 {
        return;
    }
    let rows: Vec<ListItem> = favorites[start..end]
        .iter()
        .map(|path| ListItem::new(path.clone()))
        .collect();
    let mut state = ListState::default();
    state.select(app.favorites_popup_selected_visible_index());

    let total_area = frame.area();
    if total_area.width == 0 || total_area.height == 0 {
        return;
    }
    let popup_height = (visible_rows as u16)
        .saturating_add(2)
        .min(total_area.height);
    let width_upper = total_area.width.min(70);
    let popup_width = if total_area.width < 30 {
        total_area.width
    } else {
        width_upper.max(30)
    };
    let popup_area = Rect {
        x: total_area.x + (total_area.width.saturating_sub(popup_width)) / 2,
        y: total_area.y + (total_area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title("Favorites")
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    let highlight_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let list = List::new(rows)
        .block(block)
        .highlight_style(highlight_style)
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, popup_area, &mut state);
}

fn render_history_modal(frame: &mut Frame<'_>, app: &App) {
    if !app.is_history_popup_open() {
        return;
    }
    let entries = app.history_entries();
    if entries.is_empty() {
        return;
    }
    let (start, end) = app.history_popup_window();
    if start >= end {
        return;
    }
    let rows: Vec<ListItem> = (start..end)
        .filter_map(|display_idx| {
            app.history_entry_for_display(display_idx)
                .map(|path| ListItem::new(path.display().to_string()))
        })
        .collect();
    let mut state = ListState::default();
    state.select(app.history_popup_selected_visible_index());

    let total_area = frame.area();
    if total_area.width == 0 || total_area.height == 0 {
        return;
    }
    let popup_height = (rows.len() as u16).saturating_add(2).min(total_area.height);
    let width_upper = total_area.width.min(70);
    let popup_width = if total_area.width < 40 {
        total_area.width
    } else {
        width_upper.max(40)
    };
    let popup_area = Rect {
        x: total_area.x + (total_area.width.saturating_sub(popup_width)) / 2,
        y: total_area.y + (total_area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title("History")
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    let highlight_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let list = List::new(rows)
        .block(block)
        .highlight_style(highlight_style)
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, popup_area, &mut state);
}

fn render_rename_modal(frame: &mut Frame<'_>, app: &App) {
    let total = frame.area();
    if total.width == 0 || total.height == 0 {
        return;
    }
    let width = total.width.clamp(20, 50);
    let height = 3u16;
    let area = Rect {
        x: total.x + (total.width.saturating_sub(width)) / 2,
        y: total.y + (total.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title("Rename")
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    let paragraph = Paragraph::new(app.rename_text()).block(block);
    frame.render_widget(paragraph, area);

    let inner = area.inner(Margin::new(1, 1));
    let prefix_len = app.rename_cursor().min(app.rename_text().len());
    let prefix = &app.rename_text()[..prefix_len];
    let mut cursor_width = UnicodeWidthStr::width(prefix);
    let max_visible = inner.width.saturating_sub(1) as usize;
    if cursor_width > max_visible {
        cursor_width = max_visible;
    }
    let cursor_x = inner.x + cursor_width as u16;
    let cursor_y = inner.y;
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn render_search_input_modal(frame: &mut Frame<'_>, app: &App) {
    if !app.is_search_input_open() {
        return;
    }
    let total = frame.area();
    if total.width == 0 || total.height == 0 {
        return;
    }
    let popup_width = total.width.clamp(30, 80);
    let popup_height = 3u16;
    let area = Rect {
        x: total.x + (total.width.saturating_sub(popup_width)) / 2,
        y: total.y + (total.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title("Search Pattern (*可)")
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    let paragraph = Paragraph::new(app.search_input_text().to_string()).block(block);
    frame.render_widget(paragraph, area);
    let inner = area.inner(Margin::new(1, 1));
    let cursor_index = app.search_cursor().min(app.search_input_text().len());
    let prefix = &app.search_input_text()[..cursor_index];
    let cursor_width = UnicodeWidthStr::width(prefix).min(inner.width as usize);
    let cursor_x = inner.x + cursor_width as u16;
    let cursor_y = inner.y;
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn render_search_results_modal(frame: &mut Frame<'_>, app: &App) {
    if !app.is_search_results_open() {
        return;
    }
    let total = frame.area();
    if total.width == 0 || total.height == 0 {
        return;
    }
    let popup_width = total.width.clamp(30, 90);
    let popup_height = total.height.clamp(10, 25);
    let area = Rect {
        x: total.x + (total.width.saturating_sub(popup_width)) / 2,
        y: total.y + (total.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    frame.render_widget(Clear, area);

    let results_title = format!(
        "検索結果 {} 件 (Enterで移動 / Escで閉じる)",
        app.search_results().len()
    );
    let block = Block::default()
        .title(results_title)
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(area);
    let highlight_width = 2u16;
    let available_width = inner.width.saturating_sub(highlight_width) as usize;
    let (name_width, gap_spaces, date_width) = if available_width <= SEARCH_RESULTS_DATE_WIDTH {
        (0usize, 0usize, available_width)
    } else {
        let gap = 1usize;
        let name_width = available_width.saturating_sub(SEARCH_RESULTS_DATE_WIDTH + gap);
        (name_width, gap, SEARCH_RESULTS_DATE_WIDTH)
    };

    let rows: Vec<ListItem> = if app.search_results().is_empty() {
        vec![ListItem::new("検索結果なし")]
    } else {
        app.search_results()
            .iter()
            .map(|entry| {
                let line = format_search_result_line(
                    entry,
                    app.search_scroll_offset(),
                    name_width,
                    gap_spaces,
                    date_width,
                );
                ListItem::new(line)
            })
            .collect()
    };
    let mut state = ListState::default();
    state.select(app.search_selected_index());
    let highlight_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let list = List::new(rows)
        .block(block)
        .highlight_style(highlight_style)
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_status_bar(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let info = format!("{} | {} items", app.status, app.entry_count());
    let paragraph =
        Paragraph::new(info).block(Block::default().title("Status").borders(Borders::ALL));
    frame.render_widget(paragraph, area);
}

fn render_main_panes(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
        .split(area);
    let left_area = chunks[0];
    let right_area = chunks[1];
    let left_width = left_area.width.saturating_sub(2);

    let left_selection = if app.visible_dirs.is_empty() {
        None
    } else {
        Some(app.left_index)
    };
    let right_selection = Some(app.right_index);

    let left_items = app
        .visible_directory_nodes()
        .into_iter()
        .map(|(node, depth)| {
            let label = format_dir_label(node, depth);
            let trimmed = apply_horizontal_scroll(&label, app.left_scroll_offset, left_width);
            ListItem::new(trimmed)
        });

    render_list(
        frame,
        left_area,
        app,
        FocusArea::Left,
        "Directories",
        left_items,
        left_selection,
    );

    let mut right_items: Vec<ListItem> = Vec::new();
    let current = format_current_dir_line(app.right_scroll_offset);
    right_items.push(styled_right_item(app, 0, current));
    for (idx, entry) in app.entries.iter().enumerate() {
        let row = idx + 1;
        let line = format_entry_line(entry, app.right_scroll_offset);
        right_items.push(styled_right_item(app, row, line));
    }

    render_list(
        frame,
        right_area,
        app,
        FocusArea::Right,
        "Files",
        right_items,
        right_selection,
    );
}

fn render_list<'a, I>(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    pane: FocusArea,
    title: &str,
    items: I,
    selected: Option<usize>,
) where
    I: IntoIterator<Item = ListItem<'a>>,
{
    let mut state = ListState::default();
    state.select(selected);

    let is_active = app.focus == pane;
    let border_style = if is_active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    let highlight_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    let list = List::new(items)
        .highlight_style(highlight_style)
        .highlight_symbol("> ")
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style),
        );

    frame.render_stateful_widget(list, area, &mut state);
}

fn styled_right_item<'a>(app: &App, row: usize, text: String) -> ListItem<'a> {
    if app.is_row_selected(row) && row != app.right_index {
        ListItem::new(text).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        ListItem::new(text)
    }
}

fn format_entry_line(entry: &FsEntry, offset: u16) -> String {
    let name_raw = if entry.is_dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    };
    let scrolled = apply_horizontal_scroll(name_raw.as_str(), offset, NAME_COLUMN_WIDTH as u16);
    let name = pad_to_width(&scrolled, NAME_COLUMN_WIDTH);
    let size = format_size(entry);
    let modified = format_modified(entry.modified);
    format!("{name}{:>9}  {modified}", size)
}

fn format_search_result_line(
    entry: &FsEntry,
    offset: u16,
    name_width: usize,
    gap_spaces: usize,
    date_width: usize,
) -> String {
    let mut path = entry.path.display().to_string();
    if entry.is_dir && !path.ends_with('/') {
        path.push('/');
    }
    let mut line = String::new();
    if name_width > 0 {
        let width_u16 = name_width.min(u16::MAX as usize) as u16;
        let scrolled = apply_horizontal_scroll(path.as_str(), offset, width_u16);
        line.push_str(&pad_to_width(&scrolled, name_width));
    }
    if gap_spaces > 0 {
        line.push_str(&" ".repeat(gap_spaces));
    }
    let modified = format_modified(entry.modified);
    let trimmed = truncate_end_to_width(modified.as_str(), date_width);
    let aligned = if date_width == 0 {
        String::new()
    } else {
        format!("{trimmed:>width$}", width = date_width)
    };
    line.push_str(&aligned);
    line
}

fn format_dir_label(node: &DirNode, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let marker = if node.has_children {
        if node.expanded { " - " } else { " + " }
    } else {
        "   "
    };
    let mut name = node.entry.name.clone();
    if !name.ends_with('/') {
        name.push('/');
    }
    format!("{indent}{marker} {name}")
}

fn format_current_dir_line(offset: u16) -> String {
    let scrolled = apply_horizontal_scroll(".", offset, NAME_COLUMN_WIDTH as u16);
    let name = pad_to_width(&scrolled, NAME_COLUMN_WIDTH);
    format!("{name}{:>9}  {}", "", "")
}

fn format_size(entry: &FsEntry) -> String {
    if entry.is_dir {
        "<DIR>".into()
    } else {
        human_readable_size(entry.size)
    }
}

fn human_readable_size(size: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{:.0} {}", value, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_modified(time: Option<SystemTime>) -> String {
    if let Some(system_time) = time {
        let datetime: DateTime<Local> = system_time.into();
        datetime.format("%Y-%m-%d %H:%M").to_string()
    } else {
        "-".into()
    }
}

fn truncate_to_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut acc = String::new();
    let mut current_width = 0usize;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        if current_width + ch_width > width {
            break;
        }
        acc.push(ch);
        current_width += ch_width;
    }
    acc
}

fn pad_to_width(text: &str, width: usize) -> String {
    let mut truncated = truncate_to_width(text, width);
    let mut current = UnicodeWidthStr::width(truncated.as_str());
    while current < width {
        truncated.push(' ');
        current += 1;
    }
    truncated
}

fn apply_horizontal_scroll(text: &str, offset: u16, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    let mut skip = offset as usize;
    let mut current_width = 0usize;
    let mut out = String::new();
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        if skip >= ch_width {
            skip -= ch_width;
            continue;
        } else if skip > 0 {
            skip = 0;
            continue;
        }
        if current_width + ch_width > width as usize {
            break;
        }
        out.push(ch);
        current_width += ch_width;
    }
    out
}

fn truncate_end_to_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut acc = Vec::new();
    let mut current = 0usize;
    for ch in text.chars().rev() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        if current + ch_width > width {
            break;
        }
        acc.push(ch);
        current += ch_width;
    }
    acc.iter().rev().collect()
}
