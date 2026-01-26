use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn git_repo_root() -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("failed to run git rev-parse: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git rev-parse failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let root = stdout.trim();
    if root.is_empty() {
        return Err("git rev-parse returned empty repo root".to_string());
    }
    Ok(PathBuf::from(root))
}

pub(super) fn ensure_head(repo_root: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .map_err(|e| format!("failed to run git rev-parse HEAD: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err("git repo has no commits; create an initial commit before creating worktrees"
            .to_string())
    }
}

pub(super) fn registered_worktrees(repo_root: &Path) -> Result<HashSet<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(|e| format!("failed to run git worktree list: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree list failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut registered = HashSet::new();
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            registered.insert(path.trim().to_string());
        }
    }
    Ok(registered)
}

/// Parse git worktree list --porcelain output to find worktrees with a specific branch.
/// This is separated out for testability.
fn parse_worktrees_with_branch(porcelain_output: &str, branch: &str) -> Vec<String> {
    let mut worktrees_with_branch = Vec::new();
    let mut current_path: Option<String> = None;

    // Parse porcelain output format:
    // worktree /path/to/worktree
    // HEAD <sha>
    // branch refs/heads/<branch>
    // <blank line>
    for line in porcelain_output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.trim().to_string());
        } else if let Some(branch_ref) = line.strip_prefix("branch refs/heads/") {
            if branch_ref.trim() == branch {
                if let Some(ref path) = current_path {
                    worktrees_with_branch.push(path.clone());
                }
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    worktrees_with_branch
}

/// Find all worktree paths that have a specific branch checked out.
/// Returns a list of absolute paths to worktrees using that branch.
pub(super) fn find_worktrees_with_branch(
    repo_root: &Path,
    branch: &str,
) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(|e| format!("failed to run git worktree list: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree list failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_worktrees_with_branch(&stdout, branch))
}

/// Get the branch name for an agent.
/// Format: agent/<lowercase_name> (e.g., agent/aaron)
pub fn agent_branch_name(initial: char) -> Option<String> {
    let name = crate::agent::name_from_initial(initial)?;
    Some(format!("agent/{}", name.to_lowercase()))
}

/// Check if an agent branch exists.
pub fn agent_branch_exists(initial: char) -> bool {
    let branch = match agent_branch_name(initial) {
        Some(b) => b,
        None => return false,
    };

    let output = Command::new("git")
        .args(["rev-parse", "--verify", &branch])
        .output();

    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Merge result.
#[derive(Debug, Clone)]
pub enum MergeResult {
    Success,
    Conflict(Vec<String>),
    NoBranch,
    NoChanges,
    Error(String),
}

/// Check if an agent branch has changes relative to a target branch.
pub fn agent_branch_has_changes(initial: char, target: &str) -> Result<bool, String> {
    let branch = agent_branch_name(initial)
        .ok_or_else(|| format!("invalid agent initial: {}", initial))?;

    let output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", target, branch)])
        .output()
        .map_err(|e| format!("failed to run git rev-list: {}", e))?;

    if !output.status.success() {
        // Branch might not exist
        return Ok(false);
    }

    let count_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let count: i32 = count_str.parse().unwrap_or(0);
    Ok(count > 0)
}

/// Merge an agent branch into the current branch.
/// Returns MergeResult indicating success, conflict, or error.
pub fn merge_agent_branch(initial: char, target_branch: Option<&str>) -> MergeResult {
    let branch = match agent_branch_name(initial) {
        Some(b) => b,
        None => return MergeResult::Error(format!("invalid agent initial: {}", initial)),
    };

    // Check if branch exists
    if !agent_branch_exists(initial) {
        return MergeResult::NoBranch;
    }

    // If target branch specified, checkout first
    if let Some(target) = target_branch {
        let checkout = Command::new("git").args(["checkout", target]).output();

        if let Err(e) = checkout {
            return MergeResult::Error(format!("checkout failed: {}", e));
        }
        let checkout = checkout.unwrap();
        if !checkout.status.success() {
            let stderr = String::from_utf8_lossy(&checkout.stderr);
            return MergeResult::Error(format!("checkout failed: {}", stderr));
        }

        // Check if branch has changes
        match agent_branch_has_changes(initial, target) {
            Ok(false) => return MergeResult::NoChanges,
            Err(e) => return MergeResult::Error(e),
            Ok(true) => {}
        }
    }

    // Get agent name for commit message
    let agent_name = crate::agent::name_from_initial(initial).unwrap_or("Unknown");

    // Attempt merge with --no-ff
    let merge = Command::new("git")
        .args(["merge", "--no-ff", "-m", &format!("Merge {}", branch), &branch])
        .env("GIT_AUTHOR_NAME", format!("Agent {}", agent_name))
        .env("GIT_AUTHOR_EMAIL", format!("agent-{}@swarm.local", initial))
        .env("GIT_COMMITTER_NAME", format!("Agent {}", agent_name))
        .env("GIT_COMMITTER_EMAIL", format!("agent-{}@swarm.local", initial))
        .output();

    match merge {
        Err(e) => MergeResult::Error(format!("merge command failed: {}", e)),
        Ok(output) if output.status.success() => MergeResult::Success,
        Ok(_) => {
            // Check for conflicts
            let conflicts = get_merge_conflicts();
            if !conflicts.is_empty() {
                // Abort the merge
                let _ = Command::new("git").args(["merge", "--abort"]).output();
                MergeResult::Conflict(conflicts)
            } else {
                MergeResult::Error("merge failed".to_string())
            }
        }
    }
}

/// Get list of files with merge conflicts.
fn get_merge_conflicts() -> Vec<String> {
    let output = Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=U"])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// Merge summary for multiple agents.
#[derive(Debug, Default)]
pub struct MergeSummary {
    pub success: Vec<char>,
    pub conflicts: Vec<(char, Vec<String>)>,
    pub no_changes: Vec<char>,
    pub errors: Vec<(char, String)>,
}

impl MergeSummary {
    pub fn success_count(&self) -> usize {
        self.success.len()
    }

    pub fn conflict_count(&self) -> usize {
        self.conflicts.len()
    }

    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }
}

/// Merge all agent branches into the target branch.
/// Returns a summary of merge results.
pub fn merge_all_agent_branches(initials: &[char], target_branch: &str) -> MergeSummary {
    let mut summary = MergeSummary::default();

    for &initial in initials {
        match merge_agent_branch(initial, Some(target_branch)) {
            MergeResult::Success => summary.success.push(initial),
            MergeResult::Conflict(files) => summary.conflicts.push((initial, files)),
            MergeResult::NoChanges => summary.no_changes.push(initial),
            MergeResult::NoBranch => {} // Skip non-existent branches
            MergeResult::Error(e) => summary.errors.push((initial, e)),
        }
    }

    summary
}

/// Delete an agent's branch.
/// Returns Ok(true) if deleted, Ok(false) if branch didn't exist.
pub fn delete_agent_branch(initial: char) -> Result<bool, String> {
    let branch = agent_branch_name(initial)
        .ok_or_else(|| format!("invalid agent initial: {}", initial))?;

    if !agent_branch_exists(initial) {
        return Ok(false);
    }

    let output = Command::new("git")
        .args(["branch", "-D", &branch])
        .output()
        .map_err(|e| format!("failed to run git branch -D: {}", e))?;

    if output.status.success() {
        Ok(true)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git branch -D failed: {}", stderr.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_worktrees_with_branch;

    #[test]
    fn test_parse_worktrees_with_branch_finds_match() {
        // Simulated git worktree list --porcelain output
        let porcelain = "\\
worktree /repo
HEAD abc123
branch refs/heads/main

worktree /repo/.swarm-hug/greenfield/worktrees/agent-D-Diana
HEAD def456
branch refs/heads/agent/diana

worktree /repo/.swarm-hug/phase-one/worktrees/agent-A-Aaron
HEAD ghi789
branch refs/heads/agent/aaron

";
        let result = parse_worktrees_with_branch(porcelain, "agent/diana");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "/repo/.swarm-hug/greenfield/worktrees/agent-D-Diana");
    }

    #[test]
    fn test_parse_worktrees_with_branch_no_match() {
        let porcelain = "\\
worktree /repo
HEAD abc123
branch refs/heads/main

worktree /repo/.swarm-hug/team/worktrees/agent-A-Aaron
HEAD def456
branch refs/heads/agent/aaron

";
        let result = parse_worktrees_with_branch(porcelain, "agent/diana");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_worktrees_with_branch_multiple_matches() {
        // Scenario: same branch checked out in multiple worktrees (shouldn't happen but test anyway)
        let porcelain = "\\
worktree /repo/.swarm-hug/team1/worktrees/agent-D-Diana
HEAD abc123
branch refs/heads/agent/diana

worktree /repo/.swarm-hug/team2/worktrees/agent-D-Diana
HEAD abc123
branch refs/heads/agent/diana

";
        let result = parse_worktrees_with_branch(porcelain, "agent/diana");
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"/repo/.swarm-hug/team1/worktrees/agent-D-Diana".to_string()));
        assert!(result.contains(&"/repo/.swarm-hug/team2/worktrees/agent-D-Diana".to_string()));
    }

    #[test]
    fn test_parse_worktrees_with_branch_detached_head() {
        // Worktrees can be in detached HEAD state (no branch line)
        let porcelain = "\\
worktree /repo
HEAD abc123
branch refs/heads/main

worktree /repo/.swarm-hug/team/worktrees/agent-A-Aaron
HEAD def456
detached

";
        // Should not crash and should return empty for agent/aaron
        let result = parse_worktrees_with_branch(porcelain, "agent/aaron");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_worktrees_with_branch_empty_output() {
        let result = parse_worktrees_with_branch("", "agent/diana");
        assert!(result.is_empty());
    }
}
