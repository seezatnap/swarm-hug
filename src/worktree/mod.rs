//! Git worktree management.
//!
//! Manages git worktrees and branches for agents. Each agent gets:
//! - A worktree directory: `worktrees/{project}-agent-{name}-{hash}`
//! - A dedicated branch: `{project}-agent-{name}-{hash}`
//!
//! Branch and worktree names are namespaced by project and run hash to allow
//! parallel sprints across different projects and safe restarts.
//!
//! In multi-team mode, worktrees are created under `.swarm-hug/<team>/worktrees/`.

mod cleanup;
mod create;
mod git;
mod list;

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: std::path::PathBuf,
    pub initial: char,
    pub name: String,
}

pub use cleanup::{
    cleanup_agent_worktree, cleanup_agent_worktrees, cleanup_feature_worktree, cleanup_worktrees,
    cleanup_worktrees_in, delete_branch, CleanupSummary,
};
pub use create::{create_feature_worktree_in, create_worktrees_in};
pub use git::{
    agent_branch_exists, agent_branch_has_changes, agent_branch_name, branch_is_merged,
    create_feature_branch, create_feature_branch_in, delete_agent_branch, merge_agent_branch,
    merge_agent_branch_in, merge_all_agent_branches, merge_feature_branch, MergeResult,
    MergeSummary,
};
pub use list::{list_agent_branches, list_worktrees, AgentBranch};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_context::RunContext;

    #[test]
    fn test_agent_branch_name_with_context() {
        let ctx = RunContext::new("greenfield", 1);
        let branch = agent_branch_name(&ctx, 'A');
        assert!(branch.starts_with("greenfield-agent-aaron-"));
        assert_eq!(branch.len(), "greenfield-agent-aaron-".len() + 6);
    }

    #[test]
    fn test_agent_branch_name_different_initials() {
        let ctx = RunContext::new("greenfield", 1);
        let branch_a = agent_branch_name(&ctx, 'A');
        let branch_b = agent_branch_name(&ctx, 'B');
        let branch_z = agent_branch_name(&ctx, 'Z');

        assert!(branch_a.starts_with("greenfield-agent-aaron-"));
        assert!(branch_b.starts_with("greenfield-agent-betty-"));
        assert!(branch_z.starts_with("greenfield-agent-zane-"));

        // All should share the same hash
        let hash = ctx.hash();
        assert!(branch_a.ends_with(hash));
        assert!(branch_b.ends_with(hash));
        assert!(branch_z.ends_with(hash));
    }

    #[test]
    fn test_agent_branch_name_lowercase_initial() {
        let ctx = RunContext::new("greenfield", 1);
        let branch = agent_branch_name(&ctx, 'a');
        assert!(branch.starts_with("greenfield-agent-aaron-"));
    }

    #[test]
    fn test_agent_branch_name_invalid_initial() {
        let ctx = RunContext::new("greenfield", 1);
        let branch = agent_branch_name(&ctx, '1');
        assert!(branch.starts_with("greenfield-agent-unknown-"));
    }

    #[test]
    fn test_merge_summary_default() {
        let summary = MergeSummary::default();
        assert_eq!(summary.success_count(), 0);
        assert_eq!(summary.conflict_count(), 0);
        assert!(!summary.has_conflicts());
    }

    #[test]
    fn test_cleanup_summary_default() {
        let summary = CleanupSummary::default();
        assert_eq!(summary.cleaned_count(), 0);
        assert!(!summary.has_errors());
    }

    #[test]
    fn test_agent_branch_struct() {
        // Valid agent branch
        let branch = AgentBranch {
            initial: 'A',
            name: "aaron".to_string(),
            branch: "agent-aaron".to_string(),
            exists: true,
        };
        assert_eq!(branch.initial, 'A');
        assert_eq!(branch.name, "aaron");
        assert_eq!(branch.branch, "agent-aaron");

        // Non-standard branch (like scrummaster) uses '?' as initial
        let sm_branch = AgentBranch {
            initial: '?',
            name: "scrummaster".to_string(),
            branch: "agent-scrummaster".to_string(),
            exists: true,
        };
        assert_eq!(sm_branch.initial, '?');
        assert_eq!(sm_branch.name, "scrummaster");
    }
}
