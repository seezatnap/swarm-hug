//! Terminal color utilities using ANSI escape codes.
//!
//! Provides colored output for agent names, status messages, and timestamps.

/// ANSI color codes
pub mod codes {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";

    // Standard colors
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[37m";

    // Bright colors (for more variety)
    pub const BRIGHT_RED: &str = "\x1b[91m";
    pub const BRIGHT_GREEN: &str = "\x1b[92m";
    pub const BRIGHT_YELLOW: &str = "\x1b[93m";
    pub const BRIGHT_BLUE: &str = "\x1b[94m";
    pub const BRIGHT_MAGENTA: &str = "\x1b[95m";
    pub const BRIGHT_CYAN: &str = "\x1b[96m";
}

use codes::*;

/// Colors for agent names - deterministic based on agent initial
const AGENT_COLORS: &[&str] = &[
    CYAN,
    MAGENTA,
    YELLOW,
    BLUE,
    BRIGHT_CYAN,
    BRIGHT_MAGENTA,
    BRIGHT_YELLOW,
    BRIGHT_BLUE,
    GREEN,
    BRIGHT_GREEN,
];

/// Get a deterministic color for an agent based on their initial.
pub fn agent_color(initial: char) -> &'static str {
    let index = (initial.to_ascii_uppercase() as usize).wrapping_sub('A' as usize);
    AGENT_COLORS[index % AGENT_COLORS.len()]
}

/// Color an agent name deterministically.
pub fn agent(name: &str) -> String {
    let initial = name.chars().next().unwrap_or('A');
    let color = agent_color(initial);
    format!("{}{}{}{}", BOLD, color, name, RESET)
}

/// Color an agent name with their initial for display.
pub fn agent_with_initial(name: &str, initial: char) -> String {
    let color = agent_color(initial);
    format!("{}{}{}({}){}", BOLD, color, name, initial, RESET)
}

/// Color a timestamp (dim white).
pub fn timestamp(ts: &str) -> String {
    format!("{}{}{}", DIM, ts, RESET)
}

/// Color "Completed" status (green + bold).
pub fn completed(text: &str) -> String {
    format!("{}{}{}{}",BOLD, GREEN, text, RESET)
}

/// Color "Failed" status (red + bold).
pub fn failed(text: &str) -> String {
    format!("{}{}{}{}", BOLD, RED, text, RESET)
}

/// Color success messages (green).
pub fn success(text: &str) -> String {
    format!("{}{}{}", GREEN, text, RESET)
}

/// Color error messages (red).
pub fn error(text: &str) -> String {
    format!("{}{}{}", RED, text, RESET)
}

/// Color warning messages (yellow).
pub fn warning(text: &str) -> String {
    format!("{}{}{}", YELLOW, text, RESET)
}

/// Color info messages (cyan).
pub fn info(text: &str) -> String {
    format!("{}{}{}", CYAN, text, RESET)
}

/// Color a label (bold).
pub fn label(text: &str) -> String {
    format!("{}{}{}", BOLD, text, RESET)
}

/// Color a number/count (bright cyan).
pub fn number(n: impl std::fmt::Display) -> String {
    format!("{}{}{}", BRIGHT_CYAN, n, RESET)
}

/// Colorize a chat line in the format: "timestamp | agent_name | message"
/// Colors the timestamp (dim), agent name (deterministic color), and highlights
/// "Completed:" (green) and "Failed:" (red) in the message.
pub fn chat_line(line: &str) -> String {
    // Parse the line format: "timestamp | agent_name | message"
    let parts: Vec<&str> = line.splitn(3, " | ").collect();
    if parts.len() != 3 {
        // If format doesn't match, return as-is
        return line.to_string();
    }

    let ts = parts[0];
    let agent_name = parts[1];
    let message = parts[2];

    // Color the message, highlighting Completed/Failed/Starting
    let colored_message = if message.contains("Completed:") {
        message.replace("Completed:", &format!("{}{}Completed:{}", BOLD, GREEN, RESET))
    } else if message.contains("Failed:") {
        message.replace("Failed:", &format!("{}{}Failed:{}", BOLD, RED, RESET))
    } else if message.contains("Starting:") {
        message.replace("Starting:", &format!("{}Starting:{}", CYAN, RESET))
    } else {
        message.to_string()
    };

    format!(
        "{} | {} | {}",
        timestamp(ts),
        agent(agent_name),
        colored_message
    )
}

/// Emoji constants for consistent usage
pub mod emoji {
    pub const ROCKET: &str = "🚀";
    pub const CHECK: &str = "✅";
    pub const CROSS: &str = "❌";
    pub const WARNING: &str = "⚠️";
    pub const HOURGLASS: &str = "⏳";
    pub const SPRINT: &str = "🏃";
    pub const TASK: &str = "📋";
    pub const PACKAGE: &str = "📦";
    pub const GEAR: &str = "⚙️";
    pub const SPARKLES: &str = "✨";
    pub const BRAIN: &str = "🧠";
    pub const ROBOT: &str = "🤖";
    pub const FOLDER: &str = "📁";
    pub const BRANCH: &str = "🌿";
    pub const MERGE: &str = "🔀";
    pub const CLOCK: &str = "🕐";
    pub const FIRE: &str = "🔥";
    pub const BUG: &str = "🐛";
    pub const WRENCH: &str = "🔧";
    pub const LINK: &str = "🔗";
    pub const STOP: &str = "🛑";
    pub const WAVE: &str = "👋";
    pub const PARTY: &str = "🎉";
    pub const THINKING: &str = "💭";
    pub const ZAP: &str = "⚡";
    pub const AGENT: &str = "🤖";
    pub const TEAM: &str = "👥";
    pub const NUMBER: &str = "🔢";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_color_deterministic() {
        // Same initial should always get same color
        assert_eq!(agent_color('A'), agent_color('A'));
        assert_eq!(agent_color('B'), agent_color('B'));
    }

    #[test]
    fn test_agent_color_varies() {
        // Different initials should (usually) get different colors
        // Note: with 26 letters and 10 colors, some will repeat
        let color_a = agent_color('A');
        let color_b = agent_color('B');
        // A and B should have different colors since they're consecutive
        assert_ne!(color_a, color_b);
    }

    #[test]
    fn test_agent_name_colored() {
        let colored = agent("Aaron");
        assert!(colored.contains("Aaron"));
        assert!(colored.contains(RESET));
    }

    #[test]
    fn test_completed_green_bold() {
        let text = completed("Completed");
        assert!(text.contains(GREEN));
        assert!(text.contains(BOLD));
        assert!(text.contains(RESET));
    }

    #[test]
    fn test_failed_red_bold() {
        let text = failed("Failed");
        assert!(text.contains(RED));
        assert!(text.contains(BOLD));
        assert!(text.contains(RESET));
    }

    #[test]
    fn test_timestamp_dim() {
        let text = timestamp("12:34:56");
        assert!(text.contains(DIM));
        assert!(text.contains(RESET));
    }

    #[test]
    fn test_chat_line_completed() {
        let line = "2026-01-26 00:01:26 | Aaron | AGENT_THINK: Completed: Task one";
        let colored = chat_line(line);
        assert!(colored.contains(GREEN), "Completed should be green");
        assert!(colored.contains("Aaron"), "Should contain agent name");
        assert!(colored.contains(DIM), "Timestamp should be dim");
    }

    #[test]
    fn test_chat_line_failed() {
        let line = "2026-01-26 00:01:26 | Betty | AGENT_THINK: Failed: Task two - error";
        let colored = chat_line(line);
        assert!(colored.contains(RED), "Failed should be red");
        assert!(colored.contains("Betty"), "Should contain agent name");
    }

    #[test]
    fn test_chat_line_starting() {
        let line = "2026-01-26 00:01:26 | Carlos | AGENT_THINK: Starting: Task three";
        let colored = chat_line(line);
        assert!(colored.contains(CYAN), "Starting should be cyan");
        assert!(colored.contains("Carlos"), "Should contain agent name");
    }

    #[test]
    fn test_chat_line_invalid_format() {
        let line = "this is not a valid chat line";
        let colored = chat_line(line);
        assert_eq!(colored, line, "Invalid format should be returned as-is");
    }
}
