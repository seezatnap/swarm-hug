//! Terminal User Interface using ratatui.
//!
//! Provides a scrollable output pane with a header and quit confirmation modal.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
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
                    // Auto-scroll stays at bottom when new content arrives
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

    /// Handle a key event.
    fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
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
        } else {
            match key {
                KeyCode::Char('q') | KeyCode::Char('Q') => {
                    if self.work_complete {
                        // If work is done, quit immediately
                        self.should_quit = true;
                    } else {
                        // Show confirmation modal
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
                    // Scroll up (increase offset)
                    let max_scroll = self.lines.len().saturating_sub(1);
                    self.scroll_offset = (self.scroll_offset + 1).min(max_scroll);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    // Scroll down (decrease offset)
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
                _ => {}
            }
        }
    }
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

    loop {
        // Process any pending messages
        app.process_messages();

        // Draw the UI
        terminal.draw(|f| draw_ui(f, &app))?;

        // Handle events with a timeout so we can process messages
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key.code, key.modifiers);
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

/// Draw the UI.
fn draw_ui(f: &mut Frame, app: &TuiApp) {
    let size = f.area();

    // Clear the entire frame first
    f.render_widget(Clear, size);

    // Create main layout: header + content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Header (3 lines + bottom border)
            Constraint::Min(0),    // Content
        ])
        .split(size);

    // Draw header
    draw_header(f, chunks[0]);

    // Draw content
    draw_content(f, chunks[1], app);

    // Draw quit modal if showing
    if app.show_quit_modal {
        draw_quit_modal(f, size);
    }
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

/// Draw the scrollable content area.
fn draw_content(f: &mut Frame, area: Rect, app: &TuiApp) {
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
        return;
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
    let visible_lines: Vec<Line> = app.lines[start_idx..end_idx]
        .iter()
        .map(|line| truncate_and_parse_ansi(line, inner_width))
        .collect();

    // Create the block with borders
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(format!(
            " Output ({}/{}) [↑↓ scroll, q quit] ",
            if total_lines > 0 {
                total_lines.saturating_sub(app.scroll_offset)
            } else {
                0
            },
            total_lines
        ))
        .title_style(Style::default().fg(Color::White));

    // Render the block first
    f.render_widget(block, area);

    // Calculate the inner area with padding
    let inner_area = area.inner(Margin {
        horizontal: CONTENT_PADDING + 1, // +1 for border
        vertical: CONTENT_PADDING + 1,   // +1 for border
    });

    // Render the content in the inner area (no wrapping!)
    let content = Paragraph::new(visible_lines);
    f.render_widget(content, inner_area);
}

/// Draw the quit confirmation modal.
fn draw_quit_modal(f: &mut Frame, area: Rect) {
    // Center the modal
    let modal_width = 40u16;
    let modal_height = 7u16;
    let x = (area.width.saturating_sub(modal_width)) / 2;
    let y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(x, y, modal_width.min(area.width), modal_height.min(area.height));

    // Clear the area behind the modal
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

/// Truncate a line to fit within max_width (counting visible characters, not ANSI codes)
/// and parse ANSI codes into ratatui Spans.
fn truncate_and_parse_ansi(line: &str, max_width: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let mut current_text = String::new();
    let mut current_style = Style::default();
    let mut chars = line.chars().peekable();
    let mut visible_count = 0;

    while let Some(c) = chars.next() {
        // Stop if we've reached the max visible width
        if visible_count >= max_width {
            break;
        }

        if c == '\x1b' {
            // Save current text with current style
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }

            // Parse escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                let mut code = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_alphabetic() {
                        chars.next(); // consume the letter (e.g., 'm')
                        break;
                    }
                    code.push(chars.next().unwrap());
                }

                // Parse the SGR codes (only if it was 'm' for color)
                current_style = parse_sgr_codes(&code, current_style);
            }
        } else {
            current_text.push(c);
            visible_count += 1;
        }
    }

    // Don't forget remaining text
    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    if spans.is_empty() {
        Line::from("")
    } else {
        Line::from(spans)
    }
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
            "39" => style = style.fg(Color::Reset), // default foreground
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

    // Flag to signal threads to stop
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_for_tail = Arc::clone(&stop_flag);
    let stop_for_proc = Arc::clone(&stop_flag);

    // Get the current executable path
    let exe_path = std::env::current_exe()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("failed to get exe path: {}", e)))?;

    // Start the subprocess thread
    let proc_handle = thread::spawn(move || {
        // Small delay to let TUI initialize
        thread::sleep(Duration::from_millis(100));

        let mut child = match Command::new(&exe_path)
            .args(&args)
            .stdout(Stdio::null()) // Suppress stdout (TUI shows chat file instead)
            .stderr(Stdio::null()) // Suppress stderr
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

        // Wait for subprocess or stop signal
        loop {
            if stop_for_proc.load(Ordering::SeqCst) {
                // Kill the subprocess
                let _ = child.kill();
                break;
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process exited
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
                    // Still running
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

    // Start the tail thread
    let tx_for_tail = tx.clone();
    let tail_handle = thread::spawn(move || {
        tail_chat_to_tui(&chat_path, tx_for_tail, stop_for_tail);
    });

    // Run the TUI in the main thread
    let result = run_tui(rx);

    // Signal threads to stop
    stop_flag.store(true, Ordering::SeqCst);

    // Wait for threads
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
        // Check stop flag
        if stop.load(Ordering::SeqCst) {
            break;
        }

        // Try to open the file
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

        // If file was truncated, start from beginning
        if len < offset {
            offset = 0;
        }

        // Read new content
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
                    // Apply color formatting to chat lines
                    let colored_line = crate::color::chat_line(line);
                    if tx.send(TuiMessage::AppendLine(colored_line)).is_err() {
                        // Channel closed
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
    fn test_parse_ansi_line_plain() {
        let line = "Hello, World!";
        let result = truncate_and_parse_ansi(line, 100);
        assert_eq!(result.spans.len(), 1);
    }

    #[test]
    fn test_parse_ansi_line_colored() {
        let line = "\x1b[32mGreen\x1b[0m Normal";
        let result = truncate_and_parse_ansi(line, 100);
        assert!(result.spans.len() >= 2);
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

    #[test]
    fn test_truncate_short_line() {
        let line = "Short";
        let result = truncate_and_parse_ansi(line, 10);
        assert_eq!(result.spans.len(), 1);
    }

    #[test]
    fn test_truncate_long_line() {
        let line = "This is a very long line that should be truncated";
        let result = truncate_and_parse_ansi(line, 10);
        // The visible text should be truncated to 10 chars
        let total_visible: usize = result.spans.iter()
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(total_visible, 10);
    }

    #[test]
    fn test_truncate_with_ansi_codes() {
        // Line with ANSI codes - should truncate based on visible chars only
        let line = "\x1b[32mGreen\x1b[0m and more text";
        let result = truncate_and_parse_ansi(line, 8);
        // "Green an" = 8 visible chars
        let total_visible: usize = result.spans.iter()
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(total_visible, 8);
    }
}
