use std::path::{Path, PathBuf};
use std::{cmp::Ordering, collections::BTreeSet, fs as stdfs, io};

use crate::{
    config::Config,
    fs::{self, FsEntry},
};
use crossterm::terminal;
use unicode_width::UnicodeWidthStr;

const FAVORITES_DROPDOWN_VISIBLE: usize = 6;
const HISTORY_POPUP_VISIBLE: usize = 10;
const HSCROLL_STEP: u16 = 4;
const HISTORY_LIMIT: usize = 100;
pub const NAME_COLUMN_WIDTH: usize = 32;

#[derive(Clone)]
pub struct DirNode {
    pub entry: FsEntry,
    pub children: Option<Vec<DirNode>>,
    pub expanded: bool,
    pub has_children: bool,
}

impl DirNode {
    pub fn new(entry: FsEntry) -> Self {
        let has_children = detect_child_directories(&entry.path);
        Self {
            entry,
            children: None,
            expanded: false,
            has_children,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    Path,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Modified,
}

impl SortKey {
    fn next(self) -> Self {
        match self {
            SortKey::Name => SortKey::Modified,
            SortKey::Modified => SortKey::Name,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SortKey::Name => "名前",
            SortKey::Modified => "日時",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    fn toggle(self) -> Self {
        match self {
            SortOrder::Asc => SortOrder::Desc,
            SortOrder::Desc => SortOrder::Asc,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SortOrder::Asc => "昇順",
            SortOrder::Desc => "降順",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchModalState {
    Closed,
    Input,
    Results,
}

pub struct App {
    pub current_dir: PathBuf,
    pub entries: Vec<FsEntry>,
    pub left_index: usize,
    pub right_index: usize,
    pub left_scroll_offset: u16,
    pub right_scroll_offset: u16,
    pub focus: FocusArea,
    pub status: String,
    pub tree_root: DirNode,
    pub visible_dirs: Vec<Vec<usize>>,
    left_max_label_width: usize,
    last_loaded_dir: Option<PathBuf>,
    pub path_input: String,
    pub path_cursor: usize,
    pub favorite_current: bool,
    pub path_cursor_on_star: bool,
    favorites_popup_open: bool,
    favorites_popup_index: usize,
    favorites_popup_offset: usize,
    history_popup_open: bool,
    history_popup_index: usize,
    history_popup_offset: usize,
    sort_key: SortKey,
    sort_order: SortOrder,
    rename_modal_open: bool,
    rename_input: String,
    rename_cursor: usize,
    rename_target: Option<PathBuf>,
    search_modal_state: SearchModalState,
    search_input: String,
    search_cursor: usize,
    search_results: Vec<FsEntry>,
    search_selected: usize,
    search_scroll_offset: u16,
    right_selection_anchor: Option<usize>,
    right_selected_rows: BTreeSet<usize>,
    right_max_name_width: usize,
    move_mode_active: bool,
    move_targets: Vec<PathBuf>,
    history: Vec<PathBuf>,
    history_index: usize,
    config: Config,
    config_path: PathBuf,
}

impl App {
    pub fn new(start_dir: PathBuf) -> io::Result<Self> {
        let root_entry = fs::entry_from_path(start_dir.clone())?;
        let (config, config_path) = Config::load_or_default();
        let path_display = start_dir.display().to_string();
        let mut app = Self {
            current_dir: start_dir,
            entries: Vec::new(),
            left_index: 0,
            right_index: 0,
            left_scroll_offset: 0,
            right_scroll_offset: 0,
            focus: FocusArea::Left,
            status: String::from("Ready"),
            tree_root: DirNode::new(root_entry),
            visible_dirs: Vec::new(),
            left_max_label_width: 0,
            last_loaded_dir: None,
            path_input: path_display,
            path_cursor: 0,
            favorite_current: false,
            path_cursor_on_star: false,
            favorites_popup_open: false,
            favorites_popup_index: 0,
            favorites_popup_offset: 0,
            history_popup_open: false,
            history_popup_index: 0,
            history_popup_offset: 0,
            sort_key: SortKey::Name,
            sort_order: SortOrder::Asc,
            rename_modal_open: false,
            rename_input: String::new(),
            rename_cursor: 0,
            rename_target: None,
            search_modal_state: SearchModalState::Closed,
            search_input: String::from("*"),
            search_cursor: 1,
            search_results: Vec::new(),
            search_selected: 0,
            search_scroll_offset: 0,
            right_selection_anchor: None,
            right_selected_rows: BTreeSet::new(),
            right_max_name_width: 1,
            move_mode_active: false,
            move_targets: Vec::new(),
            history: Vec::new(),
            history_index: 0,
            config,
            config_path,
        };
        app.refresh()?;
        app.sync_tree_to_current_dir();
        app.path_cursor = app.path_input.len();
        app.path_cursor_on_star = false;
        app.update_favorite_flag();
        app.focus_current_dir_row();
        app.history.clear();
        app.history_index = 0;
        let initial_dir = app.current_dir.clone();
        app.record_history_entry(&initial_dir);
        Ok(app)
    }

    pub fn refresh(&mut self) -> io::Result<()> {
        if self.last_loaded_dir.as_ref() == Some(&self.current_dir) {
            return Ok(());
        }
        match fs::read_directory(&self.current_dir) {
            Ok(entries) => {
                self.entries = entries;
                self.last_loaded_dir = Some(self.current_dir.clone());
                self.sort_entries_with_current_key();
                self.update_right_name_width();
                self.clamp_right_scroll_offset();
            }
            Err(err) => {
                self.status = format!("Failed to read {}: {}", self.current_dir.display(), err);
                self.entries.clear();
                self.last_loaded_dir = None;
                self.clamp_right_index();
                self.right_max_name_width = UnicodeWidthStr::width(".");
                self.right_scroll_offset = 0;
            }
        }
        Ok(())
    }

    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            FocusArea::Path => FocusArea::Left,
            FocusArea::Left => FocusArea::Right,
            FocusArea::Right => FocusArea::Path,
        };
        self.on_focus_changed();
    }

    pub fn focus_prev(&mut self) {
        self.focus = match self.focus {
            FocusArea::Path => FocusArea::Right,
            FocusArea::Left => FocusArea::Path,
            FocusArea::Right => FocusArea::Left,
        };
        self.on_focus_changed();
    }

    pub fn move_up(&mut self) -> io::Result<()> {
        let mut left_changed = false;
        let mut right_changed = false;
        match self.focus {
            FocusArea::Path => {}
            FocusArea::Left => {
                if self.left_index > 0 {
                    self.left_index -= 1;
                    left_changed = true;
                }
            }
            FocusArea::Right => {
                if self.right_index > 0 {
                    self.right_index -= 1;
                    right_changed = true;
                }
            }
        }
        if left_changed {
            self.update_current_dir_from_left_selection()?;
        }
        if right_changed {
            self.reset_right_selection_to_current();
        }
        Ok(())
    }

    pub fn move_down(&mut self) -> io::Result<()> {
        let mut left_changed = false;
        let mut right_changed = false;
        match self.focus {
            FocusArea::Path => {}
            FocusArea::Left => {
                let visible_count = self.visible_dirs.len();
                if visible_count > 0 && self.left_index + 1 < visible_count {
                    self.left_index += 1;
                    left_changed = true;
                }
            }
            FocusArea::Right => {
                let total_items = self.total_right_items();
                if total_items > 0 && self.right_index + 1 < total_items {
                    self.right_index += 1;
                    right_changed = true;
                }
            }
        }
        if left_changed {
            self.update_current_dir_from_left_selection()?;
        }
        if right_changed {
            self.reset_right_selection_to_current();
        }
        Ok(())
    }

    pub fn move_right_with_shift(&mut self, up: bool) -> io::Result<()> {
        if self.focus != FocusArea::Right {
            return Ok(());
        }
        self.ensure_right_anchor();
        let total_items = self.total_right_items();
        if total_items == 0 {
            return Ok(());
        }
        if up {
            if self.right_index > 0 {
                self.right_index -= 1;
            }
        } else if self.right_index + 1 < total_items {
            self.right_index += 1;
        }
        self.update_right_selection_range();
        Ok(())
    }

    pub fn open_selected(&mut self) -> io::Result<()> {
        if self.is_current_dir_row_selected() {
            self.status = String::from("現在のディレクトリです");
            return Ok(());
        }
        if let Some(entry) = self.selected_entry()
            && entry.is_dir
        {
            let status = format!("Opened {}", entry.name);
            self.set_current_directory(entry.path.clone(), Some(status))?;
        }
        Ok(())
    }

    pub fn go_parent(&mut self) -> io::Result<()> {
        let previous_dir = self.current_dir.clone();
        if let Some(parent) = self.current_dir.parent().map(PathBuf::from) {
            let status = String::from("Moved to parent directory");
            self.set_current_directory(parent, Some(status))?;
            if self.current_dir != previous_dir {
                self.select_entry_by_path(&previous_dir);
            }
        } else {
            self.status = String::from("Already at filesystem root");
        }
        Ok(())
    }

    pub fn history_back(&mut self) -> io::Result<()> {
        if self.history.is_empty() {
            self.status = String::from("履歴がありません");
            return Ok(());
        }
        if self.history_index == 0 {
            self.status = String::from("履歴の先頭です");
            return Ok(());
        }
        let mut idx = self.history_index;
        while idx > 0 {
            idx -= 1;
            if idx >= self.history.len() {
                continue;
            }
            let target = self.history[idx].clone();
            if directory_exists(&target) {
                self.history_index = idx;
                return self.set_current_directory_internal(
                    target,
                    Some(String::from("履歴: 戻る")),
                    true,
                    false,
                );
            }
            self.history.remove(idx);
            if self.history.is_empty() {
                self.history_index = 0;
                break;
            }
            if self.history_index > idx {
                self.history_index -= 1;
            }
        }
        self.status = String::from("履歴の先頭です");
        Ok(())
    }

    pub fn history_forward(&mut self) -> io::Result<()> {
        if self.history.is_empty() {
            self.status = String::from("履歴がありません");
            return Ok(());
        }
        if self.history_index + 1 >= self.history.len() {
            self.status = String::from("履歴の末尾です");
            return Ok(());
        }
        let idx = self.history_index + 1;
        while idx < self.history.len() {
            let target = self.history[idx].clone();
            if directory_exists(&target) {
                self.history_index = idx;
                return self.set_current_directory_internal(
                    target,
                    Some(String::from("履歴: 進む")),
                    true,
                    false,
                );
            }
            self.history.remove(idx);
            if self.history.is_empty() {
                self.history_index = 0;
                break;
            }
            if self.history_index >= idx && self.history_index > 0 {
                self.history_index -= 1;
            }
        }
        self.status = String::from("履歴の末尾です");
        Ok(())
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    fn set_current_directory(&mut self, path: PathBuf, status: Option<String>) -> io::Result<()> {
        self.set_current_directory_internal(path, status, true, true)
    }

    pub fn visible_directory_nodes(&self) -> Vec<(&DirNode, usize)> {
        self.visible_dirs
            .iter()
            .filter_map(|path| self.node_at_path(path).map(|node| (node, path.len())))
            .collect()
    }

    pub fn selected_file_path(&self) -> Option<PathBuf> {
        if self.focus == FocusArea::Right && !self.is_current_dir_row_selected() {
            self.entry_index_for_row(self.right_index)
                .and_then(|idx| self.entries.get(idx))
                .filter(|entry| !entry.is_dir)
                .map(|entry| entry.path.clone())
        } else {
            None
        }
    }

    pub fn toggle_sort_key(&mut self) {
        self.sort_key = self.sort_key.next();
        self.sort_entries_with_current_key();
        self.sort_search_results();
        self.status = format!(
            "ソートキーを{}に切り替え（{}）",
            self.sort_key.label(),
            self.sort_order.label()
        );
    }

    pub fn toggle_sort_order(&mut self) {
        self.sort_order = self.sort_order.toggle();
        self.sort_entries_with_current_key();
        self.sort_search_results();
        self.status = format!(
            "{}を{}でソートしました",
            self.sort_key.label(),
            self.sort_order.label()
        );
    }

    pub fn create_new_folder(&mut self) -> io::Result<()> {
        if self.focus != FocusArea::Right {
            self.status = String::from("フォルダ作成は右ペインで実行してください");
            return Ok(());
        }
        let (folder_name, folder_path) = self.generate_unique_folder_name()?;
        match std::fs::create_dir(&folder_path) {
            Ok(_) => {
                self.status = format!("フォルダ {} を作成しました", folder_name);
                self.last_loaded_dir = None;
                self.refresh()?;
                self.select_entry_by_path(&folder_path);
                self.sync_tree_to_current_dir();
            }
            Err(err) => {
                self.status = format!("フォルダ作成に失敗: {}", err);
            }
        }
        Ok(())
    }

    pub fn copy_selected_entry(&mut self) -> io::Result<()> {
        if self.focus != FocusArea::Right {
            self.status = String::from("コピーは右ペインで実行してください");
            return Ok(());
        }
        let targets = self.collect_selected_entries();
        if targets.is_empty() {
            self.status = String::from("コピー対象が見つかりません");
            return Ok(());
        }
        let mut success_paths = Vec::new();
        let mut errors = Vec::new();
        for entry in targets {
            match self.generate_copy_name(&entry.name, entry.is_dir) {
                Ok(new_name) => {
                    let dest_path = self.current_dir.join(&new_name);
                    match copy_entry_recursive(&entry.path, &dest_path) {
                        Ok(_) => success_paths.push(dest_path),
                        Err(err) => errors.push(format!("{}: {}", entry.name, err)),
                    }
                }
                Err(err) => errors.push(format!("{}: {}", entry.name, err)),
            }
        }
        if !success_paths.is_empty() {
            self.last_loaded_dir = None;
            self.refresh()?;
            if let Some(path) = success_paths.last() {
                self.select_entry_by_path(path);
            }
            self.sync_tree_to_current_dir();
            self.status = format!("{} 件コピーしました", success_paths.len());
        }
        if !errors.is_empty() {
            let msg = errors.join("; ");
            if success_paths.is_empty() {
                self.status = format!("コピーに失敗: {}", msg);
            } else {
                self.status = format!("一部コピーに失敗: {}", msg);
            }
        }
        Ok(())
    }

    pub fn start_move_mode(&mut self) -> io::Result<()> {
        if self.focus != FocusArea::Right {
            self.status = String::from("移動は右ペインで実行してください");
            return Ok(());
        }
        self.move_mode_active = false;
        self.move_targets.clear();
        let targets = self.collect_selected_entries();
        if targets.is_empty() {
            self.status = String::from("移動対象が見つかりません");
            return Ok(());
        }
        self.move_targets = targets.into_iter().map(|entry| entry.path).collect();
        self.move_mode_active = true;
        let count = self.move_targets.len();
        self.status = format!(
            "{} 件を移動待ち: 宛先の '.' を選んで Enter、Esc でキャンセル",
            count
        );
        Ok(())
    }

    pub fn cancel_move_mode(&mut self) {
        if self.move_mode_active {
            self.move_mode_active = false;
            self.move_targets.clear();
            self.status = String::from("移動モードをキャンセルしました");
        }
    }

    pub fn execute_move_to_current_dir(&mut self) -> io::Result<()> {
        if !self.move_mode_active {
            self.status = String::from("移動モードではありません");
            return Ok(());
        }
        if !self.is_current_dir_row_selected() {
            self.status = String::from("宛先の '.' を選択して Enter してください");
            return Ok(());
        }
        if self.move_targets.is_empty() {
            self.move_mode_active = false;
            self.status = String::from("移動対象がありません");
            return Ok(());
        }
        let dest_dir = self.current_dir.clone();
        let mut success = 0usize;
        let mut errors = Vec::new();
        let targets = self.move_targets.clone();
        for source in targets {
            if source.parent().map(|p| p == dest_dir).unwrap_or(false) {
                errors.push(format!(
                    "{}: すでにこのディレクトリに存在します",
                    source.display()
                ));
                continue;
            }
            if dest_dir.starts_with(&source) {
                errors.push(format!(
                    "{}: 自分自身またはその配下には移動できません",
                    source.display()
                ));
                continue;
            }
            let Some(name) = source.file_name().map(|s| s.to_owned()) else {
                errors.push(format!("{}: 名前を取得できません", source.display()));
                continue;
            };
            let dest_path = dest_dir.join(&name);
            if dest_path.exists() {
                errors.push(format!("{}: 移動先に同名が存在します", dest_path.display()));
                continue;
            }
            match stdfs::rename(&source, &dest_path) {
                Ok(_) => success += 1,
                Err(err) => errors.push(format!(
                    "{} -> {}: {}",
                    source.display(),
                    dest_path.display(),
                    err
                )),
            }
        }
        self.move_mode_active = false;
        self.move_targets.clear();
        if success > 0 {
            self.last_loaded_dir = None;
            self.refresh()?;
            self.sync_tree_to_current_dir();
            self.focus_current_dir_row();
            self.status = format!("{success} 件移動しました");
        }
        if !errors.is_empty() {
            let message = errors.join("; ");
            if success > 0 {
                self.status = format!("一部移動に失敗: {}", message);
            } else {
                self.status = format!("移動に失敗: {}", message);
            }
        }
        Ok(())
    }

    pub fn create_new_text_file(&mut self) -> io::Result<()> {
        if self.focus != FocusArea::Right || !self.is_current_dir_row_selected() {
            self.status = String::from("テキストファイル作成は . を選択中のみ実行できます");
            return Ok(());
        }
        let (file_name, file_path) = self.generate_unique_file_name()?;
        match std::fs::File::create(&file_path) {
            Ok(_) => {
                self.status = format!("ファイル {} を作成しました", file_name);
                self.last_loaded_dir = None;
                self.refresh()?;
                self.select_entry_by_path(&file_path);
                self.sync_tree_to_current_dir();
            }
            Err(err) => {
                self.status = format!("ファイル作成に失敗: {}", err);
            }
        }
        Ok(())
    }

    pub fn open_search_modal(&mut self) {
        if self.focus != FocusArea::Right {
            self.status = String::from("検索は右ペインで実行してください");
            return;
        }
        self.search_modal_state = SearchModalState::Input;
        self.search_input = String::from("*");
        self.search_cursor = self.search_input.len();
        self.search_results.clear();
        self.search_selected = 0;
        self.search_scroll_offset = 0;
        self.status = String::from("検索パターンを入力（*可）");
        self.rename_modal_open = false;
        self.favorites_popup_open = false;
    }

    pub fn close_search_modal(&mut self) {
        self.search_modal_state = SearchModalState::Closed;
        self.search_results.clear();
        self.search_scroll_offset = 0;
    }

    pub fn is_search_input_open(&self) -> bool {
        matches!(self.search_modal_state, SearchModalState::Input)
    }

    pub fn is_search_results_open(&self) -> bool {
        matches!(self.search_modal_state, SearchModalState::Results)
    }

    pub fn search_input_text(&self) -> &str {
        &self.search_input
    }

    pub fn search_cursor(&self) -> usize {
        self.search_cursor
    }

    pub fn search_results(&self) -> &[FsEntry] {
        &self.search_results
    }

    pub fn is_row_selected(&self, row: usize) -> bool {
        self.right_selected_rows.contains(&row)
    }

    pub fn is_move_mode_active(&self) -> bool {
        self.move_mode_active
    }

    pub fn search_scroll_offset(&self) -> u16 {
        self.search_scroll_offset
    }

    pub fn search_selected_index(&self) -> Option<usize> {
        if self.search_results.is_empty() {
            None
        } else {
            Some(self.search_selected)
        }
    }

    pub fn search_input_char(&mut self, c: char) {
        if !self.is_search_input_open() {
            return;
        }
        let mut buf = [0u8; 4];
        let encoded = c.encode_utf8(&mut buf);
        self.search_input.insert_str(self.search_cursor, encoded);
        self.search_cursor += encoded.len();
    }

    pub fn search_input_backspace(&mut self) {
        if !self.is_search_input_open() || self.search_cursor == 0 {
            return;
        }
        let prev = prev_char_boundary_in(&self.search_input, self.search_cursor);
        self.search_input
            .replace_range(prev..self.search_cursor, "");
        self.search_cursor = prev;
    }

    pub fn search_move_cursor_left(&mut self) {
        if !self.is_search_input_open() {
            return;
        }
        self.search_cursor = prev_char_boundary_in(&self.search_input, self.search_cursor);
    }

    pub fn search_move_cursor_right(&mut self) {
        if !self.is_search_input_open() {
            return;
        }
        self.search_cursor = next_char_boundary_in(&self.search_input, self.search_cursor);
    }

    pub fn search_results_move_up(&mut self) {
        if !self.is_search_results_open() {
            return;
        }
        if self.search_selected > 0 {
            self.search_selected -= 1;
        }
    }

    pub fn search_results_move_down(&mut self) {
        if !self.is_search_results_open() {
            return;
        }
        if self.search_selected + 1 < self.search_results.len() {
            self.search_selected += 1;
        }
    }

    pub fn scroll_search_results_horizontal(&mut self, to_right: bool) {
        if !self.is_search_results_open() || self.search_results.is_empty() {
            return;
        }
        if to_right {
            self.search_scroll_offset = self.search_scroll_offset.saturating_add(HSCROLL_STEP);
        } else {
            self.search_scroll_offset = self.search_scroll_offset.saturating_sub(HSCROLL_STEP);
        }
    }

    pub fn submit_search(&mut self) -> io::Result<()> {
        if !self.is_search_input_open() {
            return Ok(());
        }
        let pattern = self.search_input.trim();
        if pattern.is_empty() {
            self.status = String::from("検索パターンを入力してください");
            return Ok(());
        }
        match search_files(&self.tree_root.entry.path, pattern) {
            Ok(results) => {
                self.search_results = results;
                self.search_selected = 0;
                self.sort_search_results();
                self.search_modal_state = SearchModalState::Results;
                self.search_scroll_offset = 0;
                self.status = format!("{}件ヒット", self.search_results.len());
            }
            Err(err) => {
                self.status = format!("検索に失敗: {err}");
            }
        }
        Ok(())
    }

    pub fn activate_search_selection(&mut self) -> io::Result<()> {
        if !self.is_search_results_open() || self.search_results.is_empty() {
            self.status = String::from("検索結果がありません");
            return Ok(());
        }
        let entry = self
            .search_results
            .get(self.search_selected)
            .cloned()
            .unwrap();
        let target_dir = if entry.is_dir {
            entry.path.clone()
        } else {
            entry
                .path
                .parent()
                .map(PathBuf::from)
                .unwrap_or(self.current_dir.clone())
        };
        let status = format!("検索結果: {}", entry.path.display());
        self.set_current_directory(target_dir, Some(status))?;
        self.select_entry_by_path(&entry.path);
        self.close_search_modal();
        Ok(())
    }

    pub fn sort_search_results(&mut self) {
        if self.search_results.is_empty() {
            self.search_selected = 0;
            return;
        }
        sort_entries_by_key(&mut self.search_results, self.sort_key);
        if self.sort_order == SortOrder::Desc {
            reverse_groups_for_desc(&mut self.search_results);
        }
        if self.search_selected >= self.search_results.len() {
            self.search_selected = self.search_results.len().saturating_sub(1);
        }
    }

    pub fn open_rename_modal(&mut self) -> io::Result<()> {
        if self.focus != FocusArea::Right {
            self.status = String::from("リネームは右ペインで実行してください");
            return Ok(());
        }
        if self.is_current_dir_row_selected() {
            self.status = String::from("現在のディレクトリはリネームできません");
            return Ok(());
        }
        let Some(idx) = self.entry_index_for_row(self.right_index) else {
            self.status = String::from("リネーム対象が見つかりません");
            return Ok(());
        };
        let Some(entry) = self.entries.get(idx).cloned() else {
            self.status = String::from("リネーム対象が見つかりません");
            return Ok(());
        };
        self.rename_modal_open = true;
        self.favorites_popup_open = false;
        self.rename_input = entry.name.clone();
        self.rename_cursor = self.rename_input.len();
        self.rename_target = Some(entry.path);
        self.status = format!("{} をリネーム中", entry.name);
        Ok(())
    }

    pub fn cancel_rename_modal(&mut self) {
        self.rename_modal_open = false;
        self.rename_target = None;
        self.rename_input.clear();
        self.rename_cursor = 0;
    }

    pub fn submit_rename(&mut self) -> io::Result<()> {
        if !self.rename_modal_open {
            return Ok(());
        }
        let Some(target) = self.rename_target.clone() else {
            self.status = String::from("リネーム対象がありません");
            self.cancel_rename_modal();
            return Ok(());
        };
        let new_name = self.rename_input.trim();
        if new_name.is_empty() {
            self.status = String::from("名称は空にできません");
            return Ok(());
        }
        if new_name.contains('/') {
            self.status = String::from("名称に / は使用できません");
            return Ok(());
        }
        let current_name = target
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if new_name == current_name {
            self.status = String::from("名称は変更されていません");
            self.cancel_rename_modal();
            return Ok(());
        }
        let mut new_path = target.clone();
        new_path.set_file_name(new_name);
        if new_path.exists() {
            self.status = format!("リネーム先に同名が存在します: {}", new_name);
            return Ok(());
        }
        match std::fs::rename(&target, &new_path) {
            Ok(_) => {
                self.status = format!("{} を {} にリネームしました", current_name, new_name);
                self.cancel_rename_modal();
                self.last_loaded_dir = None;
                self.refresh()?;
                self.select_entry_by_path(&new_path);
                self.sync_tree_to_current_dir();
            }
            Err(err) => {
                self.status = format!("リネームに失敗: {}", err);
            }
        }
        Ok(())
    }

    pub fn rename_input_char(&mut self, c: char) {
        if !self.rename_modal_open {
            return;
        }
        let mut buf = [0u8; 4];
        let encoded = c.encode_utf8(&mut buf);
        self.rename_input.insert_str(self.rename_cursor, encoded);
        self.rename_cursor += encoded.len();
    }

    pub fn rename_input_backspace(&mut self) {
        if !self.rename_modal_open || self.rename_cursor == 0 {
            return;
        }
        let prev = prev_char_boundary_in(&self.rename_input, self.rename_cursor);
        self.rename_input
            .replace_range(prev..self.rename_cursor, "");
        self.rename_cursor = prev;
    }

    pub fn rename_move_cursor_left(&mut self) {
        if !self.rename_modal_open {
            return;
        }
        self.rename_cursor = prev_char_boundary_in(&self.rename_input, self.rename_cursor);
    }

    pub fn rename_move_cursor_right(&mut self) {
        if !self.rename_modal_open {
            return;
        }
        self.rename_cursor = next_char_boundary_in(&self.rename_input, self.rename_cursor);
    }

    pub fn is_rename_modal_open(&self) -> bool {
        self.rename_modal_open
    }

    pub fn rename_text(&self) -> &str {
        &self.rename_input
    }

    pub fn rename_cursor(&self) -> usize {
        self.rename_cursor
    }

    pub fn delete_selected_entry(&mut self) -> io::Result<()> {
        if self.focus != FocusArea::Right {
            self.status = String::from("削除は右ペインでのみ実行できます");
            return Ok(());
        }
        let targets = self.collect_selected_entries();
        if targets.is_empty() {
            self.status = String::from("削除対象が見つかりません");
            return Ok(());
        }
        let mut success = 0usize;
        let mut errors = Vec::new();
        for entry in targets {
            let result = if entry.is_dir {
                std::fs::remove_dir_all(&entry.path)
            } else {
                std::fs::remove_file(&entry.path)
            };
            match result {
                Ok(_) => success += 1,
                Err(err) => errors.push(format!("{}: {}", entry.name, err)),
            }
        }
        if success > 0 {
            self.last_loaded_dir = None;
            self.refresh()?;
            self.sync_tree_to_current_dir();
            self.status = format!("{success} 件削除しました");
            self.reset_right_selection_to_current();
        }
        if !errors.is_empty() {
            let message = errors.join("; ");
            if success > 0 {
                self.status = format!("{} 件削除しましたが、いくつか失敗: {}", success, message);
            } else {
                self.status = format!("削除に失敗: {}", message);
            }
        }
        Ok(())
    }

    pub fn favorite_paths(&self) -> &[String] {
        &self.config.favorite
    }

    pub fn history_entries(&self) -> &[PathBuf] {
        &self.history
    }

    pub fn is_favorites_popup_open(&self) -> bool {
        self.favorites_popup_open
    }

    pub fn is_history_popup_open(&self) -> bool {
        self.history_popup_open
    }

    pub fn favorites_popup_visible_rows(&self) -> usize {
        if self.favorite_paths().is_empty() {
            0
        } else {
            self.favorite_popup_visible_rows_internal().max(1)
        }
    }

    pub fn favorites_popup_window(&self) -> (usize, usize) {
        let total = self.favorite_paths().len();
        if total == 0 {
            return (0, 0);
        }
        let visible = self.favorite_popup_visible_rows_internal().min(total);
        let max_start = total.saturating_sub(visible);
        let start = self.favorites_popup_offset.min(max_start);
        let end = (start + visible).min(total);
        (start, end)
    }

    pub fn favorites_popup_selected_visible_index(&self) -> Option<usize> {
        if self.favorite_paths().is_empty() {
            None
        } else {
            let (start, _) = self.favorites_popup_window();
            Some(self.favorites_popup_index.saturating_sub(start))
        }
    }

    pub fn history_popup_window(&self) -> (usize, usize) {
        let total = self.history_entries().len();
        if total == 0 {
            return (0, 0);
        }
        let visible = total.min(HISTORY_POPUP_VISIBLE);
        let max_start = total.saturating_sub(visible);
        let start = self.history_popup_offset.min(max_start);
        (start, start + visible)
    }

    pub fn history_popup_selected_visible_index(&self) -> Option<usize> {
        if self.history_entries().is_empty() {
            None
        } else {
            let (start, _) = self.history_popup_window();
            Some(self.history_popup_index.saturating_sub(start))
        }
    }

    pub fn history_entry_for_display(&self, display_idx: usize) -> Option<&PathBuf> {
        self.history_display_to_actual(display_idx)
            .and_then(|actual| self.history_entries().get(actual))
    }

    pub fn scroll_active_pane_horizontal(&mut self, to_right: bool) {
        match self.focus {
            FocusArea::Left => {
                if to_right {
                    self.left_scroll_offset = self.left_scroll_offset.saturating_add(HSCROLL_STEP);
                } else {
                    self.left_scroll_offset = self.left_scroll_offset.saturating_sub(HSCROLL_STEP);
                }
                self.clamp_left_scroll_offset();
            }
            FocusArea::Right => {
                if to_right {
                    self.right_scroll_offset =
                        self.right_scroll_offset.saturating_add(HSCROLL_STEP);
                } else {
                    self.right_scroll_offset =
                        self.right_scroll_offset.saturating_sub(HSCROLL_STEP);
                }
                self.clamp_right_scroll_offset();
            }
            FocusArea::Path => {}
        }
    }

    pub fn open_favorites_popup(&mut self) {
        if self.favorite_paths().is_empty() {
            self.status = String::from("No favorites registered");
            self.favorites_popup_open = false;
            return;
        }
        self.close_history_popup();
        let current_key = Self::favorite_key_for(&self.current_dir);
        if let Some(idx) = self.favorite_paths().iter().position(|p| p == &current_key) {
            self.favorites_popup_index = idx;
        } else {
            self.favorites_popup_index = 0;
        }
        self.favorites_popup_open = true;
        self.ensure_favorites_popup_invariants();
    }

    pub fn close_favorites_popup(&mut self) {
        self.favorites_popup_open = false;
    }

    pub fn open_history_popup(&mut self) {
        if self.history_entries().is_empty() {
            self.status = String::from("履歴がありません");
            self.history_popup_open = false;
            return;
        }
        self.close_favorites_popup();
        self.history_popup_index = self.history_actual_to_display(self.history_index);
        self.history_popup_offset = 0;
        self.history_popup_open = true;
        self.ensure_history_popup_invariants();
    }

    pub fn close_history_popup(&mut self) {
        self.history_popup_open = false;
    }

    pub fn favorites_popup_move_down(&mut self) {
        let total = self.favorite_paths().len();
        if total == 0 {
            return;
        }
        if self.favorites_popup_index + 1 < total {
            self.favorites_popup_index += 1;
            self.ensure_favorites_selection_visible();
        }
    }

    pub fn history_popup_move_down(&mut self) {
        let total = self.history_entries().len();
        if total == 0 {
            return;
        }
        if self.history_popup_index + 1 < total {
            self.history_popup_index += 1;
            self.ensure_history_selection_visible();
        }
    }

    pub fn favorites_popup_move_up(&mut self) {
        if self.favorites_popup_index > 0 {
            self.favorites_popup_index -= 1;
            self.ensure_favorites_selection_visible();
        }
    }

    pub fn history_popup_move_up(&mut self) {
        if self.history_popup_index > 0 {
            self.history_popup_index -= 1;
            self.ensure_history_selection_visible();
        }
    }

    pub fn activate_favorites_popup_selection(&mut self) -> io::Result<()> {
        if self.favorite_paths().is_empty() {
            self.status = String::from("No favorites registered");
            self.close_favorites_popup();
            return Ok(());
        }
        let idx = self
            .favorites_popup_index
            .min(self.favorite_paths().len().saturating_sub(1));
        let target_str = self.favorite_paths()[idx].clone();
        let target_path = PathBuf::from(&target_str);
        match std::fs::metadata(&target_path) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                self.status = format!("Favorite is not a directory: {target_str}");
                self.close_favorites_popup();
                return Ok(());
            }
            Err(err) => {
                self.status = format!("Favorite unavailable: {err}");
                self.close_favorites_popup();
                return Ok(());
            }
        }
        self.close_favorites_popup();
        self.set_current_directory(target_path, Some(format!("Opened favorite {target_str}")))
    }

    pub fn remove_selected_favorite(&mut self) {
        if self.favorite_paths().is_empty() {
            self.status = String::from("お気に入りがありません");
            self.close_favorites_popup();
            return;
        }
        let idx = self
            .favorites_popup_index
            .min(self.favorite_paths().len().saturating_sub(1));
        let target = self.favorite_paths()[idx].clone();
        let removed = self.config.remove_favorite(&target);
        let message = if removed {
            match self.config.save(&self.config_path) {
                Ok(()) => format!("お気に入りを削除: {target}"),
                Err(err) => format!("お気に入り削除の保存に失敗: {err}"),
            }
        } else {
            String::from("削除対象のお気に入りが見つかりません")
        };
        self.update_favorite_flag();
        self.ensure_favorites_popup_invariants();
        if self.favorite_paths().is_empty() {
            self.close_favorites_popup();
        }
        self.status = message;
    }

    pub fn activate_history_popup_selection(&mut self) -> io::Result<()> {
        if self.history_entries().is_empty() {
            self.status = String::from("履歴がありません");
            self.close_history_popup();
            return Ok(());
        }
        let idx = self
            .history_popup_index
            .min(self.history_entries().len().saturating_sub(1));
        if let Some(actual_idx) = self.history_display_to_actual(idx)
            && let Some(target) = self.history_entries().get(actual_idx).cloned()
        {
            if !directory_exists(&target) {
                self.status = String::from("存在しない履歴エントリです");
                self.history.remove(actual_idx);
                if self.history_index >= self.history.len() {
                    self.history_index = self.history.len().saturating_sub(1);
                }
                self.ensure_history_popup_invariants();
                return Ok(());
            }
            self.close_history_popup();
            self.history_index = actual_idx;
            self.set_current_directory_internal(
                target.clone(),
                Some(format!("履歴へ移動: {}", target.display())),
                true,
                false,
            )?;
        } else {
            self.status = String::from("履歴エントリが見つかりません");
            self.close_history_popup();
        }
        Ok(())
    }

    fn selected_entry(&self) -> Option<FsEntry> {
        match self.focus {
            FocusArea::Left => self
                .visible_dirs
                .get(self.left_index)
                .and_then(|path| self.node_at_path(path))
                .map(|node| node.entry.clone()),
            FocusArea::Right => self
                .entry_index_for_row(self.right_index)
                .and_then(|idx| self.entries.get(idx).cloned()),
            FocusArea::Path => None,
        }
    }

    fn set_current_directory_internal(
        &mut self,
        path: PathBuf,
        status: Option<String>,
        sync_tree: bool,
        record_history: bool,
    ) -> io::Result<()> {
        if self.current_dir == path {
            if let Some(status) = status {
                self.status = status;
            }
            return Ok(());
        }
        self.current_dir = path;
        self.close_favorites_popup();
        self.right_index = 0;
        self.last_loaded_dir = None;
        self.refresh()?;
        if let Some(status) = status {
            self.status = status;
        }
        if sync_tree {
            self.sync_tree_to_current_dir();
        }
        self.path_input = self.current_dir.display().to_string();
        self.path_cursor = self.path_input.len();
        self.path_cursor_on_star = false;
        self.update_favorite_flag();
        self.focus_current_dir_row();
        if record_history {
            let path_clone = self.current_dir.clone();
            self.record_history_entry(&path_clone);
        }
        Ok(())
    }

    pub fn expand_selected_dir(&mut self) -> io::Result<()> {
        if self.focus != FocusArea::Left {
            return Ok(());
        }
        let path = match self.visible_dirs.get(self.left_index) {
            Some(p) => p.clone(),
            None => return Ok(()),
        };
        let mut status_msg = None;
        if let Some(node) = self.node_mut_at_path(&path)
            && !node.expanded
        {
            if node.children.is_none() {
                let node_name = node.entry.name.clone();
                if let Err(err) = load_children(node) {
                    node.children = Some(Vec::new());
                    node.has_children = false;
                    status_msg = Some(format!("Failed to read {}: {}", node_name, err));
                }
            }
            node.expanded = true;
            self.rebuild_visible_dirs();
        }
        if let Some(msg) = status_msg {
            self.status = msg;
        }
        Ok(())
    }

    pub fn collapse_selected_dir(&mut self) -> io::Result<()> {
        if self.focus != FocusArea::Left {
            return Ok(());
        }
        let mut selection_changed = false;
        if let Some(path) = self.visible_dirs.get(self.left_index).cloned()
            && let Some(node) = self.node_mut_at_path(&path)
        {
            if node.expanded {
                node.expanded = false;
                self.rebuild_visible_dirs();
            } else if let Some(parent_path) = parent_path(&path)
                && let Some(parent_index) = self
                    .visible_dirs
                    .iter()
                    .position(|p| p.as_slice() == parent_path.as_slice())
            {
                self.left_index = parent_index;
                selection_changed = true;
            }
        }
        if selection_changed {
            self.update_current_dir_from_left_selection()?;
        }
        Ok(())
    }

    fn rebuild_visible_dirs(&mut self) {
        let mut acc = Vec::new();
        fn walk(node: &DirNode, path: &mut Vec<usize>, acc: &mut Vec<Vec<usize>>) {
            acc.push(path.clone());
            if node.expanded
                && let Some(children) = &node.children
            {
                for (idx, child) in children.iter().enumerate() {
                    path.push(idx);
                    walk(child, path, acc);
                    path.pop();
                }
            }
        }
        let mut current_path = Vec::new();
        walk(&self.tree_root, &mut current_path, &mut acc);
        self.visible_dirs = acc;
        if self.visible_dirs.is_empty() {
            self.left_index = 0;
        } else if self.left_index >= self.visible_dirs.len() {
            self.left_index = self.visible_dirs.len() - 1;
        }
        self.update_left_max_label_width();
        self.clamp_left_scroll_offset();
    }

    fn node_at_path(&self, path: &[usize]) -> Option<&DirNode> {
        let mut node = &self.tree_root;
        for &idx in path {
            node = node.children.as_ref()?.get(idx)?;
        }
        Some(node)
    }

    fn node_mut_at_path(&mut self, path: &[usize]) -> Option<&mut DirNode> {
        let mut node = &mut self.tree_root;
        for &idx in path {
            node = node.children.as_mut()?.get_mut(idx)?;
        }
        Some(node)
    }

    fn update_left_max_label_width(&mut self) {
        let mut max_width = 0usize;
        for path in &self.visible_dirs {
            if let Some(node) = self.node_at_path(path) {
                let depth = path.len();
                let width = Self::left_label_display_width(node, depth);
                if width > max_width {
                    max_width = width;
                }
            }
        }
        self.left_max_label_width = max_width;
    }

    fn left_label_display_width(node: &DirNode, depth: usize) -> usize {
        let indent_width = depth * 2;
        let marker_width = 3;
        let mut name = node.entry.name.clone();
        if !name.ends_with('/') {
            name.push('/');
        }
        let name_width = UnicodeWidthStr::width(name.as_str());
        indent_width + marker_width + 1 + name_width
    }

    fn update_right_name_width(&mut self) {
        let mut max_width = UnicodeWidthStr::width(".");
        for entry in &self.entries {
            let mut name = entry.name.clone();
            if entry.is_dir && !name.ends_with('/') {
                name.push('/');
            }
            let width = UnicodeWidthStr::width(name.as_str());
            if width > max_width {
                max_width = width;
            }
        }
        self.right_max_name_width = max_width;
    }

    fn clamp_left_scroll_offset(&mut self) {
        let max_offset = self.max_left_scroll_offset();
        if self.left_scroll_offset > max_offset {
            self.left_scroll_offset = max_offset;
        }
    }

    fn clamp_right_scroll_offset(&mut self) {
        let max_offset = self.max_right_scroll_offset();
        if self.right_scroll_offset > max_offset {
            self.right_scroll_offset = max_offset;
        }
    }

    fn max_left_scroll_offset(&self) -> u16 {
        let visible = self.current_left_visible_width();
        let max_offset = self.left_max_label_width.saturating_sub(visible);
        max_offset.min(u16::MAX as usize) as u16
    }

    fn max_right_scroll_offset(&self) -> u16 {
        let max_offset = self.right_max_name_width.saturating_sub(NAME_COLUMN_WIDTH);
        max_offset.min(u16::MAX as usize) as u16
    }

    fn current_left_visible_width(&self) -> usize {
        if let Ok((width, _)) = terminal::size() {
            let mut chunk = (width as usize * 30) / 100;
            if chunk == 0 && width > 0 {
                chunk = 1;
            }
            chunk.saturating_sub(2)
        } else {
            0
        }
    }

    pub fn sync_tree_to_current_dir(&mut self) {
        let current = self.current_dir.clone();
        let visible = self.ensure_path_visible(&current);
        self.rebuild_visible_dirs();
        if visible
            && let Some(index) = self.visible_dirs.iter().position(|path| {
                self.node_at_path(path)
                    .map(|n| n.entry.path == current)
                    .unwrap_or(false)
            })
        {
            self.left_index = index;
        }
    }

    fn ensure_path_visible(&mut self, target: &Path) -> bool {
        use std::path::Component;

        let root_path = self.tree_root.entry.path.clone();
        if target == root_path {
            return true;
        }
        if !target.starts_with(&root_path) {
            if let Err(err) = self.reset_tree_root(target.to_path_buf()) {
                self.status = format!("Failed to reset tree root: {}", err);
                return false;
            }
            return true;
        }

        let mut node = &mut self.tree_root;
        node.expanded = true;
        let relative = match target.strip_prefix(&root_path) {
            Ok(path) => path,
            Err(_) => return false,
        };

        for component in relative.components() {
            let os_str = match component {
                Component::Normal(name) => name,
                Component::CurDir | Component::RootDir => continue,
                Component::ParentDir => return false,
                Component::Prefix(_) => continue,
            };
            if let Err(err) = load_children(node) {
                self.status = format!("Failed to read {}: {}", node.entry.name, err);
                return false;
            }
            let children = match node.children.as_mut() {
                Some(children) => children,
                None => return false,
            };
            let mut next_path = node.entry.path.clone();
            next_path.push(os_str);
            let Some(idx) = children
                .iter()
                .position(|child| child.entry.path == next_path)
            else {
                return false;
            };
            node = &mut children[idx];
            node.expanded = true;
        }
        true
    }

    fn current_left_path(&self) -> Option<PathBuf> {
        self.visible_dirs
            .get(self.left_index)
            .and_then(|path| self.node_at_path(path))
            .map(|node| node.entry.path.clone())
    }

    fn update_current_dir_from_left_selection(&mut self) -> io::Result<()> {
        if let Some(path) = self.current_left_path() {
            self.set_current_directory_internal(path, None, false, true)?;
        }
        Ok(())
    }

    fn reset_tree_root(&mut self, path: PathBuf) -> io::Result<()> {
        let entry = fs::entry_from_path(path)?;
        self.tree_root = DirNode::new(entry);
        self.left_index = 0;
        self.visible_dirs.clear();
        Ok(())
    }

    fn total_right_items(&self) -> usize {
        self.entries.len().saturating_add(1)
    }

    fn clamp_right_index(&mut self) {
        let total_items = self.total_right_items();
        let mut changed = false;
        if total_items == 0 {
            self.right_index = 0;
            changed = true;
        } else if self.right_index >= total_items {
            self.right_index = total_items - 1;
            changed = true;
        }
        self.right_selected_rows.retain(|idx| *idx < total_items);
        if changed || self.right_selected_rows.is_empty() {
            self.reset_right_selection_to_current();
        }
    }

    fn sort_entries_with_current_key(&mut self) {
        sort_entries_by_key(&mut self.entries, self.sort_key);
        if self.sort_order == SortOrder::Desc {
            reverse_groups_for_desc(&mut self.entries);
        }
        self.clamp_right_index();
        self.reset_right_selection_to_current();
    }

    pub fn select_entry_by_path(&mut self, path: &Path) {
        if let Some(idx) = self.entries.iter().position(|entry| entry.path == path) {
            self.right_index = idx + 1;
        } else {
            self.clamp_right_index();
        }
        self.reset_right_selection_to_current();
    }

    pub fn refresh_and_select(&mut self, path: &Path) -> io::Result<()> {
        self.last_loaded_dir = None;
        self.refresh()?;
        self.select_entry_by_path(path);
        Ok(())
    }

    pub fn is_current_dir_row_selected(&self) -> bool {
        self.focus == FocusArea::Right && self.right_index == 0
    }

    fn on_focus_changed(&mut self) {
        match self.focus {
            FocusArea::Path => {
                self.path_input = self.current_dir.display().to_string();
                self.path_cursor = self.path_input.len();
                self.path_cursor_on_star = false;
            }
            FocusArea::Right => {
                self.reset_right_selection_to_current();
            }
            FocusArea::Left => {}
        }
        if self.focus != FocusArea::Path {
            self.close_favorites_popup();
            self.close_history_popup();
        }
    }

    pub fn path_input_char(&mut self, c: char) {
        if self.path_cursor_on_star {
            self.path_cursor_on_star = false;
            self.path_cursor = 0;
        }
        let mut buf = [0u8; 4];
        let encoded = c.encode_utf8(&mut buf);
        self.path_input.insert_str(self.path_cursor, encoded);
        self.path_cursor += encoded.len();
    }

    pub fn path_input_backspace(&mut self) {
        if self.path_cursor_on_star || self.path_cursor == 0 {
            return;
        }
        let prev = self.prev_char_boundary(self.path_cursor);
        self.path_input.replace_range(prev..self.path_cursor, "");
        self.path_cursor = prev;
    }

    pub fn apply_path_input(&mut self) -> io::Result<()> {
        let trimmed = self.path_input.trim();
        if trimmed.is_empty() {
            self.status = String::from("Path cannot be empty");
            return Ok(());
        }
        let trimmed_owned = trimmed.to_string();
        let mut path = PathBuf::from(&trimmed_owned);
        if !path.is_absolute() {
            path = self.current_dir.join(path);
        }
        match std::fs::metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    self.status = format!("Not a directory: {}", path.display());
                    return Ok(());
                }
            }
            Err(err) => {
                self.status = format!("Cannot open {}: {}", path.display(), err);
                return Ok(());
            }
        }
        self.set_current_directory(path.clone(), Some(format!("Jumped to {}", trimmed_owned)))?;
        self.reset_tree_root(self.current_dir.clone())?;
        self.sync_tree_to_current_dir();
        Ok(())
    }

    pub fn move_path_cursor_left(&mut self) {
        if self.path_cursor_on_star {
            return;
        }
        if self.path_cursor == 0 {
            self.path_cursor_on_star = true;
            return;
        }
        self.path_cursor = self.prev_char_boundary(self.path_cursor);
    }

    pub fn move_path_cursor_right(&mut self) {
        if self.path_cursor_on_star {
            self.path_cursor_on_star = false;
            if !self.path_input.is_empty() {
                self.path_cursor = self.next_char_boundary(0);
            } else {
                self.path_cursor = 0;
            }
            return;
        }
        if self.path_cursor >= self.path_input.len() {
            return;
        }
        self.path_cursor = self.next_char_boundary(self.path_cursor);
    }

    fn prev_char_boundary(&self, idx: usize) -> usize {
        prev_char_boundary_in(&self.path_input, idx)
    }

    fn next_char_boundary(&self, idx: usize) -> usize {
        next_char_boundary_in(&self.path_input, idx)
    }

    pub fn toggle_favorite(&mut self) {
        let target_path = if self.focus == FocusArea::Path {
            self.display_path_candidate()
        } else {
            Some(self.current_dir.clone())
        };
        let Some(target) = target_path else {
            self.status = String::from("Invalid path for favorites");
            return;
        };
        let key = Self::favorite_key_for(&target);
        let currently_fav = self.config.is_favorite(&key);
        let result = if currently_fav {
            if self.config.remove_favorite(&key) {
                self.config.save(&self.config_path)
            } else {
                Ok(())
            }
        } else if self.config.add_favorite(&key) {
            self.config.save(&self.config_path)
        } else {
            Ok(())
        };
        match result {
            Ok(()) => {
                self.status = if currently_fav {
                    format!("Unfavorited {}", target.display())
                } else {
                    format!("Favorited {}", target.display())
                };
            }
            Err(err) => {
                self.status = format!("Failed to save favorites: {err}");
            }
        }
        self.update_favorite_flag();
        self.ensure_favorites_popup_invariants();
    }

    pub fn display_path_is_favorite(&self) -> bool {
        if self.focus == FocusArea::Path {
            if let Some(path) = self.display_path_candidate() {
                let key = Self::favorite_key_for(&path);
                return self.config.is_favorite(&key);
            }
            false
        } else {
            self.favorite_current
        }
    }

    fn display_path_candidate(&self) -> Option<PathBuf> {
        let trimmed = self.path_input.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut path = PathBuf::from(trimmed);
        if !path.is_absolute() {
            path = self.current_dir.join(path);
        }
        Some(path)
    }
}

fn parent_path(path: &[usize]) -> Option<Vec<usize>> {
    if path.is_empty() {
        None
    } else {
        let mut parent = path.to_vec();
        parent.pop();
        Some(parent)
    }
}

fn load_children(node: &mut DirNode) -> io::Result<()> {
    if node.children.is_none() {
        let entries = fs::read_directory(&node.entry.path)?;
        let dirs: Vec<DirNode> = entries
            .into_iter()
            .filter(|entry| entry.is_dir)
            .map(DirNode::new)
            .collect();
        let has_children = !dirs.is_empty();
        node.has_children = has_children;
        node.children = Some(dirs);
    }
    Ok(())
}

fn detect_child_directories(path: &Path) -> bool {
    if let Ok(read_dir) = std::fs::read_dir(path) {
        for entry in read_dir.flatten() {
            if let Ok(file_type) = entry.file_type()
                && file_type.is_dir()
            {
                return true;
            }
        }
    }
    false
}

fn directory_exists(path: &Path) -> bool {
    stdfs::metadata(path)
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
}

fn search_files(root: &Path, pattern: &str) -> io::Result<Vec<FsEntry>> {
    let mut stack = vec![root.to_path_buf()];
    let mut matches = Vec::new();
    let pattern = if pattern.is_empty() {
        "*".to_string()
    } else {
        pattern.to_string()
    };
    while let Some(dir) = stack.pop() {
        let read_dir = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            let Ok(fs_entry) = fs::entry_from_path(path.clone()) else {
                continue;
            };
            if wildcard_match(&fs_entry.name, &pattern) {
                matches.push(fs_entry.clone());
            }
            if fs_entry.is_dir
                && stdfs::symlink_metadata(&fs_entry.path)
                    .map(|meta| meta.file_type().is_dir() && !meta.file_type().is_symlink())
                    .unwrap_or(false)
            {
                stack.push(fs_entry.path);
            }
        }
    }
    Ok(matches)
}

fn wildcard_match(text: &str, pattern: &str) -> bool {
    let text_lower = text.to_lowercase();
    let pattern_lower = pattern.to_lowercase();
    let text_chars: Vec<char> = text_lower.chars().collect();
    let pattern_chars: Vec<char> = pattern_lower.chars().collect();
    let (mut ti, mut pi) = (0usize, 0usize);
    let mut star_idx = None;
    let mut match_idx = 0usize;
    while ti < text_chars.len() {
        if pi < pattern_chars.len()
            && (pattern_chars[pi] == text_chars[ti] || pattern_chars[pi] == '?')
        {
            ti += 1;
            pi += 1;
        } else if pi < pattern_chars.len() && pattern_chars[pi] == '*' {
            star_idx = Some(pi);
            pi += 1;
            match_idx = ti;
        } else if let Some(star) = star_idx {
            pi = star + 1;
            match_idx += 1;
            ti = match_idx;
        } else {
            return false;
        }
    }
    while pi < pattern_chars.len() && pattern_chars[pi] == '*' {
        pi += 1;
    }
    pi == pattern_chars.len()
}

fn prev_char_boundary_in(text: &str, idx: usize) -> usize {
    if idx == 0 {
        return 0;
    }
    let mut i = idx.min(text.len()).saturating_sub(1);
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    if text.is_char_boundary(i) { i } else { 0 }
}

fn next_char_boundary_in(text: &str, idx: usize) -> usize {
    if idx >= text.len() {
        return text.len();
    }
    let mut i = idx + 1;
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i.min(text.len())
}

impl App {
    fn update_favorite_flag(&mut self) {
        let key = Self::favorite_key_for(&self.current_dir);
        self.favorite_current = self.config.is_favorite(&key);
    }

    fn favorite_popup_visible_rows_internal(&self) -> usize {
        if self.favorite_paths().is_empty() {
            0
        } else {
            self.favorite_paths().len().min(FAVORITES_DROPDOWN_VISIBLE)
        }
    }

    fn ensure_favorites_selection_visible(&mut self) {
        let total = self.favorite_paths().len();
        if total == 0 {
            self.favorites_popup_offset = 0;
            self.favorites_popup_index = 0;
            return;
        }
        let visible = self.favorite_popup_visible_rows_internal().max(1);
        if self.favorites_popup_index < self.favorites_popup_offset {
            self.favorites_popup_offset = self.favorites_popup_index;
        } else {
            let end = self.favorites_popup_offset + visible;
            if self.favorites_popup_index >= end {
                self.favorites_popup_offset = self.favorites_popup_index + 1 - visible;
            }
        }
    }

    fn ensure_favorites_popup_invariants(&mut self) {
        let total = self.favorite_paths().len();
        if total == 0 {
            self.favorites_popup_index = 0;
            self.favorites_popup_offset = 0;
            return;
        }
        if self.favorites_popup_index >= total {
            self.favorites_popup_index = total - 1;
        }
        self.ensure_favorites_selection_visible();
    }

    fn ensure_history_selection_visible(&mut self) {
        let total = self.history_entries().len();
        if total == 0 {
            self.history_popup_offset = 0;
            self.history_popup_index = 0;
            return;
        }
        let visible = HISTORY_POPUP_VISIBLE.max(1);
        if self.history_popup_index < self.history_popup_offset {
            self.history_popup_offset = self.history_popup_index;
            return;
        }
        let end = self.history_popup_offset + visible;
        if self.history_popup_index >= end {
            self.history_popup_offset = self.history_popup_index + 1 - visible;
        }
    }

    fn ensure_history_popup_invariants(&mut self) {
        let total = self.history_entries().len();
        if total == 0 {
            self.history_popup_index = 0;
            self.history_popup_offset = 0;
            self.history_popup_open = false;
            return;
        }
        if self.history_popup_index >= total {
            self.history_popup_index = total - 1;
        }
        self.ensure_history_selection_visible();
    }

    fn favorite_key_for(path: &Path) -> String {
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }

    fn entry_index_for_row(&self, row: usize) -> Option<usize> {
        row.checked_sub(1)
    }

    fn focus_current_dir_row(&mut self) {
        self.right_index = 0;
        self.reset_right_selection_to_current();
    }

    fn record_history_entry(&mut self, path: &Path) {
        if self.history.is_empty() {
            self.history.push(path.to_path_buf());
            self.history_index = 0;
            return;
        }
        if self
            .history
            .get(self.history_index)
            .is_some_and(|current| current == path)
        {
            return;
        }
        if self.history_index + 1 < self.history.len() {
            self.history.truncate(self.history_index + 1);
        }
        self.history.push(path.to_path_buf());
        if self.history.len() > HISTORY_LIMIT {
            let overflow = self.history.len() - HISTORY_LIMIT;
            self.history.drain(0..overflow);
        }
        self.history_index = self.history.len().saturating_sub(1);
    }

    fn history_display_to_actual(&self, display_idx: usize) -> Option<usize> {
        self.history.len().checked_sub(display_idx + 1)
    }

    fn history_actual_to_display(&self, actual_idx: usize) -> usize {
        let total = self.history.len();
        if total == 0 || actual_idx >= total {
            0
        } else {
            total - actual_idx - 1
        }
    }

    fn collect_selected_entries(&self) -> Vec<FsEntry> {
        self.right_selected_rows
            .iter()
            .copied()
            .filter(|&row| row > 0)
            .filter_map(|row| {
                self.entry_index_for_row(row)
                    .and_then(|idx| self.entries.get(idx).cloned())
            })
            .collect()
    }

    fn generate_copy_name(&self, original: &str, is_dir: bool) -> io::Result<String> {
        let base = if original.is_empty() {
            if is_dir {
                "NewFolder".to_string()
            } else {
                "NewFile".to_string()
            }
        } else {
            original.to_string()
        };
        for counter in 1..=10_000 {
            let candidate = build_copy_name(&base, counter, is_dir);
            let candidate_path = self.current_dir.join(&candidate);
            if !candidate_path.exists() {
                return Ok(candidate);
            }
        }
        Err(io::Error::other("コピー名を生成できませんでした"))
    }

    fn reset_right_selection_to_current(&mut self) {
        self.right_selected_rows.clear();
        self.right_selected_rows.insert(self.right_index);
        self.right_selection_anchor = Some(self.right_index);
    }

    fn ensure_right_anchor(&mut self) {
        if self.right_selection_anchor.is_none() {
            self.right_selection_anchor = Some(self.right_index);
        }
        if self.right_selected_rows.is_empty() {
            self.right_selected_rows.insert(self.right_index);
        }
    }

    fn update_right_selection_range(&mut self) {
        if let Some(anchor) = self.right_selection_anchor {
            self.right_selected_rows.clear();
            let start = anchor.min(self.right_index);
            let end = anchor.max(self.right_index);
            for idx in start..=end {
                self.right_selected_rows.insert(idx);
            }
        } else {
            self.reset_right_selection_to_current();
        }
    }

    fn generate_unique_folder_name(&self) -> io::Result<(String, PathBuf)> {
        let base = "NewFolder";
        let base_path = self.current_dir.join(base);
        if !base_path.exists() {
            return Ok((base.to_string(), base_path));
        }
        for counter in 1..=10_000 {
            let name = format!("{base} ({counter})");
            let candidate = self.current_dir.join(&name);
            if !candidate.exists() {
                return Ok((name, candidate));
            }
        }
        Err(io::Error::other(
            "ユニークなフォルダ名を生成できませんでした",
        ))
    }

    fn generate_unique_file_name(&self) -> io::Result<(String, PathBuf)> {
        generate_default_file_name(&self.current_dir)
    }
}

fn sort_entries_by_key(entries: &mut [FsEntry], key: SortKey) {
    match key {
        SortKey::Name => fs::sort_entries(entries),
        SortKey::Modified => entries.sort_by(compare_modified_entries),
    }
}

fn compare_modified_entries(a: &FsEntry, b: &FsEntry) -> Ordering {
    match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => {
            let order = match (&a.modified, &b.modified) {
                (Some(ma), Some(mb)) => mb.cmp(ma),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            };
            if order == Ordering::Equal {
                compare_names_case_insensitive(a, b)
            } else {
                order
            }
        }
    }
}

fn compare_names_case_insensitive(a: &FsEntry, b: &FsEntry) -> Ordering {
    let lower = a.name.to_lowercase().cmp(&b.name.to_lowercase());
    if lower == Ordering::Equal {
        a.name.cmp(&b.name)
    } else {
        lower
    }
}

fn reverse_groups_for_desc(entries: &mut [FsEntry]) {
    let dir_end = entries
        .iter()
        .position(|entry| !entry.is_dir)
        .unwrap_or(entries.len());
    entries[..dir_end].reverse();
    entries[dir_end..].reverse();
}

fn generate_default_file_name(dir: &Path) -> io::Result<(String, PathBuf)> {
    let base = "NewFile";
    let extension = "txt";
    let base_name = format!("{base}.{extension}");
    let base_path = dir.join(&base_name);
    if !base_path.exists() {
        return Ok((base_name, base_path));
    }
    for counter in 1..=10_000 {
        let name = format!("{base} ({counter}).{extension}");
        let candidate = dir.join(&name);
        if !candidate.exists() {
            return Ok((name, candidate));
        }
    }
    Err(io::Error::other(
        "ユニークなファイル名を生成できませんでした",
    ))
}

fn build_copy_name(original: &str, counter: usize, is_dir: bool) -> String {
    if is_dir {
        return format!("{original} ({counter})");
    }
    let path = Path::new(original);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let ext = path.extension().map(|s| s.to_string_lossy().into_owned());
    match (stem, ext) {
        (Some(stem), Some(ext)) if !stem.is_empty() => format!("{stem} ({counter}).{ext}"),
        _ => format!("{original} ({counter})"),
    }
}

fn copy_entry_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    let metadata = stdfs::symlink_metadata(src)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        stdfs::create_dir(dst)?;
        for entry in stdfs::read_dir(src)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            copy_entry_recursive(&child_src, &child_dst)?;
        }
        Ok(())
    } else {
        stdfs::copy(src, dst)?;
        Ok(())
    }
}
