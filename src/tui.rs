//! Terminal User Interface using ratatui.
//!
//! Provides a scrollable output pane with a header, search, and quit confirmation modal.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame, Terminal,
};

/// ASCII art header for SWARM HUG
const HEADER: &str = r#"┏━┓╻ ╻┏━┓┏━┓┏┳┓   ╻ ╻╻ ╻┏━╸
┗━┓┃╻┃┣━┫┣┳┛┃┃┃   ┣━┫┃ ┃┃╺┓
┗━┛┗┻┛╹ ╹╹┗╸╹ ╹   ╹ ╹┗━┛┗━┛"#;

/// Padding inside content area (1 cell on each side)
const CONTENT_PADDING: u16 = 1;

/// Number of lines to scroll with mouse wheel
const MOUSE_SCROLL_LINES: usize = 3;

/// Message types for TUI communication
#[derive(Clone)]
pub enum TuiMessage {
    /// Append a line to the output
    AppendLine(String),
    /// Signal that the work is complete
    WorkComplete,
    /// Request to quit (user pressed q)
    QuitRequested,
}

/// Input mode for the TUI
#[derive(Clone, Copy, PartialEq)]
enum InputMode {
    /// Normal mode - scrolling and navigation
    Normal,
    /// Search mode - typing search query
    Search,
}

/// TUI application state
pub struct TuiApp {
    /// Lines of output to display (raw with ANSI codes)
    lines: Vec<String>,
    /// Current scroll position (line offset from bottom)
    scroll_offset: usize,
    /// Whether the quit confirmation modal is showing
    show_quit_modal: bool,
    /// Whether work is complete (affects quit behavior)
    work_complete: bool,
    /// Whether the user confirmed quit
    should_quit: bool,
    /// Channel receiver for incoming messages
    rx: Receiver<TuiMessage>,
    /// Current input mode
    input_mode: InputMode,
    /// Search query string
    search_query: String,
    /// Indices of lines matching the search query
    search_matches: Vec<usize>,
    /// Current match index (for n/N navigation)
    current_match: usize,
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
    fn process_messages(&mut self) {
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
    fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers, inner_height: usize) {
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
            InputMode::Search => {
                match key {
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
                }
            }
            InputMode::Normal => {
                match key {
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
                }
            }
        }
    }

    /// Handle mouse scroll event.
    fn handle_mouse_scroll(&mut self, up: bool) {
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

/// Strip ANSI escape codes from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip escape sequence
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_alphabetic() {
                        chars.next();
                        break;
                    }
                    chars.next();
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Run the TUI application.
pub fn run_tui(rx: Receiver<TuiMessage>) -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Clear the screen first
    terminal.clear()?;

    let mut app = TuiApp::new(rx);

    // Track inner height for search navigation
    let mut last_inner_height: usize = 20;

    loop {
        // Process any pending messages
        app.process_messages();

        // Draw the UI and capture inner height
        terminal.draw(|f| {
            last_inner_height = draw_ui(f, &app);
        })?;

        // Handle events with a timeout so we can process messages
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    app.handle_key(key.code, key.modifiers, last_inner_height);
                }
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            app.handle_mouse_scroll(true);
                        }
                        MouseEventKind::ScrollDown => {
                            app.handle_mouse_scroll(false);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

/// Draw the UI. Returns the inner content height for search navigation.
fn draw_ui(f: &mut Frame, app: &TuiApp) -> usize {
    let size = f.area();

    // Clear the entire frame first
    f.render_widget(Clear, size);

    // Create main layout: header + content + (optional search bar)
    let has_search = app.input_mode == InputMode::Search || !app.search_query.is_empty();
    let constraints = if has_search {
        vec![
            Constraint::Length(4), // Header
            Constraint::Min(0),    // Content
            Constraint::Length(3), // Search bar
        ]
    } else {
        vec![
            Constraint::Length(4), // Header
            Constraint::Min(0),    // Content
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(size);

    // Draw header
    draw_header(f, chunks[0]);

    // Draw content and get inner height
    let inner_height = draw_content(f, chunks[1], app);

    // Draw search bar if active
    if has_search {
        draw_search_bar(f, chunks[2], app);
    }

    // Draw quit modal if showing
    if app.show_quit_modal {
        draw_quit_modal(f, size);
    }

    inner_height
}

/// Draw the ASCII art header.
fn draw_header(f: &mut Frame, area: Rect) {
    let header_lines: Vec<Line> = HEADER
        .lines()
        .map(|line| {
            Line::from(Span::styled(
                line,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();

    let header = Paragraph::new(header_lines)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

    f.render_widget(header, area);
}

/// Draw the scrollable content area. Returns inner height.
fn draw_content(f: &mut Frame, area: Rect, app: &TuiApp) -> usize {
    // Calculate inner area accounting for borders (1 cell each side) and padding
    let border_size: u16 = 2; // 1 for each side
    let inner_width = area.width.saturating_sub(border_size + CONTENT_PADDING * 2) as usize;
    let inner_height = area.height.saturating_sub(border_size + CONTENT_PADDING * 2) as usize;

    if inner_height == 0 || inner_width == 0 {
        // Not enough space to render
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Output ")
            .title_style(Style::default().fg(Color::White));
        f.render_widget(block, area);
        return 0;
    }

    // Calculate which lines to show based on scroll offset
    let total_lines = app.lines.len();
    let start_idx = if total_lines <= inner_height {
        0
    } else {
        total_lines
            .saturating_sub(inner_height)
            .saturating_sub(app.scroll_offset)
    };
    let end_idx = (start_idx + inner_height).min(total_lines);

    // Convert lines to styled Lines, parsing ANSI colors and truncating to fit width
    // Also highlight search matches
    let visible_lines: Vec<Line> = app.lines[start_idx..end_idx]
        .iter()
        .enumerate()
        .map(|(visible_idx, line)| {
            let actual_idx = start_idx + visible_idx;
            let is_match = app.search_matches.contains(&actual_idx);
            let is_current_match = !app.search_matches.is_empty()
                && app.current_match < app.search_matches.len()
                && app.search_matches[app.current_match] == actual_idx;

            truncate_and_parse_ansi_with_highlight(
                line,
                inner_width,
                &app.search_query,
                is_match,
                is_current_match,
            )
        })
        .collect();

    // Build title with search info
    let title = if !app.search_matches.is_empty() {
        format!(
            " Output ({}/{}) [match {}/{}] [↑↓ scroll, / search, n/N next/prev, q quit] ",
            if total_lines > 0 { total_lines.saturating_sub(app.scroll_offset) } else { 0 },
            total_lines,
            app.current_match + 1,
            app.search_matches.len()
        )
    } else {
        format!(
            " Output ({}/{}) [↑↓ scroll, / search, q quit] ",
            if total_lines > 0 { total_lines.saturating_sub(app.scroll_offset) } else { 0 },
            total_lines
        )
    };

    // Create the block with borders
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title)
        .title_style(Style::default().fg(Color::White));

    // Render the block first
    f.render_widget(block, area);

    // Calculate the inner area with padding
    let inner_area = area.inner(Margin {
        horizontal: CONTENT_PADDING + 1,
        vertical: CONTENT_PADDING + 1,
    });

    // Render the content in the inner area (no wrapping!)
    let content = Paragraph::new(visible_lines);
    f.render_widget(content, inner_area);

    inner_height
}

/// Draw the search bar.
fn draw_search_bar(f: &mut Frame, area: Rect, app: &TuiApp) {
    let (border_color, title) = if app.input_mode == InputMode::Search {
        (Color::Yellow, " Search (Enter to confirm, Esc to cancel) ")
    } else {
        (Color::DarkGray, " Search (Esc to clear) ")
    };

    let search_text = if app.input_mode == InputMode::Search {
        format!("/{}_", app.search_query)
    } else {
        format!("/{}", app.search_query)
    };

    let match_info = if app.search_matches.is_empty() {
        if app.search_query.is_empty() {
            String::new()
        } else {
            " (no matches)".to_string()
        }
    } else {
        format!(" ({} matches)", app.search_matches.len())
    };

    let search_line = Line::from(vec![
        Span::styled(&search_text, Style::default().fg(Color::White)),
        Span::styled(match_info, Style::default().fg(Color::DarkGray)),
    ]);

    let search_bar = Paragraph::new(search_line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(title)
                .title_style(Style::default().fg(border_color)),
        );

    f.render_widget(search_bar, area);
}

/// Draw the quit confirmation modal.
fn draw_quit_modal(f: &mut Frame, area: Rect) {
    let modal_width = 40u16;
    let modal_height = 7u16;
    let x = (area.width.saturating_sub(modal_width)) / 2;
    let y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(x, y, modal_width.min(area.width), modal_height.min(area.height));

    f.render_widget(Clear, modal_area);

    let modal_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Work is still in progress!",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Are you sure you want to quit?"),
        Line::from(Span::styled(
            "[Y]es  [N]o",
            Style::default().fg(Color::Cyan),
        )),
    ];

    let modal = Paragraph::new(modal_text)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .title(" Quit? ")
                .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        );

    f.render_widget(modal, modal_area);
}

/// Truncate a line to fit within max_width, parse ANSI codes, and optionally highlight search matches.
fn truncate_and_parse_ansi_with_highlight(
    line: &str,
    max_width: usize,
    search_query: &str,
    is_match: bool,
    is_current_match: bool,
) -> Line<'static> {
    let mut spans = Vec::new();
    let mut current_text = String::new();
    let mut current_style = Style::default();
    let mut chars = line.chars().peekable();
    let mut visible_count = 0;

    // If this is the current match, add a marker
    if is_current_match {
        spans.push(Span::styled(
            "▶ ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
        visible_count += 2;
    } else if is_match {
        spans.push(Span::styled(
            "  ",
            Style::default(),
        ));
        visible_count += 2;
    }

    while let Some(c) = chars.next() {
        if visible_count >= max_width {
            break;
        }

        if c == '\x1b' {
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }

            if chars.peek() == Some(&'[') {
                chars.next();
                let mut code = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_alphabetic() {
                        chars.next();
                        break;
                    }
                    code.push(chars.next().unwrap());
                }
                current_style = parse_sgr_codes(&code, current_style);
            }
        } else {
            current_text.push(c);
            visible_count += 1;
        }
    }

    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    // If there's a search query and this line matches, highlight the matches
    if !search_query.is_empty() && is_match {
        spans = highlight_search_in_spans(spans, search_query);
    }

    if spans.is_empty() {
        Line::from("")
    } else {
        Line::from(spans)
    }
}

/// Highlight search query occurrences within spans.
fn highlight_search_in_spans(spans: Vec<Span<'static>>, query: &str) -> Vec<Span<'static>> {
    let query_lower = query.to_lowercase();
    let mut result = Vec::new();

    for span in spans {
        let text = span.content.to_string();
        let text_lower = text.to_lowercase();
        let style = span.style;

        let mut last_end = 0;
        for (start, _) in text_lower.match_indices(&query_lower) {
            // Add text before match
            if start > last_end {
                result.push(Span::styled(
                    text[last_end..start].to_string(),
                    style,
                ));
            }
            // Add highlighted match
            let end = start + query.len();
            result.push(Span::styled(
                text[start..end].to_string(),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
            last_end = end;
        }
        // Add remaining text
        if last_end < text.len() {
            result.push(Span::styled(
                text[last_end..].to_string(),
                style,
            ));
        }
    }

    result
}

/// Parse SGR (Select Graphic Rendition) codes.
fn parse_sgr_codes(code: &str, mut style: Style) -> Style {
    for part in code.split(';') {
        match part {
            "0" | "" => style = Style::default(),
            "1" => style = style.add_modifier(Modifier::BOLD),
            "2" => style = style.add_modifier(Modifier::DIM),
            "3" => style = style.add_modifier(Modifier::ITALIC),
            "4" => style = style.add_modifier(Modifier::UNDERLINED),
            "30" => style = style.fg(Color::Black),
            "31" => style = style.fg(Color::Red),
            "32" => style = style.fg(Color::Green),
            "33" => style = style.fg(Color::Yellow),
            "34" => style = style.fg(Color::Blue),
            "35" => style = style.fg(Color::Magenta),
            "36" => style = style.fg(Color::Cyan),
            "37" => style = style.fg(Color::White),
            "39" => style = style.fg(Color::Reset),
            "90" => style = style.fg(Color::DarkGray),
            "91" => style = style.fg(Color::LightRed),
            "92" => style = style.fg(Color::LightGreen),
            "93" => style = style.fg(Color::LightYellow),
            "94" => style = style.fg(Color::LightBlue),
            "95" => style = style.fg(Color::LightMagenta),
            "96" => style = style.fg(Color::LightCyan),
            "97" => style = style.fg(Color::White),
            _ => {}
        }
    }
    style
}

/// Run the TUI with a subprocess that does the actual work.
///
/// This spawns the swarm command as a subprocess to avoid stdout corruption.
/// The TUI only shows the chat file content (which the subprocess writes to).
pub fn run_tui_with_subprocess(chat_path: &str, args: Vec<String>) -> io::Result<()> {
    use std::process::{Command, Stdio};

    let (tx, rx) = mpsc::channel();
    let tx_clone = tx.clone();
    let chat_path = chat_path.to_string();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_for_tail = Arc::clone(&stop_flag);
    let stop_for_proc = Arc::clone(&stop_flag);

    let exe_path = std::env::current_exe()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("failed to get exe path: {}", e)))?;

    let proc_handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));

        let mut child = match Command::new(&exe_path)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = tx_clone.send(TuiMessage::AppendLine(
                    format!("❌ Failed to start subprocess: {}", e)
                ));
                let _ = tx_clone.send(TuiMessage::WorkComplete);
                return;
            }
        };

        loop {
            if stop_for_proc.load(Ordering::SeqCst) {
                let _ = child.kill();
                break;
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        let _ = tx_clone.send(TuiMessage::AppendLine(
                            "✅ Work complete! Press 'q' to exit.".to_string()
                        ));
                    } else {
                        let _ = tx_clone.send(TuiMessage::AppendLine(
                            format!("❌ Process exited with status: {}", status)
                        ));
                    }
                    let _ = tx_clone.send(TuiMessage::WorkComplete);
                    break;
                }
                Ok(None) => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    let _ = tx_clone.send(TuiMessage::AppendLine(
                        format!("❌ Error waiting for process: {}", e)
                    ));
                    let _ = tx_clone.send(TuiMessage::WorkComplete);
                    break;
                }
            }
        }
    });

    let tx_for_tail = tx.clone();
    let tail_handle = thread::spawn(move || {
        tail_chat_to_tui(&chat_path, tx_for_tail, stop_for_tail);
    });

    let result = run_tui(rx);

    stop_flag.store(true, Ordering::SeqCst);

    let _ = proc_handle.join();
    let _ = tail_handle.join();

    result
}

/// Tail a chat file and send lines to the TUI.
fn tail_chat_to_tui(path: &str, tx: Sender<TuiMessage>, stop: Arc<AtomicBool>) {
    use std::fs::File;
    use std::io::{BufReader, Read, Seek, SeekFrom};

    let mut offset: u64 = 0;

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }

        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        let len = match file.metadata() {
            Ok(m) => m.len(),
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        if len < offset {
            offset = 0;
        }

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(offset)).is_err() {
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        let mut new_content = String::new();
        let bytes_read = reader.read_to_string(&mut new_content).unwrap_or(0);

        if bytes_read > 0 {
            for line in new_content.lines() {
                if !line.is_empty() {
                    let colored_line = crate::color::chat_line(line);
                    if tx.send(TuiMessage::AppendLine(colored_line)).is_err() {
                        return;
                    }
                }
            }
            offset += bytes_read as u64;
        }

        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("Hello"), "Hello");
        assert_eq!(strip_ansi("\x1b[32mGreen\x1b[0m"), "Green");
        assert_eq!(strip_ansi("\x1b[1;32mBold Green\x1b[0m text"), "Bold Green text");
    }

    #[test]
    fn test_truncate_short_line() {
        let line = "Short";
        let result = truncate_and_parse_ansi_with_highlight(line, 10, "", false, false);
        assert_eq!(result.spans.len(), 1);
    }

    #[test]
    fn test_truncate_long_line() {
        let line = "This is a very long line that should be truncated";
        let result = truncate_and_parse_ansi_with_highlight(line, 10, "", false, false);
        let total_visible: usize = result.spans.iter()
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(total_visible, 10);
    }

    #[test]
    fn test_parse_sgr_codes_reset() {
        let style = parse_sgr_codes("0", Style::default().fg(Color::Red));
        assert_eq!(style, Style::default());
    }

    #[test]
    fn test_parse_sgr_codes_bold() {
        let style = parse_sgr_codes("1", Style::default());
        assert!(style.add_modifier == Modifier::BOLD);
    }
}
