use std::fmt;

use crate::agent;

use super::{Task, TaskList, TaskStatus};

impl TaskList {
    /// Parse a TASKS.md file content.
    ///
    /// Preserves document structure by storing non-task lines (section headings,
    /// blank lines) as prefixes on the following task. This ensures roundtrip
    /// fidelity when writing back.
    pub fn parse(content: &str) -> Self {
        let mut header = Vec::new();
        let mut tasks = Vec::new();
        let mut seen_task = false;
        let mut pending_prefix: Vec<String> = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            if let Some(mut task) = parse_task_line(line, line_num + 1) {
                // Attach any pending prefix lines to this task
                task.prefix = std::mem::take(&mut pending_prefix);
                tasks.push(task);
                seen_task = true;
            } else if !seen_task {
                // Before any task, everything goes to header
                header.push(line.to_string());
            } else {
                // After seeing at least one task, non-task lines become
                // prefix for the next task (or footer if no more tasks)
                pending_prefix.push(line.to_string());
            }
        }

        // Any remaining pending lines after the last task become footer
        let footer = pending_prefix;

        Self {
            header,
            tasks,
            footer,
        }
    }
}

impl fmt::Display for TaskList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut lines = Vec::new();

        for h in &self.header {
            lines.push(h.clone());
        }

        for task in &self.tasks {
            // Output any prefix lines (section headings, etc.) before this task
            for prefix_line in &task.prefix {
                lines.push(prefix_line.clone());
            }
            lines.push(task.to_line());
        }

        for footer_line in &self.footer {
            lines.push(footer_line.clone());
        }

        let mut result = lines.join("\n");
        if !result.ends_with('\n') {
            result.push('\n');
        }
        f.write_str(&result)
    }
}

/// Parse a single task line.
pub(super) fn parse_task_line(line: &str, line_number: usize) -> Option<Task> {
    let trimmed = line.trim();

    // Must start with "- ["
    if !trimmed.starts_with("- [") {
        return None;
    }

    // Find the closing bracket
    let bracket_end = trimmed.find(']')?;
    if bracket_end < 4 {
        return None;
    }

    let marker = &trimmed[3..bracket_end];
    let rest = trimmed[bracket_end + 1..].trim();

    // Parse based on marker
    let (status, description) = if marker == " " {
        // Unassigned: - [ ] description
        (TaskStatus::Unassigned, rest.to_string())
    } else if marker == "x" || marker == "X" {
        // Completed: - [x] description (A)
        // Extract the agent initial from the end
        if let Some(agent_start) = rest.rfind(" (") {
            if rest.ends_with(')') {
                let agent_part = &rest[agent_start + 2..rest.len() - 1];
                if agent_part.len() == 1 {
                    let initial = agent_part.chars().next()?;
                    if agent::is_valid_initial(initial) {
                        let desc = rest[..agent_start].to_string();
                        return Some(Task {
                            description: desc,
                            status: TaskStatus::Completed(initial.to_ascii_uppercase()),
                            line_number,
                            prefix: Vec::new(),
                        });
                    }
                }
            }
        }
        // Completed but no agent attribution (treat as completed by unknown)
        (TaskStatus::Completed('?'), rest.to_string())
    } else if marker.len() == 1 {
        // Assigned: - [A] description
        let initial = marker.chars().next()?;
        if agent::is_valid_initial(initial) {
            (
                TaskStatus::Assigned(initial.to_ascii_uppercase()),
                rest.to_string(),
            )
        } else {
            return None;
        }
    } else {
        return None;
    };

    Some(Task {
        description,
        status,
        line_number,
        prefix: Vec::new(),
    })
}
