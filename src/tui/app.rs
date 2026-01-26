use std::sync::mpsc::Receiver;

use crossterm::event::{KeyCode, KeyModifiers};

use super::ansi::strip_ansi;
use super::message::TuiMessage;

/// Number of lines to scroll with mouse wheel
const MOUSE_SCROLL_LINES: usize = 3;

/// Input mode for the TUI
#[derive(Clone, Copy, PartialEq)]
pub(super) enum InputMode {
    /// Normal mode - scrolling and navigation
    Normal,
    /// Search mode - typing search query
    Search,
}

/// TUI application state
pub struct TuiApp {
    /// Lines of output to display (raw with ANSI codes)
    pub(super) lines: Vec<String>,
    /// Current scroll position (line offset from bottom)
    pub(super) scroll_offset: usize,
    /// Whether the quit confirmation modal is showing
    pub(super) show_quit_modal: bool,
    /// Whether work is complete (affects quit behavior)
    work_complete: bool,
    /// Whether the user confirmed quit
    pub(super) should_quit: bool,
    /// Channel receiver for incoming messages
    rx: Receiver<TuiMessage>,
    /// Current input mode
    pub(super) input_mode: InputMode,
    /// Search query string
    pub(super) search_query: String,
    /// Indices of lines matching the search query
    pub(super) search_matches: Vec<usize>,
    /// Current match index (for n/N navigation)
    pub(super) current_match: usize,
}

impl TuiApp {
    /// Create a new TUI application with a message receiver.
    pub fn new(rx: Receiver<TuiMessage>) -> Self {
        Self {
            lines: Vec::new(),
            scroll_offset: 0,
            show_quit_modal: false,
            work_complete: false,
            should_quit: false,
            rx,
            input_mode: InputMode::Normal,
            search_query: String::new(),
            search_matches: Vec::new(),
            current_match: 0,
        }
    }

    /// Process any pending messages from the channel.
    pub(super) fn process_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                TuiMessage::AppendLine(line) => {
                    // Skip empty lines (used for channel health checks)
                    if line.is_empty() {
                        continue;
                    }
                    // Split multi-line strings
                    for l in line.lines() {
                        self.lines.push(l.to_string());
                    }
                    // Update search matches if we have an active search
                    if !self.search_query.is_empty() {
                        self.update_search_matches();
                    }
                }
                TuiMessage::WorkComplete => {
                    self.work_complete = true;
                }
                TuiMessage::QuitRequested => {
                    self.show_quit_modal = true;
                }
            }
        }
    }

    /// Update search matches based on current query.
    fn update_search_matches(&mut self) {
        self.search_matches.clear();
        if self.search_query.is_empty() {
            return;
        }
        let query_lower = self.search_query.to_lowercase();
        for (idx, line) in self.lines.iter().enumerate() {
            // Strip ANSI codes for search matching
            let plain = strip_ansi(line);
            if plain.to_lowercase().contains(&query_lower) {
                self.search_matches.push(idx);
            }
        }
        // Reset current match if out of bounds
        if self.current_match >= self.search_matches.len() {
            self.current_match = 0;
        }
    }

    /// Jump to the current search match.
    fn jump_to_current_match(&mut self, inner_height: usize) {
        if self.search_matches.is_empty() {
            return;
        }
        let match_idx = self.search_matches[self.current_match];
        let total = self.lines.len();

        // Calculate scroll offset to show the matched line
        // We want the matched line to be visible in the viewport
        if total <= inner_height {
            self.scroll_offset = 0;
        } else {
            // scroll_offset is distance from bottom
            // match_idx is 0-indexed from top
            // We want match_idx to be visible
            let lines_from_bottom = total.saturating_sub(match_idx + 1);
            // Clamp to valid range
            let max_scroll = total.saturating_sub(inner_height);
            self.scroll_offset = lines_from_bottom.min(max_scroll);
        }
    }

    /// Handle a key event.
    pub(super) fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers, inner_height: usize) {
        // Handle quit modal first
        if self.show_quit_modal {
            match key {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.should_quit = true;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.show_quit_modal = false;
                }
                _ => {}
            }
            return;
        }

        match self.input_mode {
            InputMode::Search => match key {
                KeyCode::Esc => {
                    // Exit search mode
                    self.input_mode = InputMode::Normal;
                }
                KeyCode::Enter => {
                    // Confirm search and jump to first match
                    self.update_search_matches();
                    if !self.search_matches.is_empty() {
                        self.current_match = 0;
                        self.jump_to_current_match(inner_height);
                    }
                    self.input_mode = InputMode::Normal;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.update_search_matches();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.update_search_matches();
                }
                _ => {}
            },
            InputMode::Normal => match key {
                KeyCode::Char('/') => {
                    // Enter search mode
                    self.input_mode = InputMode::Search;
                    self.search_query.clear();
                    self.search_matches.clear();
                }
                KeyCode::Char('n') => {
                    // Next search match
                    if !self.search_matches.is_empty() {
                        self.current_match = (self.current_match + 1) % self.search_matches.len();
                        self.jump_to_current_match(inner_height);
                    }
                }
                KeyCode::Char('N') => {
                    // Previous search match
                    if !self.search_matches.is_empty() {
                        self.current_match = if self.current_match == 0 {
                            self.search_matches.len().saturating_sub(1)
                        } else {
                            self.current_match - 1
                        };
                        self.jump_to_current_match(inner_height);
                    }
                }
                KeyCode::Char('q') | KeyCode::Char('Q') => {
                    if self.work_complete {
                        self.should_quit = true;
                    } else {
                        self.show_quit_modal = true;
                    }
                }
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    if self.work_complete {
                        self.should_quit = true;
                    } else {
                        self.show_quit_modal = true;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let max_scroll = self.lines.len().saturating_sub(1);
                    self.scroll_offset = (self.scroll_offset + 1).min(max_scroll);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.scroll_offset = self.scroll_offset.saturating_sub(1);
                }
                KeyCode::PageUp => {
                    let max_scroll = self.lines.len().saturating_sub(1);
                    self.scroll_offset = (self.scroll_offset + 10).min(max_scroll);
                }
                KeyCode::PageDown => {
                    self.scroll_offset = self.scroll_offset.saturating_sub(10);
                }
                KeyCode::Home => {
                    self.scroll_offset = self.lines.len().saturating_sub(1);
                }
                KeyCode::End => {
                    self.scroll_offset = 0;
                }
                KeyCode::Esc => {
                    // Clear search
                    self.search_query.clear();
                    self.search_matches.clear();
                }
                _ => {}
            },
        }
    }

    /// Handle mouse scroll event.
    pub(super) fn handle_mouse_scroll(&mut self, up: bool) {
        if self.show_quit_modal || self.input_mode == InputMode::Search {
            return;
        }

        let max_scroll = self.lines.len().saturating_sub(1);
        if up {
            self.scroll_offset = (self.scroll_offset + MOUSE_SCROLL_LINES).min(max_scroll);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_sub(MOUSE_SCROLL_LINES);
        }
    }
}
