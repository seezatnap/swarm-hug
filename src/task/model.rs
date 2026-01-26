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
    /// Get count of unassigned tasks.
    pub fn unassigned_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Unassigned))
            .count()
    }

    /// Get count of assigned tasks.
    pub fn assigned_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Assigned(_)))
            .count()
    }

    /// Get count of completed tasks.
    pub fn completed_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Completed(_)))
            .count()
    }
}
