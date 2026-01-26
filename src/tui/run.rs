use std::io;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use super::app::TuiApp;
use super::message::TuiMessage;
use super::render::draw_ui;

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
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        app.handle_mouse_scroll(true);
                    }
                    MouseEventKind::ScrollDown => {
                        app.handle_mouse_scroll(false);
                    }
                    _ => {}
                },
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
