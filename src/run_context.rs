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
    /// Target branch for this run variation.
    pub target_branch: String,
    /// Runtime identifier shared by all sprints in a single `swarm run`.
    /// Derived from project + target branch + run instance.
    pub runtime_id: String,
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
        let run_instance = generate_run_hash();
        Self::new_for_run(project, "default-target", &run_instance, sprint_number)
    }

    /// Creates a run context for a specific run variation.
    ///
    /// The `run_instance` should be created once per `swarm run` invocation and
    /// reused for every sprint in that invocation. This keeps runtime state keys
    /// stable within a run while still allowing per-sprint artifact hashes.
    pub fn new_for_run(
        project: &str,
        target_branch: &str,
        run_instance: &str,
        sprint_number: u32,
    ) -> Self {
        Self {
            project: project.to_string(),
            target_branch: target_branch.to_string(),
            runtime_id: compose_runtime_id(project, target_branch, run_instance),
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

    /// Returns the stable runtime identifier for this run variation.
    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    /// Prefix a state key with this context's runtime identifier.
    ///
    /// This ensures concurrent variations don't share runtime state keys.
    pub fn runtime_state_key(&self, key: &str) -> String {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            self.runtime_id.clone()
        } else {
            format!("{}::{}", self.runtime_id, trimmed)
        }
    }
}

fn compose_runtime_id(project: &str, target_branch: &str, run_instance: &str) -> String {
    let project = sanitize_runtime_component(project, "project");
    let target_branch = sanitize_runtime_component(target_branch, "target");
    let run_instance = sanitize_runtime_component(run_instance, "run");
    format!("{}::{}::{}", project, target_branch, run_instance)
}

fn sanitize_runtime_component(component: &str, fallback: &str) -> String {
    let mut sanitized = String::with_capacity(component.len());
    for byte in component.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            sanitized.push(ch);
        } else {
            sanitized.push('%');
            sanitized.push(hex_char(byte >> 4));
            sanitized.push(hex_char(byte & 0x0f));
        }
    }
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn hex_char(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + (nibble - 10)) as char,
        _ => '0',
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
    fn test_new_for_run_sets_target_branch() {
        let ctx = RunContext::new_for_run("greenfield", "feature/x", "run42", 1);
        assert_eq!(ctx.target_branch, "feature/x");
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
        let sprint_hash = sprint.split('-').next_back().unwrap();
        let agent_hash = agent.split('-').next_back().unwrap();
        assert_eq!(sprint_hash, agent_hash);
    }

    #[test]
    fn test_hash_returns_run_hash() {
        let ctx = RunContext::new("greenfield", 1);
        assert_eq!(ctx.hash(), &ctx.run_hash);
    }

    #[test]
    fn test_runtime_id_contains_project_target_and_run_instance() {
        let ctx = RunContext::new_for_run("greenfield", "feature/x", "abc123", 1);
        assert_eq!(ctx.runtime_id(), "greenfield::feature%2Fx::abc123");
    }

    #[test]
    fn test_runtime_id_differs_for_different_target_branches() {
        let ctx_main = RunContext::new_for_run("greenfield", "main", "run42", 1);
        let ctx_feature = RunContext::new_for_run("greenfield", "feature/a", "run42", 1);
        assert_ne!(ctx_main.runtime_id(), ctx_feature.runtime_id());
    }

    #[test]
    fn test_runtime_id_stays_same_for_same_run_instance_across_sprints() {
        let sprint1 = RunContext::new_for_run("greenfield", "feature/a", "run42", 1);
        let sprint2 = RunContext::new_for_run("greenfield", "feature/a", "run42", 2);
        assert_eq!(sprint1.runtime_id(), sprint2.runtime_id());
        assert_ne!(sprint1.hash(), sprint2.hash());
    }

    #[test]
    fn test_runtime_state_key_prefixes_runtime_id() {
        let ctx = RunContext::new_for_run("greenfield", "feature/a", "run42", 1);
        assert_eq!(
            ctx.runtime_state_key("sprint-history"),
            "greenfield::feature%2Fa::run42::sprint-history"
        );
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
