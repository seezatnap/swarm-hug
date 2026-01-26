//! Git worktree management.
//!
//! Manages git worktrees and branches for agents. Each agent gets:
//! - A worktree directory: `worktrees/agent-<INITIAL>-<name>`
//! - A dedicated branch: `agent/<lowercase_name>`
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
    cleanup_agent_worktree, cleanup_agent_worktrees, cleanup_worktrees, cleanup_worktrees_in,
    delete_branch, CleanupSummary,
};
pub use create::{create_worktrees, create_worktrees_in};
pub use git::{
    agent_branch_exists, agent_branch_has_changes, agent_branch_name, delete_agent_branch,
    merge_agent_branch, merge_all_agent_branches, MergeResult, MergeSummary,
};
pub use list::{list_agent_branches, list_worktrees, AgentBranch};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_branch_name() {
        assert_eq!(agent_branch_name('A'), Some("agent/aaron".to_string()));
        assert_eq!(agent_branch_name('B'), Some("agent/betty".to_string()));
        assert_eq!(agent_branch_name('Z'), Some("agent/zane".to_string()));
        assert_eq!(agent_branch_name('a'), Some("agent/aaron".to_string()));
        assert_eq!(agent_branch_name('1'), None);
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
            branch: "agent/aaron".to_string(),
            exists: true,
        };
        assert_eq!(branch.initial, 'A');
        assert_eq!(branch.name, "aaron");
        assert_eq!(branch.branch, "agent/aaron");

        // Non-standard branch (like scrummaster) uses '?' as initial
        let sm_branch = AgentBranch {
            initial: '?',
            name: "scrummaster".to_string(),
            branch: "agent/scrummaster".to_string(),
            exists: true,
        };
        assert_eq!(sm_branch.initial, '?');
        assert_eq!(sm_branch.name, "scrummaster");
    }
}
