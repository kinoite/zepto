use std::{
    io::{self, stdout},
    fs,
    env,
    hash::{Hasher, DefaultHasher, Hash},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{
        enable_raw_mode, disable_raw_mode,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
    execute,
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap, BorderType},
    text::Span,
};

mod config;

#[derive(PartialEq)]
enum ApplicationMode {
    Editing,
    Help,
    PromptSave,
}

#[derive(PartialEq)]
enum InputMode {
    Normal,
    Insert,
}

struct Editor<B: Backend> {
    buffer: Vec<String>,
    cursor_x: usize,
    cursor_y: usize,
    scroll_x: usize,
    scroll_y: usize,
    original_buffer_hash: u64,
    filename: Option<String>,
    application_mode: ApplicationMode,
    input_mode: InputMode,
    vim_enabled: bool,
    status_message: String,
    prompt_message: String,
    config: config::Config,
    clipboard: String,
    selection_start: Option<(usize, usize)>,
    selection_end: Option<(usize, usize)>,
    _phantom: std::marker::PhantomData<B>,
}

impl<B: Backend> Editor<B> {
    fn new_with_backend(config: config::Config) -> Self {
        let vim_enabled = config.editor_behavior.vim;
        let initial_input_mode = if vim_enabled { InputMode::Normal } else { InputMode::Insert };
        let initial_status_message = if vim_enabled {
            "-- NORMAL --".to_string()
        } else {
            "Ctrl+X Exit | Ctrl+W Save | Ctrl+H Help".to_string()
        };

        Editor {
            buffer: vec![String::new()],
            cursor_x: 0,
            cursor_y: 0,
            scroll_x: 0,
            scroll_y: 0,
            original_buffer_hash: Self::hash_buffer(&vec![String::new()]),
            filename: None,
            application_mode: ApplicationMode::Editing,
            input_mode: initial_input_mode,
            vim_enabled,
            status_message: initial_status_message,
            prompt_message: String::new(),
            config,
            clipboard: String::new(),
            selection_start: None,
            selection_end: None,
            _phantom: std::marker::PhantomData,
        }
    }

    fn hash_buffer(buffer: &[String]) -> u64 {
        let mut s = DefaultHasher::new();
        for line in buffer {
            line.hash(&mut s);
        }
        s.finish()
    }

    fn is_dirty(&self) -> bool {
        Self::hash_buffer(&self.buffer) != self.original_buffer_hash
    }

    fn open_file(&mut self, path: &str) -> io::Result<()> {
        let content = fs::read_to_string(path)?;
        self.buffer = content.lines().map(|s| s.to_string()).collect();
        if self.buffer.is_empty() {
            self.buffer.push(String::new());
        }
        self.filename = Some(path.to_string());
        self.original_buffer_hash = Self::hash_buffer(&self.buffer);
        if !self.vim_enabled {
            self.status_message = format!("Opened: {}", path);
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.scroll_x = 0;
        self.scroll_y = 0;
        self.clear_selection();
        Ok(())
    }

    fn save_file(&mut self) -> io::Result<()> {
        if let Some(filename) = &self.filename {
            let content = self.buffer.join("\n");
            fs::write(filename, content)?;
            self.original_buffer_hash = Self::hash_buffer(&self.buffer);
            self.status_message = format!("Saved {} lines to {}", self.buffer.len(), filename);
            Ok(())
        } else {
            self.status_message = "No filename. Cannot save. (Implement :w <filename>)".to_string();
            Err(io::ErrorKind::Other.into())
        }
    }

    fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
    }

    fn get_normalized_selection(&self) -> Option<((usize, usize), (usize, usize))> {
        match (self.selection_start, self.selection_end) {
            (Some(start), Some(end)) => {
                if start.0 < end.0 || (start.0 == end.0 && start.1 <= end.1) {
                    Some((start, end))
                } else {
                    Some((end, start))
                }
            },
            _ => None,
        }
    }

    fn delete_selected_text(&mut self, editor_content_area: Rect) {
        if let Some(((start_row, start_col), (end_row, end_col))) = self.get_normalized_selection() {
            if start_row == end_row {
                self.buffer[start_row].replace_range(start_col..end_col, "");
            } else {
                let mut new_line = self.buffer[start_row][..start_col].to_string();
                new_line.push_str(&self.buffer[end_row][end_col..]);
                self.buffer.splice(start_row..=end_row, [new_line]);
            }
            self.cursor_y = start_row;
            self.cursor_x = start_col;
            self.clear_selection();
            self.ensure_cursor_in_view(
                editor_content_area,
                self.config.main_section.line_numbers.enabled,
                self.config.main_section.line_numbers.gutter_width
            );
        }
    }

    fn ensure_cursor_in_view(&mut self, editor_content_area: Rect, line_numbers_enabled: bool, gutter_width: u16) {
        let line_numbers_gutter_width = if line_numbers_enabled {
            gutter_width + 1
        } else {
            0
        };
        let effective_width = editor_content_area.width.saturating_sub(2).saturating_sub(line_numbers_gutter_width) as usize;

        let visible_height = editor_content_area.height.saturating_sub(2) as usize;

        if self.cursor_y < self.scroll_y {
            self.scroll_y = self.cursor_y;
        } else if self.cursor_y >= self.scroll_y + visible_height {
            self.scroll_y = self.cursor_y - visible_height + 1;
        }

        if self.cursor_x < self.scroll_x {
            self.scroll_x = self.cursor_x;
        } else if self.cursor_x >= self.scroll_x + effective_width {
            self.scroll_x = self.cursor_x - effective_width + 1;
        }

        self.scroll_y = self.scroll_y.min(self.buffer.len().saturating_sub(1).max(0));

        if self.cursor_y < self.buffer.len() {
             self.scroll_x = self.scroll_x.min(self.buffer[self.cursor_y].len().saturating_sub(effective_width).max(0));
        } else {
            self.scroll_x = 0;
        }
        self.cursor_x = self.cursor_x.min(self.buffer[self.cursor_y].len());
    }

    fn update_selection_on_move(&mut self, shift_pressed: bool) {
        if shift_pressed {
            if self.selection_start.is_none() {
                self.selection_start = Some((self.cursor_y, self.cursor_x));
            }
            self.selection_end = Some((self.cursor_y, self.cursor_x));
        } else {
            self.clear_selection();
        }
    }

    fn move_cursor_left(&mut self, editor_content_area: Rect, shift_pressed: bool) {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;

        if self.cursor_x > 0 {
            self.cursor_x -= 1;
        } else if self.cursor_y > 0 {
            self.cursor_y -= 1;
            self.cursor_x = self.buffer[self.cursor_y].len();
        }
        self.update_selection_on_move(shift_pressed);
        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
    }

    fn move_cursor_right(&mut self, editor_content_area: Rect, shift_pressed: bool) {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;

        if self.cursor_x < self.buffer[self.cursor_y].len() {
            self.cursor_x += 1;
        } else if self.cursor_y < self.buffer.len() - 1 {
            self.cursor_y += 1;
            self.cursor_x = 0;
        }
        self.update_selection_on_move(shift_pressed);
        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
    }

    fn move_cursor_up(&mut self, editor_content_area: Rect, shift_pressed: bool) {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;

        if self.cursor_y > 0 {
            self.cursor_y -= 1;
            self.cursor_x = self.cursor_x.min(self.buffer[self.cursor_y].len());
        }
        self.update_selection_on_move(shift_pressed);
        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
    }

    fn move_cursor_down(&mut self, editor_content_area: Rect, shift_pressed: bool) {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;

        if self.cursor_y < self.buffer.len() - 1 {
            self.cursor_y += 1;
            self.cursor_x = self.cursor_x.min(self.buffer[self.cursor_y].len());
        }
        self.update_selection_on_move(shift_pressed);
        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
    }

    fn move_cursor_word_left(&mut self, editor_content_area: Rect, shift_pressed: bool) {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;

        if self.cursor_x == 0 {
            if self.cursor_y > 0 {
                self.cursor_y -= 1;
                self.cursor_x = self.buffer[self.cursor_y].len();
            } else {
                return;
            }
        }

        let current_line_chars: Vec<char> = self.buffer[self.cursor_y].chars().collect();

        while self.cursor_x > 0 && !current_line_chars[self.cursor_x - 1].is_alphanumeric() {
            self.cursor_x -= 1;
        }

        while self.cursor_x > 0 && current_line_chars[self.cursor_x - 1].is_alphanumeric() {
            self.cursor_x -= 1;
        }

        self.update_selection_on_move(shift_pressed);
        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
    }

    fn move_cursor_word_right(&mut self, editor_content_area: Rect, shift_pressed: bool) {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;

        if self.cursor_x == self.buffer[self.cursor_y].len() {
            if self.cursor_y < self.buffer.len() - 1 {
                self.cursor_y += 1;
                self.cursor_x = 0;
            } else {
                return;
            }
        }

        let current_line_chars: Vec<char> = self.buffer[self.cursor_y].chars().collect();
        let original_cursor_x = self.cursor_x;

        while self.cursor_x < current_line_chars.len() && current_line_chars[self.cursor_x].is_alphanumeric() {
            self.cursor_x += 1;
        }

        while self.cursor_x < current_line_chars.len() && !current_line_chars[self.cursor_x].is_alphanumeric() {
            self.cursor_x += 1;
        }

        if original_cursor_x == self.cursor_x && self.cursor_x < current_line_chars.len() && current_line_chars[original_cursor_x].is_alphanumeric() {
             while self.cursor_x < current_line_chars.len() && current_line_chars[self.cursor_x].is_alphanumeric() {
                self.cursor_x += 1;
            }
            while self.cursor_x < current_line_chars.len() && !current_line_chars[self.cursor_x].is_alphanumeric() {
                self.cursor_x += 1;
            }
        }

        self.update_selection_on_move(shift_pressed);
        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
    }

    fn get_selected_text(&self) -> Option<String> {
        self.get_normalized_selection().map(|((start_row, start_col), (end_row, end_col))| {
            let mut selected_text = String::new();
            if start_row == end_row {
                selected_text.push_str(&self.buffer[start_row][start_col..end_col]);
            } else {
                selected_text.push_str(&self.buffer[start_row][start_col..]);
                for r in (start_row + 1)..end_row {
                    selected_text.push('\n');
                    selected_text.push_str(&self.buffer[r]);
                }
                selected_text.push('\n');
                selected_text.push_str(&self.buffer[end_row][..end_col]);
            }
            selected_text
        })
    }

    fn copy_selection(&mut self) {
        if let Some(text) = self.get_selected_text() {
            self.clipboard = text;
            self.status_message = format!("Copied {} characters.", self.clipboard.len());
        } else {
            self.status_message = "No selection to copy.".to_string();
        }
    }

    fn cut_selection(&mut self, editor_content_area: Rect) {
        if let Some(text) = self.get_selected_text() {
            self.clipboard = text;
            self.delete_selected_text(editor_content_area);
            self.status_message = format!("Cut {} characters.", self.clipboard.len());
        } else {
            self.status_message = "No selection to cut.".to_string();
        }
    }

    fn insert_text_at_cursor(&mut self, text: &str, editor_content_area: Rect) {
        if self.selection_start.is_some() {
            self.delete_selected_text(editor_content_area);
        }

        let lines: Vec<&str> = text.split('\n').collect();
        if lines.is_empty() { return; }

        let remaining_line = self.buffer[self.cursor_y].split_off(self.cursor_x);

        self.buffer[self.cursor_y].push_str(lines[0]);
        self.cursor_x += lines[0].len();

        if lines.len() > 1 {
            for (i, &line_part) in lines.iter().enumerate().skip(1) {
                if i < lines.len() - 1 {
                    self.buffer.insert(self.cursor_y + 1, line_part.to_string());
                } else {
                    self.buffer.insert(self.cursor_y + 1, line_part.to_string() + &remaining_line);
                }
                self.cursor_y += 1;
            }
            self.cursor_x = lines.last().unwrap().len();
        } else {
            self.buffer[self.cursor_y].push_str(&remaining_line);
        }

        self.ensure_cursor_in_view(
            editor_content_area,
            self.config.main_section.line_numbers.enabled,
            self.config.main_section.line_numbers.gutter_width
        );
    }

    fn paste(&mut self, editor_content_area: Rect) {
        let clipboard_content = self.clipboard.clone();
        if !clipboard_content.is_empty() {
            self.insert_text_at_cursor(&clipboard_content, editor_content_area);
            self.status_message = format!("Pasted {} characters.", clipboard_content.len());
        } else {
            self.status_message = "Clipboard is empty.".to_string();
        }
    }

    fn insert_char(&mut self, c: char, editor_content_area: Rect) {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;
        self.clear_selection();
        self.buffer[self.cursor_y].insert(self.cursor_x, c);
        self.cursor_x += 1;
        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
    }

    fn insert_newline(&mut self, editor_content_area: Rect) {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;
        self.clear_selection();
        let rest_of_line = self.buffer[self.cursor_y].split_off(self.cursor_x);
        self.buffer.insert(self.cursor_y + 1, rest_of_line);
        self.cursor_y += 1;
        self.cursor_x = 0;
        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
    }

    fn delete_char_backward(&mut self, editor_content_area: Rect) {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;
        if self.selection_start.is_some() {
            self.delete_selected_text(editor_content_area);
            return;
        }

        if self.cursor_x > 0 {
            self.cursor_x -= 1;
            self.buffer[self.cursor_y].remove(self.cursor_x);
        } else if self.cursor_y > 0 {
            let current_line = self.buffer.remove(self.cursor_y);
            self.cursor_y -= 1;
            self.cursor_x = self.buffer[self.cursor_y].len();
            self.buffer[self.cursor_y].push_str(&current_line);
        }
        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
    }

    fn delete_char_forward(&mut self, editor_content_area: Rect) {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;
        if self.selection_start.is_some() {
            self.delete_selected_text(editor_content_area);
            return;
        }

        if self.cursor_x < self.buffer[self.cursor_y].len() {
            self.buffer[self.cursor_y].remove(self.cursor_x);
        } else if self.cursor_y < self.buffer.len() - 1 {
            let next_line = self.buffer.remove(self.cursor_y + 1);
            self.buffer[self.cursor_y].push_str(&next_line);
        }
        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
    }

    fn handle_key_insert_mode(&mut self, key_event: KeyEvent, editor_content_area: Rect) -> bool {
        let shift_pressed = key_event.modifiers.contains(KeyModifiers::SHIFT);
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;
        let editor_visible_height = editor_content_area.height.saturating_sub(2) as usize;

        match key_event.code {
            KeyCode::Esc => {
                if self.vim_enabled {
                    self.input_mode = InputMode::Normal;
                    self.status_message = "-- NORMAL --".to_string();
                    self.clear_selection();
                    self.cursor_x = self.cursor_x.saturating_sub(1).min(self.buffer[self.cursor_y].len().saturating_sub(1).max(0));
                }
                false
            }
            KeyCode::Char(c) => {
                if key_event.modifiers.is_empty() || key_event.modifiers.contains(KeyModifiers::SHIFT) {
                    self.insert_char(c, editor_content_area);
                }
                false
            }
            KeyCode::Enter => {
                self.insert_newline(editor_content_area);
                false
            }
            KeyCode::Backspace => {
                self.delete_char_backward(editor_content_area);
                false
            }
            KeyCode::Delete => {
                self.delete_char_forward(editor_content_area);
                false
            }
            KeyCode::Left => {
                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                    self.move_cursor_word_left(editor_content_area, shift_pressed);
                } else {
                    self.move_cursor_left(editor_content_area, shift_pressed);
                }
                false
            }
            KeyCode::Right => {
                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                    self.move_cursor_word_right(editor_content_area, shift_pressed);
                } else {
                    self.move_cursor_right(editor_content_area, shift_pressed);
                }
                false
            }
            KeyCode::Up => {
                self.move_cursor_up(editor_content_area, shift_pressed);
                false
            }
            KeyCode::Down => {
                self.move_cursor_down(editor_content_area, shift_pressed);
                false
            }
            KeyCode::Home if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor_y = 0; self.cursor_x = 0;
                self.scroll_y = 0; self.scroll_x = 0;
                self.update_selection_on_move(shift_pressed);
                false
            }
            KeyCode::End if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor_y = self.buffer.len().saturating_sub(1);
                if self.cursor_y < self.buffer.len() { self.cursor_x = self.buffer[self.cursor_y].len(); } else { self.cursor_x = 0; }
                self.update_selection_on_move(shift_pressed);
                self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
                false
            }
            KeyCode::Home => {
                self.cursor_x = 0; self.scroll_x = 0;
                self.update_selection_on_move(shift_pressed);
                self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
                false
            }
            KeyCode::End => {
                if self.cursor_y < self.buffer.len() { self.cursor_x = self.buffer[self.cursor_y].len(); } else { self.cursor_x = 0; }
                self.update_selection_on_move(shift_pressed);
                self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
                false
            }
            KeyCode::PageUp => {
                self.scroll_y = self.scroll_y.saturating_sub(editor_visible_height);
                self.cursor_y = self.cursor_y.saturating_sub(editor_visible_height).max(self.scroll_y);
                self.cursor_x = self.cursor_x.min(self.buffer[self.cursor_y].len());
                self.update_selection_on_move(shift_pressed);
                self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
                false
            }
            KeyCode::PageDown => {
                self.scroll_y = (self.scroll_y + editor_visible_height).min(self.buffer.len().saturating_sub(1).max(0));
                self.cursor_y = (self.cursor_y + editor_visible_height).min(self.buffer.len().saturating_sub(1));
                self.cursor_x = self.cursor_x.min(self.buffer[self.cursor_y].len());
                self.update_selection_on_move(shift_pressed);
                self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width);
                false
            }
            _ => false,
        }
    }

    fn handle_key_normal_mode(&mut self, key_event: KeyEvent, editor_content_area: Rect) -> bool {
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let gutter_width = self.config.main_section.line_numbers.gutter_width;
        let _editor_visible_height = editor_content_area.height.saturating_sub(2) as usize;

        let shift_pressed = key_event.modifiers.contains(KeyModifiers::SHIFT);
        if !shift_pressed && self.selection_start.is_some() {
            self.clear_selection();
        }

        match key_event.code {
            KeyCode::Char('i') => {
                self.input_mode = InputMode::Insert;
                self.status_message = "-- INSERT --".to_string();
                false
            }
            KeyCode::Char('a') => {
                self.cursor_x += 1;
                self.input_mode = InputMode::Insert;
                self.status_message = "-- INSERT --".to_string();
                false
            }
            KeyCode::Char('o') => {
                self.cursor_y += 1;
                self.cursor_x = 0;
                self.insert_newline(editor_content_area);
                self.input_mode = InputMode::Insert;
                self.status_message = "-- INSERT --".to_string();
                false
            }
            KeyCode::Char('O') => {
                self.insert_newline(editor_content_area);
                self.cursor_y = self.cursor_y.saturating_sub(1);
                self.cursor_x = 0;
                self.input_mode = InputMode::Insert;
                self.status_message = "-- INSERT --".to_string();
                false
            }

            KeyCode::Char('h') | KeyCode::Left => { self.move_cursor_left(editor_content_area, shift_pressed); false }
            KeyCode::Char('j') | KeyCode::Down => { self.move_cursor_down(editor_content_area, shift_pressed); false }
            KeyCode::Char('k') | KeyCode::Up => { self.move_cursor_up(editor_content_area, shift_pressed); false }
            KeyCode::Char('l') | KeyCode::Right => { self.move_cursor_right(editor_content_area, shift_pressed); false }

            KeyCode::Char('b') => { self.move_cursor_word_left(editor_content_area, shift_pressed); false }
            KeyCode::Char('w') => { self.move_cursor_word_right(editor_content_area, shift_pressed); false }

            KeyCode::Char('0') => { self.cursor_x = 0; self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width); false }
            KeyCode::Char('$') => { if self.cursor_y < self.buffer.len() { self.cursor_x = self.buffer[self.cursor_y].len(); } else { self.cursor_x = 0; } self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, gutter_width); false }

            KeyCode::Char('x') => { self.delete_char_forward(editor_content_area); false }

            KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => { self.copy_selection(); false }
            KeyCode::Char('u') if key_event.modifiers.contains(KeyModifiers::CONTROL) => { self.cut_selection(editor_content_area); false }
            KeyCode::Char('v') if key_event.modifiers.contains(KeyModifiers::CONTROL) => { self.paste(editor_content_area); false }

            KeyCode::Esc => {
                self.clear_selection();
                if self.cursor_x > 0 && self.cursor_x == self.buffer[self.cursor_y].len() && self.buffer[self.cursor_y].len() > 0 {
                    self.cursor_x -= 1;
                }
                false
            }
            _ => false,
        }
    }

    fn handle_key_input(&mut self, key_event: KeyEvent, editor_content_area: Rect) -> bool {
        let should_exit = match key_event.code {
            KeyCode::Char('x') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.application_mode == ApplicationMode::Editing {
                    if self.selection_start.is_some() {
                        self.cut_selection(editor_content_area);
                        false
                    } else if self.is_dirty() {
                        self.application_mode = ApplicationMode::PromptSave;
                        self.prompt_message = "Save modified buffer? (Y/N)".to_string();
                        false
                    } else {
                        true
                    }
                } else { false }
            }
            KeyCode::Char('w') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.application_mode == ApplicationMode::Editing {
                    if let Err(e) = self.save_file() {
                        self.status_message = format!("Error saving: {}", e);
                    }
                }
                false
            }
            KeyCode::Char('q') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.application_mode == ApplicationMode::Editing {
                    if self.is_dirty() {
                        self.application_mode = ApplicationMode::PromptSave;
                        self.prompt_message = "Quit without saving? (Y/N)".to_string();
                        false
                    } else {
                        true
                    }
                } else { false }
            }
            KeyCode::Char('h') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.application_mode == ApplicationMode::Editing {
                    self.application_mode = ApplicationMode::Help;
                    if self.vim_enabled {
                        self.status_message = "-- HELP --".to_string();
                    }
                }
                false
            }
            _ => false,
        };

        if should_exit { return true; }

        match self.application_mode {
            ApplicationMode::Editing => {
                match self.input_mode {
                    InputMode::Insert => self.handle_key_insert_mode(key_event, editor_content_area),
                    InputMode::Normal => self.handle_key_normal_mode(key_event, editor_content_area),
                }
            },
            ApplicationMode::Help => self.handle_key_help_mode(key_event),
            ApplicationMode::PromptSave => self.handle_key_prompt_save_mode(key_event),
        }
    }

    fn handle_key_help_mode(&mut self, key_event: KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Enter => {
                self.application_mode = ApplicationMode::Editing;
                if self.vim_enabled {
                    self.status_message = match self.input_mode {
                        InputMode::Normal => "-- NORMAL --".to_string(),
                        InputMode::Insert => "-- INSERT --".to_string(),
                    };
                } else {
                    self.status_message = "Ctrl+X Exit | Ctrl+W Save | Ctrl+H Help".to_string();
                }
            }
            _ => {}
        }
        false
    }

    fn handle_key_prompt_save_mode(&mut self, key_event: KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Err(e) = self.save_file() {
                    self.status_message = format!("Error saving: {}", e);
                    self.application_mode = ApplicationMode::Editing;
                    false
                } else {
                    return true;
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                return true;
            }
            KeyCode::Esc => {
                self.application_mode = ApplicationMode::Editing;
                if self.vim_enabled {
                    self.status_message = match self.input_mode {
                        InputMode::Normal => "-- NORMAL --".to_string(),
                        InputMode::Insert => "-- INSERT --".to_string(),
                    };
                } else {
                    self.status_message = "Ctrl+X Exit | Ctrl+W Save | Ctrl+H Help".to_string();
                }
                false
            }
            _ => false,
        }
    }

    fn draw_ui(&mut self, frame: &mut Frame<'_>) {
        let size = frame.area();
        let line_numbers_enabled = self.config.main_section.line_numbers.enabled;
        let line_numbers_gutter_width = self.config.main_section.line_numbers.gutter_width;
        let line_numbers_color = self.config.main_section.line_numbers.color.parse::<Color>().unwrap_or(Color::DarkGray);
        let line_numbers_show_separator = self.config.main_section.line_numbers.show_separator_line;

        let frame_hide = self.config.main_section.frame.hide;
        let frame_color_str = self.config.main_section.frame.color.clone();
        let frame_corner = self.config.main_section.frame.corner.clone();
        let background_color_str = self.config.main_section.background_color.clone();

        let status_panel_enabled = self.config.main_section.status_panel.enabled;
        let status_panel_bg_color_str = self.config.main_section.status_panel.background_color.clone();
        let status_panel_fg_color_str = self.config.main_section.status_panel.foreground_color.clone();

        let prompt_panel_enabled = self.config.main_section.prompt_panel.enabled;
        let prompt_panel_bg_color_str = self.config.main_section.prompt_panel.background_color.clone();
        let prompt_panel_fg_color_str = self.config.main_section.prompt_panel.foreground_color.clone();


        let mut constraints = vec![Constraint::Min(1)];
        if status_panel_enabled {
            constraints.push(Constraint::Length(1));
        }
        if prompt_panel_enabled {
            constraints.push(Constraint::Length(1));
        }

        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(size);

        let editor_content_area = main_layout[0];

        self.ensure_cursor_in_view(editor_content_area, line_numbers_enabled, line_numbers_gutter_width);

        let mut text_lines: Vec<Line> = Vec::new();

        let visible_height = editor_content_area.height.saturating_sub(2) as usize;
        let start_line_idx = self.scroll_y;
        let end_line_idx = (self.scroll_y + visible_height).min(self.buffer.len());

        let line_numbers_gutter_width_total = if line_numbers_enabled {
            line_numbers_gutter_width + 1
        } else {
            0
        };
        let effective_editor_width = editor_content_area.width.saturating_sub(2).saturating_sub(line_numbers_gutter_width_total) as usize;

        let normalized_selection = self.get_normalized_selection();
        let selection_bg_color = Color::Rgb(50, 50, 100);

        for i in start_line_idx..end_line_idx {
            let mut spans = Vec::new();
            let line = &self.buffer[i];

            if line_numbers_enabled {
                let separator_char_width = if line_numbers_show_separator { 1 } else { 0 };
                let trailing_space_width = 1;

                let number_display_width = line_numbers_gutter_width.saturating_sub(separator_char_width).saturating_sub(trailing_space_width);
                let number_display_width = number_display_width.max(1);

                let line_num_str = format!("{:>width$}", i + 1, width = number_display_width as usize);
                spans.push(Span::styled(line_num_str, Style::default().fg(line_numbers_color)));

                if line_numbers_show_separator {
                    spans.push(Span::styled("|", Style::default().fg(line_numbers_color)));
                }
                spans.push(Span::raw(" "));
            }

            let chars_on_line: Vec<char> = line.chars().collect();
            for (char_idx_in_line, &c) in chars_on_line.iter().enumerate() {
                if char_idx_in_line >= self.scroll_x && char_idx_in_line < self.scroll_x + effective_editor_width {
                    let mut char_style = Style::default();

                    if let Some(((sel_start_row, sel_start_col), (sel_end_row, sel_end_col))) = normalized_selection {
                        let is_selected = if i > sel_start_row && i < sel_end_row {
                            true
                        } else if i == sel_start_row && i == sel_end_row {
                            char_idx_in_line >= sel_start_col && char_idx_in_line < sel_end_col
                        } else if i == sel_start_row && i < sel_end_row {
                            char_idx_in_line >= sel_start_col
                        } else if i > sel_start_row && i == sel_end_row {
                            char_idx_in_line < sel_end_col
                        } else {
                            false
                        };

                        if is_selected {
                            char_style = char_style.bg(selection_bg_color);
                        }
                    }
                    spans.push(Span::styled(c.to_string(), char_style));
                }
            }
            text_lines.push(Line::from(spans));
        }

        let mut editor_block = Block::default();

        if !frame_hide {
            let border_style = Style::default().fg(frame_color_str.parse::<Color>().unwrap_or(Color::Blue));
            editor_block = editor_block.borders(Borders::ALL)
                .border_type(match frame_corner.as_str() {
                    "rounded" => BorderType::Rounded,
                    "thick" => BorderType::Thick,
                    _ => BorderType::Plain,
                })
                .border_style(border_style);
        }

        editor_block = editor_block.title(
            format!(
                "Zepto - {} {}",
                self.filename.as_deref().unwrap_or("[No Name]"),
                if self.is_dirty() { "(Modified)" } else { "" }
            )
        );

        let editor_bg_color = background_color_str.parse::<Color>().unwrap_or(Color::Black);
        editor_block = editor_block.style(Style::default().bg(editor_bg_color));

        let editor_paragraph = Paragraph::new(text_lines)
            .block(editor_block)
            .wrap(Wrap { trim: false });

        frame.render_widget(editor_paragraph, editor_content_area);

        let cursor_offset_x_from_content_start: u16 = if line_numbers_enabled {
            line_numbers_gutter_width + 1
        } else {
            1
        };

        let relative_cursor_x_in_view = self.cursor_x.saturating_sub(self.scroll_x) as u16;
        let relative_cursor_y_in_view = self.cursor_y.saturating_sub(self.scroll_y) as u16;

        let actual_cursor_x_for_display = if self.vim_enabled && self.input_mode == InputMode::Normal && self.cursor_x == self.buffer[self.cursor_y].len() && self.buffer[self.cursor_y].len() > 0 {
            relative_cursor_x_in_view.saturating_sub(1)
        } else {
            relative_cursor_x_in_view
        };

        frame.set_cursor_position((
            editor_content_area.x + cursor_offset_x_from_content_start + actual_cursor_x_for_display,
            editor_content_area.y + 1 + relative_cursor_y_in_view,
        ));

        let mut current_layout_index = 1;

        if status_panel_enabled {
            let status_block = Block::default()
                .style(Style::default()
                    .bg(status_panel_bg_color_str.parse::<Color>().unwrap_or(Color::Blue))
                    .fg(status_panel_fg_color_str.parse::<Color>().unwrap_or(Color::White)));

            let status_text = Paragraph::new(self.status_message.as_str())
                .block(status_block);
            frame.render_widget(status_text, main_layout[current_layout_index]);
            current_layout_index += 1;
        }

        if prompt_panel_enabled {
            let prompt_block = Block::default()
                .style(Style::default()
                    .bg(prompt_panel_bg_color_str.parse::<Color>().unwrap_or(Color::DarkGray))
                    .fg(prompt_panel_fg_color_str.parse::<Color>().unwrap_or(Color::White)));
            let prompt_text = Paragraph::new(self.prompt_message.as_str())
                .block(prompt_block);
            frame.render_widget(prompt_text, main_layout[current_layout_index]);
        }
    }

    fn draw_help_ui(&self, frame: &mut Frame<'_>) {
        let size = frame.area();
        let help_text_nano = vec![
            Line::from("--- Help (Nano-like) ---"),
            Line::from(""),
            Line::from("Ctrl+X: Exit (prompts to save if modified)"),
            Line::from("Ctrl+W: Save File"),
            Line::from("Ctrl+Q: Quit without saving (prompts if modified)"),
            Line::from("Ctrl+H: Show this Help"),
            Line::from(""),
            Line::from("Arrow Keys: Move Cursor"),
            Line::from("Shift+Arrow Keys: Select Text"),
            Line::from("Ctrl+C: Copy Selection"),
            Line::from("Ctrl+U: Cut Selection"),
            Line::from("Ctrl+V: Paste"),
            Line::from("Ctrl+Left/Right: Move cursor by word"),
            Line::from("PageUp/PageDown: Scroll through file"),
            Line::from("Home/End: Go to start/end of line"),
            Line::from("Ctrl+Home/Ctrl+End: Go to start/end of file"),
            Line::from("Backspace: Delete character backward"),
            Line::from("Delete: Delete character forward"),
            Line::from("Enter: New line"),
            Line::from("Esc: Clear selection"),
            Line::from(""),
            Line::from("Press ESC or any key to return to editor."),
        ];

        let help_text_vim = vec![
            Line::from("--- Help (Vim-like) ---"),
            Line::from(""),
            Line::from("GLOBAL COMMANDS:"),
            Line::from("  Ctrl+X: Exit (prompts to save if modified)"),
            Line::from("  Ctrl+W: Save File"),
            Line::from("  Ctrl+Q: Quit without saving (prompts if modified)"),
            Line::from("  Ctrl+H: Show this Help"),
            Line::from(""),
            Line::from("NORMAL MODE:"),
            Line::from("  i: Insert before cursor"),
            Line::from("  a: Insert after cursor"),
            Line::from("  o: Insert new line below"),
            Line::from("  O: Insert new line above"),
            Line::from("  h, j, k, l: Move cursor (Left, Down, Up, Right)"),
            Line::from("  w, b: Move cursor by word (Forward, Backward)"),
            Line::from("  0: Go to start of line"),
            Line::from("  $: Go to end of line"),
            Line::from("  x: Delete character under cursor"),
            Line::from("  Ctrl+C: Copy Selection (Visual Mode needed for full power)"),
            Line::from("  Ctrl+U: Cut Selection (Visual Mode needed for full power)"),
            Line::from("  Ctrl+V: Paste"),
            Line::from("  Esc: Clear selection (if active)"),
            Line::from(""),
            Line::from("INSERT MODE:"),
            Line::from("  Typing: Insert characters"),
            Line::from("  Enter: New line"),
            Line::from("  Backspace/Delete: Delete characters"),
            Line::from("  Arrow Keys: Move cursor"),
            Line::from("  Shift+Arrow Keys: Select text"),
            Line::from("  Esc: Exit to Normal Mode"),
            Line::from(""),
            Line::from("Press ESC or any key to return to editor."),
        ];

        let help_paragraph = Paragraph::new(if self.vim_enabled { help_text_vim } else { help_text_nano })
            .block(Block::default().borders(Borders::ALL).title("Zepto Help"))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false });

        let area = Rect::new(
            size.width / 4,
            size.height / 4,
            size.width / 2,
            size.height / 2,
        );
        frame.render_widget(help_paragraph, area);
    }

    pub fn run(mut self, mut terminal: Terminal<B>) -> io::Result<Terminal<B>> {
        let args: Vec<String> = env::args().collect();
        if args.len() > 1 {
            if let Err(e) = self.open_file(&args[1]) {
                self.status_message = format!("Error opening file: {}", e);
            }
        }

        let mut should_exit = false;
        while !should_exit {
            let editor_content_area = {
                let size_of_terminal = terminal.size()?;
                let mut temp_constraints = vec![Constraint::Min(1)];
                if self.config.main_section.status_panel.enabled {
                    temp_constraints.push(Constraint::Length(1));
                }
                if self.config.main_section.prompt_panel.enabled {
                    temp_constraints.push(Constraint::Length(1));
                }
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(temp_constraints)
                    .split(Rect::new(0, 0, size_of_terminal.width, size_of_terminal.height))[0]
            };

            terminal.draw(|frame| {
                match self.application_mode {
                    ApplicationMode::Editing | ApplicationMode::PromptSave => self.draw_ui(frame),
                    ApplicationMode::Help => self.draw_help_ui(frame),
                }
            })?;

            if event::poll(std::time::Duration::from_millis(50))? {
                if let Event::Key(key_event) = event::read()? {
                    should_exit = self.handle_key_input(key_event, editor_content_area);
                }
            }
        }

        Ok(terminal)
    }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let config = config::load_config();

    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;

    let editor = Editor::new_with_backend(config);

    let mut terminal_after_run = editor.run(terminal)?;

    terminal_after_run.backend_mut().execute(LeaveAlternateScreen)?;
    terminal_after_run.show_cursor()?;
    disable_raw_mode()?;

    Ok(())
}
