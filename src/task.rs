//! Task file (TASKS.md) parser and writer.
//!
//! Supports the checklist format:
//! - `- [ ] Task description` (unassigned)
//! - `- [A] Task description` (assigned to Aaron)
//! - `- [x] Task description (A)` (completed by Aaron)

use crate::agent;

/// Task status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Unassigned task: `- [ ] ...`
    Unassigned,
    /// Assigned to an agent: `- [A] ...`
    Assigned(char),
    /// Completed by an agent: `- [x] ... (A)`
    Completed(char),
}

/// A single task parsed from TASKS.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// The task description (without the checkbox prefix).
    pub description: String,
    /// The task status.
    pub status: TaskStatus,
    /// Original line number (1-indexed) for error reporting.
    pub line_number: usize,
    /// Lines that appeared before this task (section headings, blank lines, etc.).
    /// This preserves document structure when writing back.
    pub prefix: Vec<String>,
}

impl Task {
    /// Create a new unassigned task.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            status: TaskStatus::Unassigned,
            line_number: 0,
            prefix: Vec::new(),
        }
    }

    /// Check if this task is blocked.
    ///
    /// Blocked detection recognizes: `BLOCKED`, `blocked`, `Blocked by:`.
    pub fn is_blocked(&self) -> bool {
        let desc = &self.description;
        desc.contains("BLOCKED")
            || desc.contains("blocked")
            || desc.contains("Blocked by:")
    }

    /// Assign this task to an agent.
    pub fn assign(&mut self, initial: char) {
        if matches!(self.status, TaskStatus::Unassigned) {
            self.status = TaskStatus::Assigned(initial.to_ascii_uppercase());
        }
    }

    /// Unassign this task (revert from Assigned to Unassigned).
    /// Only affects tasks that are currently Assigned, not Completed.
    pub fn unassign(&mut self) {
        if matches!(self.status, TaskStatus::Assigned(_)) {
            self.status = TaskStatus::Unassigned;
        }
    }

    /// Mark this task as completed.
    pub fn complete(&mut self, initial: char) {
        self.status = TaskStatus::Completed(initial.to_ascii_uppercase());
    }

    /// Check if this task is assignable (unassigned and not blocked).
    pub fn is_assignable(&self) -> bool {
        matches!(self.status, TaskStatus::Unassigned) && !self.is_blocked()
    }

    /// Format this task as a TASKS.md line.
    pub fn to_line(&self) -> String {
        match self.status {
            TaskStatus::Unassigned => format!("- [ ] {}", self.description),
            TaskStatus::Assigned(initial) => format!("- [{}] {}", initial, self.description),
            TaskStatus::Completed(initial) => format!("- [x] {} ({})", self.description, initial),
        }
    }
}

/// A collection of tasks parsed from TASKS.md.
#[derive(Debug, Clone, Default)]
pub struct TaskList {
    /// Header lines before the first task (preserved on write).
    pub header: Vec<String>,
    /// The tasks in backlog order (top to bottom priority).
    pub tasks: Vec<Task>,
    /// Footer lines after the last task (preserved on write).
    pub footer: Vec<String>,
}

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

        Self { header, tasks, footer }
    }

    /// Format tasks back to TASKS.md content.
    ///
    /// Preserves document structure by outputting each task's prefix lines
    /// (section headings, blank lines) before the task itself.
    pub fn to_string(&self) -> String {
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

        for f in &self.footer {
            lines.push(f.clone());
        }

        let mut result = lines.join("\n");
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result
    }

    /// Get count of unassigned tasks.
    pub fn unassigned_count(&self) -> usize {
        self.tasks.iter().filter(|t| matches!(t.status, TaskStatus::Unassigned)).count()
    }

    /// Get count of assigned tasks.
    pub fn assigned_count(&self) -> usize {
        self.tasks.iter().filter(|t| matches!(t.status, TaskStatus::Assigned(_))).count()
    }

    /// Unassign all currently assigned tasks.
    /// This is used at sprint start to reset incomplete tasks from previous sprints.
    /// Returns the number of tasks that were unassigned.
    pub fn unassign_all(&mut self) -> usize {
        let mut count = 0;
        for task in &mut self.tasks {
            if matches!(task.status, TaskStatus::Assigned(_)) {
                task.unassign();
                count += 1;
            }
        }
        count
    }

    /// Get count of completed tasks.
    pub fn completed_count(&self) -> usize {
        self.tasks.iter().filter(|t| matches!(t.status, TaskStatus::Completed(_))).count()
    }

    /// Get count of assignable tasks (unassigned and not blocked).
    pub fn assignable_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.is_assignable()).count()
    }

    /// Get tasks assigned to a specific agent.
    pub fn tasks_for_agent(&self, initial: char) -> Vec<&Task> {
        let upper = initial.to_ascii_uppercase();
        self.tasks.iter()
            .filter(|t| matches!(t.status, TaskStatus::Assigned(i) if i == upper))
            .collect()
    }

    /// Assign tasks to agents for a sprint.
    ///
    /// Returns the number of tasks assigned.
    pub fn assign_sprint(&mut self, agent_initials: &[char], tasks_per_agent: usize) -> usize {
        let mut assigned = 0;
        let mut agent_task_count: std::collections::HashMap<char, usize> = std::collections::HashMap::new();

        for task in &mut self.tasks {
            if !task.is_assignable() {
                continue;
            }

            // Find an agent with capacity
            for &initial in agent_initials {
                let count = agent_task_count.entry(initial).or_insert(0);
                if *count < tasks_per_agent {
                    task.assign(initial);
                    *count += 1;
                    assigned += 1;
                    break;
                }
            }
        }

        assigned
    }
}

/// Parse a single task line.
fn parse_task_line(line: &str, line_number: usize) -> Option<Task> {
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
            (TaskStatus::Assigned(initial.to_ascii_uppercase()), rest.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_unassigned() {
        let task = parse_task_line("- [ ] Write tests", 1).unwrap();
        assert_eq!(task.description, "Write tests");
        assert_eq!(task.status, TaskStatus::Unassigned);
    }

    #[test]
    fn test_parse_assigned() {
        let task = parse_task_line("- [A] Write tests", 1).unwrap();
        assert_eq!(task.description, "Write tests");
        assert_eq!(task.status, TaskStatus::Assigned('A'));
    }

    #[test]
    fn test_parse_assigned_lowercase() {
        let task = parse_task_line("- [a] Write tests", 1).unwrap();
        assert_eq!(task.description, "Write tests");
        assert_eq!(task.status, TaskStatus::Assigned('A'));
    }

    #[test]
    fn test_parse_completed() {
        let task = parse_task_line("- [x] Write tests (A)", 1).unwrap();
        assert_eq!(task.description, "Write tests");
        assert_eq!(task.status, TaskStatus::Completed('A'));
    }

    #[test]
    fn test_parse_completed_uppercase_x() {
        let task = parse_task_line("- [X] Write tests (B)", 1).unwrap();
        assert_eq!(task.description, "Write tests");
        assert_eq!(task.status, TaskStatus::Completed('B'));
    }

    #[test]
    fn test_parse_not_a_task() {
        assert!(parse_task_line("# Header", 1).is_none());
        assert!(parse_task_line("Some text", 1).is_none());
        assert!(parse_task_line("", 1).is_none());
    }

    #[test]
    fn test_task_is_blocked() {
        let mut task = Task::new("Fix the BLOCKED feature");
        assert!(task.is_blocked());

        task.description = "This is blocked by something".to_string();
        assert!(task.is_blocked());

        task.description = "Blocked by: issue #123".to_string();
        assert!(task.is_blocked());

        task.description = "Normal task".to_string();
        assert!(!task.is_blocked());
    }

    #[test]
    fn test_task_assign() {
        let mut task = Task::new("Write tests");
        assert!(task.is_assignable());

        task.assign('a');
        assert_eq!(task.status, TaskStatus::Assigned('A'));
        assert!(!task.is_assignable());
    }

    #[test]
    fn test_task_complete() {
        let mut task = Task::new("Write tests");
        task.assign('B');
        task.complete('B');
        assert_eq!(task.status, TaskStatus::Completed('B'));
    }

    #[test]
    fn test_task_to_line() {
        let mut task = Task::new("Write tests");
        assert_eq!(task.to_line(), "- [ ] Write tests");

        task.assign('A');
        assert_eq!(task.to_line(), "- [A] Write tests");

        task.complete('A');
        assert_eq!(task.to_line(), "- [x] Write tests (A)");
    }

    #[test]
    fn test_tasklist_parse() {
        let content = "# Tasks\n\n- [ ] Task 1\n- [A] Task 2\n- [x] Task 3 (B)\n";
        let list = TaskList::parse(content);

        assert_eq!(list.header.len(), 2); // "# Tasks" and empty line
        assert_eq!(list.tasks.len(), 3);
        assert_eq!(list.tasks[0].status, TaskStatus::Unassigned);
        assert_eq!(list.tasks[1].status, TaskStatus::Assigned('A'));
        assert_eq!(list.tasks[2].status, TaskStatus::Completed('B'));
    }

    #[test]
    fn test_tasklist_counts() {
        let content = "- [ ] Task 1\n- [ ] Task 2\n- [A] Task 3\n- [x] Task 4 (B)\n";
        let list = TaskList::parse(content);

        assert_eq!(list.unassigned_count(), 2);
        assert_eq!(list.assigned_count(), 1);
        assert_eq!(list.completed_count(), 1);
    }

    #[test]
    fn test_tasklist_assignable_count() {
        let content = "- [ ] Task 1\n- [ ] Task 2 BLOCKED\n- [A] Task 3\n";
        let list = TaskList::parse(content);

        assert_eq!(list.assignable_count(), 1);
    }

    #[test]
    fn test_tasklist_tasks_for_agent() {
        let content = "- [A] Task 1\n- [B] Task 2\n- [A] Task 3\n";
        let list = TaskList::parse(content);

        let a_tasks = list.tasks_for_agent('A');
        assert_eq!(a_tasks.len(), 2);
        assert_eq!(a_tasks[0].description, "Task 1");
        assert_eq!(a_tasks[1].description, "Task 3");
    }

    #[test]
    fn test_tasklist_assign_sprint() {
        let content = "- [ ] Task 1\n- [ ] Task 2\n- [ ] Task 3\n- [ ] Task 4\n- [ ] Task 5\n";
        let mut list = TaskList::parse(content);

        let assigned = list.assign_sprint(&['A', 'B'], 2);
        assert_eq!(assigned, 4);

        // A gets tasks 1, 2; B gets tasks 3, 4
        assert_eq!(list.tasks[0].status, TaskStatus::Assigned('A'));
        assert_eq!(list.tasks[1].status, TaskStatus::Assigned('A'));
        assert_eq!(list.tasks[2].status, TaskStatus::Assigned('B'));
        assert_eq!(list.tasks[3].status, TaskStatus::Assigned('B'));
        assert_eq!(list.tasks[4].status, TaskStatus::Unassigned);
    }

    #[test]
    fn test_tasklist_assign_sprint_skips_blocked() {
        let content = "- [ ] Task 1 BLOCKED\n- [ ] Task 2\n- [ ] Task 3\n";
        let mut list = TaskList::parse(content);

        let assigned = list.assign_sprint(&['A'], 2);
        assert_eq!(assigned, 2);

        assert_eq!(list.tasks[0].status, TaskStatus::Unassigned); // still blocked
        assert_eq!(list.tasks[1].status, TaskStatus::Assigned('A'));
        assert_eq!(list.tasks[2].status, TaskStatus::Assigned('A'));
    }

    #[test]
    fn test_tasklist_to_string() {
        let content = "# Tasks\n\n- [ ] Task 1\n- [A] Task 2\n";
        let list = TaskList::parse(content);
        let output = list.to_string();

        assert!(output.contains("# Tasks"));
        assert!(output.contains("- [ ] Task 1"));
        assert!(output.contains("- [A] Task 2"));
    }

    #[test]
    fn test_tasklist_roundtrip() {
        let content = "# Tasks\n\n- [ ] Task 1\n- [A] Task 2\n- [x] Task 3 (B)\n";
        let list = TaskList::parse(content);
        let output = list.to_string();

        // Parse again and verify
        let list2 = TaskList::parse(&output);
        assert_eq!(list2.tasks.len(), 3);
        assert_eq!(list2.tasks[0].description, "Task 1");
        assert_eq!(list2.tasks[1].description, "Task 2");
        assert_eq!(list2.tasks[2].description, "Task 3");
    }

    #[test]
    fn test_task_unassign() {
        let mut task = Task::new("Write tests");
        task.assign('A');
        assert_eq!(task.status, TaskStatus::Assigned('A'));

        task.unassign();
        assert_eq!(task.status, TaskStatus::Unassigned);
        assert!(task.is_assignable());
    }

    #[test]
    fn test_task_unassign_completed_no_effect() {
        let mut task = Task::new("Write tests");
        task.assign('A');
        task.complete('A');
        assert_eq!(task.status, TaskStatus::Completed('A'));

        task.unassign(); // Should have no effect on completed tasks
        assert_eq!(task.status, TaskStatus::Completed('A'));
    }

    #[test]
    fn test_tasklist_unassign_all() {
        let content = "- [ ] Task 1\n- [A] Task 2\n- [B] Task 3\n- [x] Task 4 (C)\n";
        let mut list = TaskList::parse(content);

        assert_eq!(list.assigned_count(), 2);

        let unassigned = list.unassign_all();
        assert_eq!(unassigned, 2);
        assert_eq!(list.assigned_count(), 0);
        assert_eq!(list.unassigned_count(), 3); // Task 1, 2, 3 now unassigned
        assert_eq!(list.completed_count(), 1); // Task 4 still completed
    }

    #[test]
    fn test_tasklist_preserves_section_headings() {
        // Test that section headings between tasks are preserved
        let content = "# Tasks\n\n### Section 1\n- [ ] Task 1\n- [ ] Task 2\n\n### Section 2\n- [ ] Task 3\n";
        let list = TaskList::parse(content);

        // Header includes everything before the first task
        assert_eq!(list.header.len(), 3); // "# Tasks", "", "### Section 1"
        assert_eq!(list.header, vec!["# Tasks", "", "### Section 1"]);
        assert_eq!(list.tasks.len(), 3);

        // First task has no prefix (section heading is in header since it's before first task)
        assert!(list.tasks[0].prefix.is_empty());
        assert_eq!(list.tasks[0].description, "Task 1");

        // Second task has no prefix (follows directly after first)
        assert!(list.tasks[1].prefix.is_empty());
        assert_eq!(list.tasks[1].description, "Task 2");

        // Third task should have blank line and section heading as prefix
        assert_eq!(list.tasks[2].prefix, vec!["", "### Section 2"]);
        assert_eq!(list.tasks[2].description, "Task 3");
    }

    #[test]
    fn test_tasklist_section_roundtrip() {
        // Test that parsing and writing back preserves document structure
        let content = "# Phase 0 Tasks\n\n## M0.1 — Setup\n\n### Directory Structure\n- [ ] Task 1\n- [A] Task 2\n\n### Tooling\n- [ ] Task 3\n- [x] Task 4 (B)\n\n## M0.2 — Database\n- [ ] Task 5\n";
        let list = TaskList::parse(content);
        let output = list.to_string();

        // The output should preserve the section structure
        assert!(output.contains("# Phase 0 Tasks"));
        assert!(output.contains("## M0.1 — Setup"));
        assert!(output.contains("### Directory Structure"));
        assert!(output.contains("### Tooling"));
        assert!(output.contains("## M0.2 — Database"));

        // Verify order is correct by checking substring positions
        let pos_setup = output.find("## M0.1 — Setup").unwrap();
        let pos_dir = output.find("### Directory Structure").unwrap();
        let pos_task1 = output.find("Task 1").unwrap();
        let pos_tooling = output.find("### Tooling").unwrap();
        let pos_task3 = output.find("Task 3").unwrap();
        let pos_database = output.find("## M0.2 — Database").unwrap();
        let pos_task5 = output.find("Task 5").unwrap();

        assert!(pos_setup < pos_dir, "Setup should come before Directory Structure");
        assert!(pos_dir < pos_task1, "Directory Structure should come before Task 1");
        assert!(pos_task1 < pos_tooling, "Task 1 should come before Tooling");
        assert!(pos_tooling < pos_task3, "Tooling should come before Task 3");
        assert!(pos_task3 < pos_database, "Task 3 should come before Database");
        assert!(pos_database < pos_task5, "Database should come before Task 5");
    }

    #[test]
    fn test_tasklist_section_roundtrip_exact() {
        // Test exact roundtrip fidelity
        let content = "# Tasks\n\n### Section A\n- [ ] Task 1\n\n### Section B\n- [ ] Task 2\n";
        let list = TaskList::parse(content);
        let output = list.to_string();

        assert_eq!(output, content);
    }

    #[test]
    fn test_tasklist_preserves_blank_lines_between_sections() {
        let content = "# Header\n\n- [ ] Task 1\n\n\n### New Section\n- [ ] Task 2\n";
        let list = TaskList::parse(content);

        assert_eq!(list.tasks.len(), 2);
        // Task 2 should have two blank lines and section heading as prefix
        assert_eq!(list.tasks[1].prefix, vec!["", "", "### New Section"]);

        let output = list.to_string();
        assert_eq!(output, content);
    }

    #[test]
    fn test_tasklist_complex_structure_roundtrip() {
        // Real-world example similar to user's issue
        let content = r#"# Phase 0 Tasks

## M0.1 — Repository Structure

### Directory Setup
- [x] Create /apps/web directory (A)
- [A] Configure ESLint
- [ ] Configure Prettier

### Build Scripts
- [B] Add pnpm build script
- [ ] Add pnpm test script

## M0.2 — Database

### Schema
- [ ] Create jobs table migration
- [ ] Create candidates table migration
"#;
        let list = TaskList::parse(content);
        let output = list.to_string();

        // Verify all sections are preserved in correct order
        let lines: Vec<&str> = output.lines().collect();

        // Find key lines and verify order
        let phase_idx = lines.iter().position(|l| l.contains("# Phase 0")).unwrap();
        let m01_idx = lines.iter().position(|l| l.contains("## M0.1")).unwrap();
        let dir_idx = lines.iter().position(|l| l.contains("### Directory")).unwrap();
        let build_idx = lines.iter().position(|l| l.contains("### Build")).unwrap();
        let m02_idx = lines.iter().position(|l| l.contains("## M0.2")).unwrap();
        let schema_idx = lines.iter().position(|l| l.contains("### Schema")).unwrap();

        assert!(phase_idx < m01_idx);
        assert!(m01_idx < dir_idx);
        assert!(dir_idx < build_idx);
        assert!(build_idx < m02_idx);
        assert!(m02_idx < schema_idx);

        // Verify tasks are under correct sections
        let create_web_idx = lines.iter().position(|l| l.contains("Create /apps/web")).unwrap();
        let eslint_idx = lines.iter().position(|l| l.contains("Configure ESLint")).unwrap();
        let build_script_idx = lines.iter().position(|l| l.contains("pnpm build")).unwrap();
        let jobs_table_idx = lines.iter().position(|l| l.contains("jobs table")).unwrap();

        assert!(dir_idx < create_web_idx && create_web_idx < build_idx,
            "Create /apps/web should be under Directory Setup");
        assert!(dir_idx < eslint_idx && eslint_idx < build_idx,
            "Configure ESLint should be under Directory Setup");
        assert!(build_idx < build_script_idx && build_script_idx < m02_idx,
            "pnpm build should be under Build Scripts");
        assert!(schema_idx < jobs_table_idx,
            "jobs table should be under Schema");
    }
}
