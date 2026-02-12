use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Strip ANSI escape codes from a string.
pub(super) fn strip_ansi(s: &str) -> String {
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

/// Truncate a line to fit within max_width, parse ANSI codes, and optionally highlight search matches.
pub(super) fn truncate_and_parse_ansi_with_highlight(
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
            "\u{25b6} ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        visible_count += 2;
    } else if is_match {
        spans.push(Span::styled("  ", Style::default()));
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
                result.push(Span::styled(text[last_end..start].to_string(), style));
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
            result.push(Span::styled(text[last_end..].to_string(), style));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("Hello"), "Hello");
        assert_eq!(strip_ansi("\x1b[32mGreen\x1b[0m"), "Green");
        assert_eq!(
            strip_ansi("\x1b[1;32mBold Green\x1b[0m text"),
            "Bold Green text"
        );
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
        let total_visible: usize = result.spans.iter().map(|s| s.content.chars().count()).sum();
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
