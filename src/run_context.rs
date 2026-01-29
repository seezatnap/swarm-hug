//! Run context for sprint run isolation.
//!
//! Holds the project name, sprint number, and run hash for a single sprint run.
//! All artifacts (worktrees, branches) for the run share the same hash,
//! ensuring isolation between runs and projects.

use crate::agent;
use crate::run_hash::generate_run_hash;

/// Context for a single sprint run.
///
/// Created once at run start and passed to all functions that create
/// namespaced artifacts (worktrees, branches). All artifacts from a
/// single run share the same hash, ensuring they can be cleanly
/// identified and cleaned up together.
///
/// # Examples
/// ```
/// use swarm::run_context::RunContext;
///
/// let ctx = RunContext::new("greenfield", 1);
/// assert!(ctx.sprint_branch().starts_with("greenfield-sprint-1-"));
/// assert!(ctx.agent_branch('A').starts_with("greenfield-agent-aaron-"));
/// assert_eq!(ctx.hash().len(), 6);
/// ```
#[derive(Debug, Clone)]
pub struct RunContext {
    /// Project name (team name).
    pub project: String,
    /// Sprint number within the project.
    pub sprint_number: u32,
    /// Unique hash for this run (6 alphanumeric characters).
    pub run_hash: String,
}

impl RunContext {
    /// Creates a new run context with a freshly generated hash.
    ///
    /// # Arguments
    /// * `project` - The project/team name
    /// * `sprint_number` - The sprint number within the project
    ///
    /// # Examples
    /// ```
    /// use swarm::run_context::RunContext;
    ///
    /// let ctx = RunContext::new("payments", 2);
    /// assert_eq!(ctx.project, "payments");
    /// assert_eq!(ctx.sprint_number, 2);
    /// assert_eq!(ctx.run_hash.len(), 6);
    /// ```
    pub fn new(project: &str, sprint_number: u32) -> Self {
        Self {
            project: project.to_string(),
            sprint_number,
            run_hash: generate_run_hash(),
        }
    }

    /// Returns the sprint branch name: `{project}-sprint-{n}-{hash}`.
    ///
    /// # Examples
    /// ```
    /// use swarm::run_context::RunContext;
    ///
    /// let ctx = RunContext::new("greenfield", 1);
    /// let branch = ctx.sprint_branch();
    /// assert!(branch.starts_with("greenfield-sprint-1-"));
    /// assert_eq!(branch.len(), "greenfield-sprint-1-".len() + 6);
    /// ```
    pub fn sprint_branch(&self) -> String {
        format!(
            "{}-sprint-{}-{}",
            self.project, self.sprint_number, self.run_hash
        )
    }

    /// Returns the agent branch name: `{project}-agent-{name}-{hash}`.
    ///
    /// # Arguments
    /// * `initial` - The agent's initial (A-Z)
    ///
    /// # Examples
    /// ```
    /// use swarm::run_context::RunContext;
    ///
    /// let ctx = RunContext::new("greenfield", 1);
    /// let branch = ctx.agent_branch('A');
    /// assert!(branch.starts_with("greenfield-agent-aaron-"));
    /// ```
    pub fn agent_branch(&self, initial: char) -> String {
        let name = agent::name_from_initial(initial).unwrap_or("unknown");
        format!(
            "{}-agent-{}-{}",
            self.project,
            name.to_lowercase(),
            self.run_hash
        )
    }

    /// Returns the run hash for display/logging.
    ///
    /// # Examples
    /// ```
    /// use swarm::run_context::RunContext;
    ///
    /// let ctx = RunContext::new("greenfield", 1);
    /// assert_eq!(ctx.hash().len(), 6);
    /// ```
    pub fn hash(&self) -> &str {
        &self.run_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_sets_project() {
        let ctx = RunContext::new("greenfield", 1);
        assert_eq!(ctx.project, "greenfield");
    }

    #[test]
    fn test_new_sets_sprint_number() {
        let ctx = RunContext::new("greenfield", 5);
        assert_eq!(ctx.sprint_number, 5);
    }

    #[test]
    fn test_new_generates_hash() {
        let ctx = RunContext::new("greenfield", 1);
        assert_eq!(ctx.run_hash.len(), 6);
        assert!(ctx
            .run_hash
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn test_sprint_branch_format() {
        let ctx = RunContext::new("greenfield", 1);
        let branch = ctx.sprint_branch();
        assert!(branch.starts_with("greenfield-sprint-1-"));
        assert_eq!(branch.len(), "greenfield-sprint-1-".len() + 6);
    }

    #[test]
    fn test_sprint_branch_includes_hash() {
        let ctx = RunContext::new("greenfield", 1);
        let branch = ctx.sprint_branch();
        assert!(branch.ends_with(&ctx.run_hash));
    }

    #[test]
    fn test_agent_branch_format_initial_a() {
        let ctx = RunContext::new("greenfield", 1);
        let branch = ctx.agent_branch('A');
        assert!(branch.starts_with("greenfield-agent-aaron-"));
    }

    #[test]
    fn test_agent_branch_format_initial_b() {
        let ctx = RunContext::new("greenfield", 1);
        let branch = ctx.agent_branch('B');
        assert!(branch.starts_with("greenfield-agent-betty-"));
    }

    #[test]
    fn test_agent_branch_format_initial_z() {
        let ctx = RunContext::new("greenfield", 1);
        let branch = ctx.agent_branch('Z');
        assert!(branch.starts_with("greenfield-agent-zane-"));
    }

    #[test]
    fn test_agent_branch_lowercase_initial() {
        let ctx = RunContext::new("greenfield", 1);
        let branch = ctx.agent_branch('a');
        assert!(branch.starts_with("greenfield-agent-aaron-"));
    }

    #[test]
    fn test_agent_branch_invalid_initial() {
        let ctx = RunContext::new("greenfield", 1);
        let branch = ctx.agent_branch('1');
        assert!(branch.starts_with("greenfield-agent-unknown-"));
    }

    #[test]
    fn test_agent_branch_includes_same_hash_as_sprint() {
        let ctx = RunContext::new("greenfield", 1);
        let sprint = ctx.sprint_branch();
        let agent = ctx.agent_branch('A');

        // Extract hash from both (last 6 characters)
        let sprint_hash = sprint.split('-').last().unwrap();
        let agent_hash = agent.split('-').last().unwrap();
        assert_eq!(sprint_hash, agent_hash);
    }

    #[test]
    fn test_hash_returns_run_hash() {
        let ctx = RunContext::new("greenfield", 1);
        assert_eq!(ctx.hash(), &ctx.run_hash);
    }

    #[test]
    fn test_hash_length() {
        let ctx = RunContext::new("greenfield", 1);
        assert_eq!(ctx.hash().len(), 6);
    }

    #[test]
    fn test_different_runs_different_hashes() {
        let ctx1 = RunContext::new("greenfield", 1);
        let ctx2 = RunContext::new("greenfield", 1);
        assert_ne!(ctx1.run_hash, ctx2.run_hash);
        assert_ne!(ctx1.sprint_branch(), ctx2.sprint_branch());
    }

    #[test]
    fn test_different_projects_different_branches() {
        let ctx1 = RunContext::new("greenfield", 1);
        let ctx2 = RunContext::new("payments", 1);
        // Even with different hashes, prefixes differ
        assert!(!ctx1.sprint_branch().starts_with("payments-"));
        assert!(!ctx2.sprint_branch().starts_with("greenfield-"));
    }

    #[test]
    fn test_all_agents_share_same_hash() {
        let ctx = RunContext::new("greenfield", 1);
        let expected_hash = ctx.hash();
        for initial in 'A'..='Z' {
            let branch = ctx.agent_branch(initial);
            assert!(
                branch.ends_with(expected_hash),
                "Agent {} branch doesn't end with expected hash: {}",
                initial,
                branch
            );
        }
    }

    #[test]
    fn test_clone() {
        let ctx = RunContext::new("greenfield", 1);
        let cloned = ctx.clone();
        assert_eq!(ctx.project, cloned.project);
        assert_eq!(ctx.sprint_number, cloned.sprint_number);
        assert_eq!(ctx.run_hash, cloned.run_hash);
    }

    #[test]
    fn test_debug() {
        let ctx = RunContext::new("greenfield", 1);
        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("RunContext"));
        assert!(debug_str.contains("greenfield"));
    }

    #[test]
    fn test_sprint_number_zero() {
        let ctx = RunContext::new("greenfield", 0);
        let branch = ctx.sprint_branch();
        assert!(branch.starts_with("greenfield-sprint-0-"));
    }

    #[test]
    fn test_sprint_number_large() {
        let ctx = RunContext::new("greenfield", 999);
        let branch = ctx.sprint_branch();
        assert!(branch.starts_with("greenfield-sprint-999-"));
    }

    #[test]
    fn test_project_with_hyphens() {
        let ctx = RunContext::new("my-cool-project", 1);
        let branch = ctx.sprint_branch();
        assert!(branch.starts_with("my-cool-project-sprint-1-"));
    }

    #[test]
    fn test_empty_project_name() {
        let ctx = RunContext::new("", 1);
        let branch = ctx.sprint_branch();
        assert!(branch.starts_with("-sprint-1-"));
    }
}
