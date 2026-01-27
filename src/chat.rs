//! CHAT.md writer and reader.
//!
//! All communication is appended to CHAT.md with the format:
//! `YYYY-MM-DD HH:MM:SS | <AgentName> | <message>`

use chrono::Local;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

const HEARTBEAT_PREFIX: &str = "AGENT_ACTIVITY:";

/// Format a chat message for CHAT.md.
///
/// # Examples
/// ```
/// use swarm::chat::format_message;
/// let msg = format_message("Aaron", "Starting task");
/// assert!(msg.contains("Aaron"));
/// assert!(msg.contains("Starting task"));
/// ```
pub fn format_message(agent_name: &str, message: &str) -> String {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    format!("{} | {} | {}", timestamp, agent_name, message)
}

/// Format a chat message with a custom timestamp (for testing).
pub fn format_message_with_timestamp(timestamp: &str, agent_name: &str, message: &str) -> String {
    format!("{} | {} | {}", timestamp, agent_name, message)
}

/// Append a message to CHAT.md.
pub fn write_message<P: AsRef<Path>>(path: P, agent_name: &str, message: &str) -> io::Result<()> {
    let line = format_message(agent_name, message);
    append_line(path, &line)
}

/// Append a heartbeat message to CHAT.md.
pub fn write_heartbeat<P: AsRef<Path>>(
    path: P,
    agent_name: &str,
    message: &str,
) -> io::Result<()> {
    let msg = format!("{} {}", HEARTBEAT_PREFIX, message);
    write_message(path, agent_name, &msg)
}

/// Check if a chat line is a heartbeat entry.
pub fn is_heartbeat_line(line: &str) -> bool {
    parse_line(line)
        .map(|(_, _, message)| message.trim_start().starts_with(HEARTBEAT_PREFIX))
        .unwrap_or(false)
}

/// Append a raw line to a file.
fn append_line<P: AsRef<Path>>(path: P, line: &str) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", line)
}

/// Read recent lines from CHAT.md.
pub fn read_recent<P: AsRef<Path>>(path: P, count: usize) -> io::Result<Vec<String>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;

    if lines.len() <= count {
        Ok(lines)
    } else {
        Ok(lines[lines.len() - count..].to_vec())
    }
}

/// Read all messages from a specific agent.
pub fn read_from_agent<P: AsRef<Path>>(path: P, agent_name: &str) -> io::Result<Vec<String>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let pattern = format!("| {} |", agent_name);

    let lines: Vec<String> = reader
        .lines()
        .filter_map(|line| line.ok())
        .filter(|line| line.contains(&pattern))
        .collect();

    Ok(lines)
}

/// Write a sprint plan summary to CHAT.md.
pub fn write_sprint_plan<P: AsRef<Path>>(
    path: P,
    sprint_number: usize,
    assignments: &[(char, &str)],
) -> io::Result<()> {
    let summary = format!(
        "Sprint {} plan: {} task(s) assigned",
        sprint_number,
        assignments.len()
    );
    write_message(&path, "ScrumMaster", &summary)?;

    for (initial, description) in assignments {
        let agent_name = crate::agent::name_from_initial(*initial).unwrap_or("Unknown");
        let msg = format!("{} assigned: {}", agent_name, description);
        write_message(&path, "ScrumMaster", &msg)?;
    }

    Ok(())
}

/// Write a sprint status summary to CHAT.md.
pub fn write_sprint_status<P: AsRef<Path>>(
    path: P,
    team_name: &str,
    sprint_number: usize,
    completed_this_sprint: usize,
    failed_this_sprint: usize,
    remaining_tasks: usize,
    total_tasks: usize,
) -> io::Result<()> {
    let header = format!(
        "SPRINT STATUS: {} Sprint {} complete",
        team_name, sprint_number
    );
    write_message(&path, "ScrumMaster", &header)?;
    write_message(
        &path,
        "ScrumMaster",
        &format!(
            "SPRINT STATUS: Completed this sprint: {}",
            completed_this_sprint
        ),
    )?;
    write_message(
        &path,
        "ScrumMaster",
        &format!("SPRINT STATUS: Failed this sprint: {}", failed_this_sprint),
    )?;
    write_message(
        &path,
        "ScrumMaster",
        &format!("SPRINT STATUS: Remaining tasks: {}", remaining_tasks),
    )?;
    write_message(
        &path,
        "ScrumMaster",
        &format!("SPRINT STATUS: Total tasks: {}", total_tasks),
    )?;

    Ok(())
}

/// Clear a chat file and write a boot message.
///
/// This clears the chat.md file and writes the "SWARM HUG BOOTING UP" message.
pub fn write_boot_message<P: AsRef<Path>>(path: P) -> io::Result<()> {
    // Truncate the file (clear all contents)
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)?;

    // Write the boot banner
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let banner = format!(
        "{} | ScrumMaster | 🚀🐝 SWARM HUG BOOTING UP 🐝🚀",
        timestamp
    );
    writeln!(file, "{}", banner)
}

/// Write a merge status to CHAT.md.
pub fn write_merge_status<P: AsRef<Path>>(
    path: P,
    agent_name: &str,
    success: bool,
    message: &str,
) -> io::Result<()> {
    let status = if success { "success" } else { "conflict" };
    let msg = format!("Merge {} for {}: {}", status, agent_name, message);
    write_message(path, "ScrumMaster", &msg)
}

/// Parse a chat line into (timestamp, agent_name, message).
pub fn parse_line(line: &str) -> Option<(&str, &str, &str)> {
    // Format: YYYY-MM-DD HH:MM:SS | AgentName | message
    let parts: Vec<&str> = line.splitn(3, " | ").collect();
    if parts.len() != 3 {
        return None;
    }

    let timestamp = parts[0];
    let agent_name = parts[1];
    let message = parts[2];

    Some((timestamp, agent_name, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_format_message() {
        let msg = format_message("Aaron", "Starting task");
        assert!(msg.contains("Aaron"));
        assert!(msg.contains("Starting task"));
        // Check timestamp format
        assert!(msg.contains("-"));
        assert!(msg.contains(":"));
    }

    #[test]
    fn test_format_message_with_timestamp() {
        let msg = format_message_with_timestamp("2024-01-15 10:30:00", "Aaron", "Hello");
        assert_eq!(msg, "2024-01-15 10:30:00 | Aaron | Hello");
    }

    #[test]
    fn test_write_message() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        write_message(path, "Aaron", "Starting task").unwrap();
        write_message(path, "Betty", "Also starting").unwrap();

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("Aaron"));
        assert!(content.contains("Betty"));
        assert_eq!(content.lines().count(), 2);
    }

    #[test]
    fn test_write_heartbeat_and_detect() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        write_heartbeat(path, "Aaron", "Still working").unwrap();

        let content = std::fs::read_to_string(path).unwrap();
        let line = content.lines().next().unwrap();
        assert!(is_heartbeat_line(line));
    }

    #[test]
    fn test_is_heartbeat_line_false_for_normal_message() {
        let line = format_message_with_timestamp(
            "2024-01-15 10:30:00",
            "Aaron",
            "Starting task",
        );
        assert!(!is_heartbeat_line(&line));
    }

    #[test]
    fn test_read_recent() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        for i in 1..=10 {
            let msg = format!("Message {}", i);
            write_message(path, "Aaron", &msg).unwrap();
        }

        let recent = read_recent(path, 3).unwrap();
        assert_eq!(recent.len(), 3);
        assert!(recent[0].contains("Message 8"));
        assert!(recent[1].contains("Message 9"));
        assert!(recent[2].contains("Message 10"));
    }

    #[test]
    fn test_read_recent_fewer_lines() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        write_message(path, "Aaron", "Only one").unwrap();

        let recent = read_recent(path, 10).unwrap();
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn test_read_from_agent() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        write_message(path, "Aaron", "Message 1").unwrap();
        write_message(path, "Betty", "Message 2").unwrap();
        write_message(path, "Aaron", "Message 3").unwrap();

        let aaron_lines = read_from_agent(path, "Aaron").unwrap();
        assert_eq!(aaron_lines.len(), 2);
        assert!(aaron_lines[0].contains("Message 1"));
        assert!(aaron_lines[1].contains("Message 3"));
    }

    #[test]
    fn test_parse_line() {
        let line = "2024-01-15 10:30:00 | Aaron | Starting task";
        let (timestamp, agent, message) = parse_line(line).unwrap();
        assert_eq!(timestamp, "2024-01-15 10:30:00");
        assert_eq!(agent, "Aaron");
        assert_eq!(message, "Starting task");
    }

    #[test]
    fn test_parse_line_invalid() {
        assert!(parse_line("invalid line").is_none());
        assert!(parse_line("").is_none());
    }

    #[test]
    fn test_write_sprint_plan() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        let assignments = vec![
            ('A', "Task 1"),
            ('B', "Task 2"),
        ];

        write_sprint_plan(path, 1, &assignments).unwrap();

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("Sprint 1 plan: 2 task(s) assigned"));
        assert!(content.contains("Aaron assigned: Task 1"));
        assert!(content.contains("Betty assigned: Task 2"));
    }

    #[test]
    fn test_write_sprint_status() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        write_sprint_status(path, "Alpha", 3, 2, 1, 4, 7).unwrap();

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("SPRINT STATUS: Alpha Sprint 3 complete"));
        assert!(content.contains("SPRINT STATUS: Completed this sprint: 2"));
        assert!(content.contains("SPRINT STATUS: Failed this sprint: 1"));
        assert!(content.contains("SPRINT STATUS: Remaining tasks: 4"));
        assert!(content.contains("SPRINT STATUS: Total tasks: 7"));
    }

    #[test]
    fn test_write_merge_status_success() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        write_merge_status(path, "Aaron", true, "Merged branch agent-aaron to main").unwrap();

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("Merge success for Aaron"));
    }

    #[test]
    fn test_write_merge_status_conflict() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        write_merge_status(path, "Betty", false, "Conflicts in file.txt").unwrap();

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("Merge conflict for Betty"));
    }

    #[test]
    fn test_write_boot_message() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        // Write some initial content
        write_message(path, "Aaron", "Some old message").unwrap();

        // Boot message should clear and write new content
        write_boot_message(path).unwrap();

        let content = std::fs::read_to_string(path).unwrap();
        // Should contain the boot banner
        assert!(content.contains("SWARM HUG BOOTING UP"));
        assert!(content.contains("ScrumMaster"));
        // Should NOT contain old content (was cleared)
        assert!(!content.contains("Some old message"));
        // Should only have one line
        assert_eq!(content.lines().count(), 1);
    }
}
