use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use super::ansi::truncate_and_parse_ansi_with_highlight;
use super::app::{InputMode, TuiApp};

/// ASCII art header for SWARM HUG
const HEADER: &str = "\u{250f}\u{2501}\u{2513}\u{257b} \u{257b}\u{250f}\u{2501}\u{2513}\u{250f}\u{2501}\u{2513}\u{250f}\u{2533}\u{2513}   \u{257b} \u{257b}\u{257b} \u{257b}\u{250f}\u{2501}\u{2578}\n\u{2517}\u{2501}\u{2513}\u{2503}\u{257b}\u{2503}\u{2523}\u{2501}\u{252b}\u{2523}\u{2533}\u{251b}\u{2503}\u{2503}\u{2503}   \u{2523}\u{2501}\u{252b}\u{2503} \u{2503}\u{2503}\u{257a}\u{2513}\n\u{2517}\u{2501}\u{251b}\u{2517}\u{253b}\u{251b}\u{2579} \u{2579}\u{2579}\u{2517}\u{2578}\u{2579} \u{2579}   \u{2579} \u{2579}\u{2517}\u{2501}\u{251b}\u{2517}\u{2501}\u{251b}";

/// Padding inside content area (1 cell on each side)
const CONTENT_PADDING: u16 = 1;

/// Draw the UI. Returns the inner content height for search navigation.
pub(super) fn draw_ui(f: &mut Frame, app: &TuiApp) -> usize {
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
    let inner_height = area
        .height
        .saturating_sub(border_size + CONTENT_PADDING * 2) as usize;

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
            " Output ({}/{}) [match {}/{}] [\u{2191}\u{2193} scroll, / search, n/N next/prev, q quit] ",
            if total_lines > 0 {
                total_lines.saturating_sub(app.scroll_offset)
            } else {
                0
            },
            total_lines,
            app.current_match + 1,
            app.search_matches.len()
        )
    } else {
        format!(
            " Output ({}/{}) [\u{2191}\u{2193} scroll, / search, q quit] ",
            if total_lines > 0 {
                total_lines.saturating_sub(app.scroll_offset)
            } else {
                0
            },
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

    let search_bar = Paragraph::new(search_line).block(
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
    let modal_area = Rect::new(
        x,
        y,
        modal_width.min(area.width),
        modal_height.min(area.height),
    );

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
