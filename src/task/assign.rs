use super::{Task, TaskList, TaskStatus};

impl Task {
    /// Extract the task number from a leading "(#N)" prefix.
    pub fn task_number(&self) -> Option<usize> {
        let desc = self.description.trim_start();
        let after_prefix = desc.strip_prefix("(#")?;

        let mut digits_len = 0;
        for ch in after_prefix.chars() {
            if ch.is_ascii_digit() {
                digits_len += ch.len_utf8();
            } else {
                break;
            }
        }

        if digits_len == 0 {
            return None;
        }

        let after_digits = after_prefix.get(digits_len..)?;
        if !after_digits.starts_with(')') {
            return None;
        }

        after_prefix[..digits_len].parse::<usize>().ok()
    }

    /// Check if this task has blocking references.
    ///
    /// Returns true if the task has `(blocked by #N)` in its description.
    /// Use `TaskList::is_task_blocked()` to check if blockers are actually incomplete.
    pub fn has_blockers(&self) -> bool {
        !self.blocking_task_numbers().is_empty()
    }

    /// Extract blocking task numbers from the description.
    ///
    /// Parses patterns like `(blocked by #1)` or `(blocked by #1, #2, #3)`.
    /// Returns a vector of task numbers that this task depends on.
    pub fn blocking_task_numbers(&self) -> Vec<usize> {
        let desc = &self.description;

        // Look for "(blocked by #N)" or "(blocked by #N, #M, ...)" pattern
        if let Some(start) = desc.find("(blocked by ") {
            let after_prefix = &desc[start + 12..]; // skip "(blocked by "
            if let Some(end) = after_prefix.find(')') {
                let refs = &after_prefix[..end];
                // Parse comma-separated #N references
                return refs
                    .split(',')
                    .filter_map(|part| {
                        part.trim()
                            .strip_prefix('#')
                            .and_then(|num| num.parse::<usize>().ok())
                    })
                    .collect();
            }
        }

        Vec::new()
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

    /// Check if this task is assignable based on status alone.
    ///
    /// Note: For full blocking checks, use `TaskList::is_task_assignable()` which
    /// checks both status and whether blocking dependencies are complete.
    pub fn is_assignable(&self) -> bool {
        matches!(self.status, TaskStatus::Unassigned)
    }
}

impl TaskList {
    /// Get the highest task number in the list, based on "(#N)" prefixes.
    pub fn max_task_number(&self) -> usize {
        self.tasks
            .iter()
            .filter_map(Task::task_number)
            .max()
            .unwrap_or(0)
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

    /// Get count of assignable tasks (unassigned and not blocked).
    pub fn assignable_count(&self) -> usize {
        (0..self.tasks.len())
            .filter(|&i| self.is_task_assignable(i))
            .count()
    }

    /// Check if a task at the given index is blocked.
    ///
    /// A task is blocked if it has `(blocked by #N)` references where any
    /// referenced task is not yet completed.
    pub fn is_task_blocked(&self, task_index: usize) -> bool {
        let task = match self.tasks.get(task_index) {
            Some(t) => t,
            None => return false,
        };

        // Get blocking task numbers from "(blocked by #N)" references
        let blocking_numbers = task.blocking_task_numbers();
        if blocking_numbers.is_empty() {
            return false;
        }

        // Check if any blocking task is NOT completed
        for blocking_num in blocking_numbers {
            // Task numbers in "(blocked by #N)" are 1-indexed from the PRD format
            // We need to find the task with that number in its description
            let blocker_completed = self.is_task_number_completed(blocking_num);
            if !blocker_completed {
                return true; // Still blocked by an incomplete task
            }
        }

        false // All blockers are completed
    }

    /// Check if a task at the given index is assignable.
    ///
    /// A task is assignable if it's unassigned and not blocked.
    pub fn is_task_assignable(&self, task_index: usize) -> bool {
        let task = match self.tasks.get(task_index) {
            Some(t) => t,
            None => return false,
        };

        matches!(task.status, TaskStatus::Unassigned) && !self.is_task_blocked(task_index)
    }

    /// Get tasks assigned to a specific agent.
    pub fn tasks_for_agent(&self, initial: char) -> Vec<&Task> {
        let upper = initial.to_ascii_uppercase();
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Assigned(i) if i == upper))
            .collect()
    }

    /// Assign tasks to agents for a sprint.
    ///
    /// Returns the number of tasks assigned.
    pub fn assign_sprint(&mut self, agent_initials: &[char], tasks_per_agent: usize) -> usize {
        let mut assigned = 0;
        let mut agent_task_count: std::collections::HashMap<char, usize> =
            std::collections::HashMap::new();

        for task_idx in 0..self.tasks.len() {
            if !self.is_task_assignable(task_idx) {
                continue;
            }

            // Find an agent with capacity
            for &initial in agent_initials {
                let count = agent_task_count.entry(initial).or_insert(0);
                if *count < tasks_per_agent {
                    self.tasks[task_idx].assign(initial);
                    *count += 1;
                    assigned += 1;
                    break;
                }
            }
        }

        assigned
    }

    /// Check if a task with the given number (from #N format) is completed.
    ///
    /// Looks for tasks with `(#N)` in their description.
    fn is_task_number_completed(&self, task_num: usize) -> bool {
        let pattern = format!("(#{})", task_num);
        for task in &self.tasks {
            if task.description.contains(&pattern) {
                return matches!(task.status, TaskStatus::Completed(_));
            }
        }
        // If we can't find the task, assume it's not completed (conservative)
        false
    }
}
