//! Agent lifecycle tracking.
//!
//! Each agent goes through these states:
//! - Assigned: Task has been assigned but agent hasn't started
//! - Working: Agent is actively executing the task
//! - Done: Agent completed the task (success or failure)
//! - Terminated: Agent has been cleaned up

use std::collections::HashMap;
use std::fmt;

/// Agent lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    /// Task assigned, agent not yet started.
    Assigned,
    /// Agent is working on the task.
    Working,
    /// Agent completed (success or failure).
    Done,
    /// Agent terminated and cleaned up.
    Terminated,
}

impl fmt::Display for AgentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentState::Assigned => write!(f, "assigned"),
            AgentState::Working => write!(f, "working"),
            AgentState::Done => write!(f, "done"),
            AgentState::Terminated => write!(f, "terminated"),
        }
    }
}

/// Agent execution context.
#[derive(Debug, Clone)]
pub struct AgentContext {
    /// Agent initial (A-Z).
    pub initial: char,
    /// Agent name (Aaron, Betty, etc.).
    pub name: String,
    /// Current state.
    pub state: AgentState,
    /// Assigned task description.
    pub task: String,
    /// Worktree path for this agent.
    pub worktree_path: String,
    /// Whether the task completed successfully.
    pub success: Option<bool>,
    /// Error message if failed.
    pub error: Option<String>,
}

impl AgentContext {
    /// Create a new agent context in Assigned state.
    pub fn new(initial: char, name: &str, task: &str, worktree_path: &str) -> Self {
        Self {
            initial,
            name: name.to_string(),
            state: AgentState::Assigned,
            task: task.to_string(),
            worktree_path: worktree_path.to_string(),
            success: None,
            error: None,
        }
    }

    /// Transition to Working state.
    pub fn start(&mut self) {
        if self.state == AgentState::Assigned {
            self.state = AgentState::Working;
        }
    }

    /// Transition to Done state with success.
    pub fn complete(&mut self) {
        if self.state == AgentState::Working {
            self.state = AgentState::Done;
            self.success = Some(true);
        }
    }

    /// Transition to Done state with failure.
    pub fn fail(&mut self, error: &str) {
        if self.state == AgentState::Working {
            self.state = AgentState::Done;
            self.success = Some(false);
            self.error = Some(error.to_string());
        }
    }

    /// Transition to Terminated state.
    pub fn terminate(&mut self) {
        if self.state == AgentState::Done {
            self.state = AgentState::Terminated;
        }
    }

    /// Check if agent is in a terminal state.
    pub fn is_finished(&self) -> bool {
        matches!(self.state, AgentState::Done | AgentState::Terminated)
    }

    /// Check if agent completed successfully.
    pub fn succeeded(&self) -> bool {
        self.success == Some(true)
    }
}

/// Tracks lifecycle state for all agents in a sprint.
#[derive(Debug, Default)]
pub struct LifecycleTracker {
    /// Agent contexts by initial.
    agents: HashMap<char, AgentContext>,
}

impl LifecycleTracker {
    /// Create a new tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an agent with a task.
    pub fn register(&mut self, initial: char, name: &str, task: &str, worktree_path: &str) {
        self.agents.insert(
            initial,
            AgentContext::new(initial, name, task, worktree_path),
        );
    }

    /// Get agent context.
    pub fn get(&self, initial: char) -> Option<&AgentContext> {
        self.agents.get(&initial)
    }

    /// Get mutable agent context.
    pub fn get_mut(&mut self, initial: char) -> Option<&mut AgentContext> {
        self.agents.get_mut(&initial)
    }

    /// Start an agent's work.
    pub fn start(&mut self, initial: char) {
        if let Some(ctx) = self.agents.get_mut(&initial) {
            ctx.start();
        }
    }

    /// Mark an agent as completed.
    pub fn complete(&mut self, initial: char) {
        if let Some(ctx) = self.agents.get_mut(&initial) {
            ctx.complete();
        }
    }

    /// Mark an agent as failed.
    pub fn fail(&mut self, initial: char, error: &str) {
        if let Some(ctx) = self.agents.get_mut(&initial) {
            ctx.fail(error);
        }
    }

    /// Terminate an agent.
    pub fn terminate(&mut self, initial: char) {
        if let Some(ctx) = self.agents.get_mut(&initial) {
            ctx.terminate();
        }
    }

    /// Terminate all done agents.
    pub fn terminate_all_done(&mut self) {
        for ctx in self.agents.values_mut() {
            if ctx.state == AgentState::Done {
                ctx.terminate();
            }
        }
    }

    /// Get all agents.
    pub fn all(&self) -> impl Iterator<Item = &AgentContext> {
        self.agents.values()
    }

    /// Get agents in a specific state.
    pub fn in_state(&self, state: AgentState) -> Vec<&AgentContext> {
        self.agents
            .values()
            .filter(|ctx| ctx.state == state)
            .collect()
    }

    /// Count agents in each state.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let mut assigned = 0;
        let mut working = 0;
        let mut done = 0;
        let mut terminated = 0;

        for ctx in self.agents.values() {
            match ctx.state {
                AgentState::Assigned => assigned += 1,
                AgentState::Working => working += 1,
                AgentState::Done => done += 1,
                AgentState::Terminated => terminated += 1,
            }
        }

        (assigned, working, done, terminated)
    }

    /// Check if all agents are done or terminated.
    pub fn all_finished(&self) -> bool {
        self.agents.values().all(|ctx| ctx.is_finished())
    }

    /// Get success count.
    pub fn success_count(&self) -> usize {
        self.agents.values().filter(|ctx| ctx.succeeded()).count()
    }

    /// Get failure count.
    pub fn failure_count(&self) -> usize {
        self.agents
            .values()
            .filter(|ctx| ctx.success == Some(false))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_state_display() {
        assert_eq!(format!("{}", AgentState::Assigned), "assigned");
        assert_eq!(format!("{}", AgentState::Working), "working");
        assert_eq!(format!("{}", AgentState::Done), "done");
        assert_eq!(format!("{}", AgentState::Terminated), "terminated");
    }

    #[test]
    fn test_agent_context_new() {
        let ctx = AgentContext::new('A', "Aaron", "Write tests", "/tmp/wt");
        assert_eq!(ctx.initial, 'A');
        assert_eq!(ctx.name, "Aaron");
        assert_eq!(ctx.task, "Write tests");
        assert_eq!(ctx.state, AgentState::Assigned);
        assert!(ctx.success.is_none());
    }

    #[test]
    fn test_agent_context_lifecycle() {
        let mut ctx = AgentContext::new('A', "Aaron", "Write tests", "/tmp/wt");

        // Assigned -> Working
        assert_eq!(ctx.state, AgentState::Assigned);
        ctx.start();
        assert_eq!(ctx.state, AgentState::Working);

        // Working -> Done (success)
        ctx.complete();
        assert_eq!(ctx.state, AgentState::Done);
        assert!(ctx.succeeded());

        // Done -> Terminated
        ctx.terminate();
        assert_eq!(ctx.state, AgentState::Terminated);
    }

    #[test]
    fn test_agent_context_failure() {
        let mut ctx = AgentContext::new('B', "Betty", "Fix bug", "/tmp/wt");
        ctx.start();
        ctx.fail("compilation error");

        assert_eq!(ctx.state, AgentState::Done);
        assert!(!ctx.succeeded());
        assert_eq!(ctx.error, Some("compilation error".to_string()));
    }

    #[test]
    fn test_lifecycle_tracker() {
        let mut tracker = LifecycleTracker::new();

        tracker.register('A', "Aaron", "Task A", "/wt/a");
        tracker.register('B', "Betty", "Task B", "/wt/b");

        assert_eq!(tracker.counts(), (2, 0, 0, 0));

        tracker.start('A');
        assert_eq!(tracker.counts(), (1, 1, 0, 0));

        tracker.complete('A');
        assert_eq!(tracker.counts(), (1, 0, 1, 0));

        tracker.start('B');
        tracker.fail('B', "error");
        assert_eq!(tracker.counts(), (0, 0, 2, 0));

        assert_eq!(tracker.success_count(), 1);
        assert_eq!(tracker.failure_count(), 1);
        assert!(tracker.all_finished());

        tracker.terminate_all_done();
        assert_eq!(tracker.counts(), (0, 0, 0, 2));
    }

    #[test]
    fn test_tracker_in_state() {
        let mut tracker = LifecycleTracker::new();
        tracker.register('A', "Aaron", "Task A", "/wt/a");
        tracker.register('B', "Betty", "Task B", "/wt/b");

        tracker.start('A');

        let working = tracker.in_state(AgentState::Working);
        assert_eq!(working.len(), 1);
        assert_eq!(working[0].initial, 'A');

        let assigned = tracker.in_state(AgentState::Assigned);
        assert_eq!(assigned.len(), 1);
        assert_eq!(assigned[0].initial, 'B');
    }
}
