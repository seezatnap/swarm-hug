//! Git worktree management.
//!
//! Manages git worktrees and branches for agents. Each agent gets:
//! - A worktree directory: `worktrees/agent-<INITIAL>-<name>`
//! - A dedicated branch: `agent/<lowercase_name>`
//!
//! In multi-team mode, worktrees are created under `.swarm-hug/<team>/worktrees/`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub initial: char,
    pub name: String,
}

fn git_repo_root() -> Result<PathBuf, String> {
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

fn ensure_head(repo_root: &Path) -> Result<(), String> {
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

fn worktrees_dir_abs(worktrees_dir: &Path, repo_root: &Path) -> PathBuf {
    if worktrees_dir.is_absolute() {
        worktrees_dir.to_path_buf()
    } else {
        repo_root.join(worktrees_dir)
    }
}

fn registered_worktrees(repo_root: &Path) -> Result<HashSet<String>, String> {
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

fn is_registered_path(registered: &HashSet<String>, path: &Path) -> bool {
    let display = path.to_string_lossy().to_string();
    if registered.contains(&display) {
        return true;
    }
    if let Ok(canonical) = path.canonicalize() {
        return registered.contains(&canonical.to_string_lossy().to_string());
    }
    false
}

fn worktree_is_registered(repo_root: &Path, path: &Path) -> Result<bool, String> {
    let registered = registered_worktrees(repo_root)?;
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    Ok(is_registered_path(&registered, &abs))
}

fn worktree_path(root: &Path, initial: char, name: &str) -> PathBuf {
    root.join(format!("agent-{}-{}", initial, name))
}

/// Create worktrees in the specified directory.
/// The `worktrees_dir` should be the full path to the worktrees directory
/// (e.g., ".swarm-hug/authentication/worktrees" in multi-team mode).
pub fn create_worktrees_in(
    worktrees_dir: &Path,
    assignments: &[(char, String)],
) -> Result<Vec<Worktree>, String> {
    let mut created = Vec::new();
    let mut seen = HashSet::new();

    if assignments.is_empty() {
        return Ok(created);
    }

    let repo_root = git_repo_root()?;
    ensure_head(&repo_root)?;
    let worktrees_dir = worktrees_dir_abs(worktrees_dir, &repo_root);

    fs::create_dir_all(&worktrees_dir)
        .map_err(|e| format!("failed to create worktrees dir: {}", e))?;

    let mut registered = registered_worktrees(&repo_root)?;

    for (initial, _task) in assignments {
        let upper = initial.to_ascii_uppercase();
        if !seen.insert(upper) {
            continue;
        }
        let name = crate::agent::name_from_initial(upper).unwrap_or("Unknown");
        let path = worktree_path(&worktrees_dir, upper, name);
        let path_str = path.to_string_lossy().to_string();
        if is_registered_path(&registered, &path) {
            created.push(Worktree {
                path,
                initial: upper,
                name: name.to_string(),
            });
            continue;
        }

        if path.exists() {
            return Err(format!(
                "worktree path exists but is not registered: {}",
                path.display()
            ));
        }

        let branch = agent_branch_name(upper)
            .ok_or_else(|| format!("invalid agent initial: {}", upper))?;

        let output = Command::new("git")
            .arg("-C")
            .arg(&repo_root)
            .args(["worktree", "add", "-B", &branch, &path_str])
            .output()
            .map_err(|e| format!("failed to run git worktree add: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "git worktree add failed for {}: {}",
                path.display(),
                stderr.trim()
            ));
        }

        registered.insert(path_str);
        created.push(Worktree {
            path,
            initial: upper,
            name: name.to_string(),
        });
    }

    Ok(created)
}

/// Legacy function for backwards compatibility.
/// Creates worktrees under `base/worktrees/`.
pub fn create_worktrees(
    base: &Path,
    assignments: &[(char, String)],
) -> Result<Vec<Worktree>, String> {
    create_worktrees_in(&base.join("worktrees"), assignments)
}

/// Clean up worktrees in the specified directory.
pub fn cleanup_worktrees_in(worktrees_dir: &Path) -> Result<(), String> {
    if !worktrees_dir.exists() {
        return Ok(());
    }

    let repo_root = match git_repo_root() {
        Ok(root) => root,
        Err(_) => {
            fs::remove_dir_all(worktrees_dir)
                .map_err(|e| format!("failed to remove worktrees: {}", e))?;
            return Ok(());
        }
    };

    let worktrees_dir = worktrees_dir_abs(worktrees_dir, &repo_root);
    if !worktrees_dir.exists() {
        return Ok(());
    }

    let worktrees = list_worktrees(&worktrees_dir)?;
    let mut errors = Vec::new();

    for wt in worktrees {
        let path = if wt.path.is_absolute() {
            wt.path
        } else {
            repo_root.join(wt.path)
        };
        match worktree_is_registered(&repo_root, &path) {
            Ok(true) => {
                let path_str = path.to_string_lossy().to_string();
                let output = Command::new("git")
                    .arg("-C")
                    .arg(&repo_root)
                    .args(["worktree", "remove", "--force", &path_str])
                    .output();
                match output {
                    Ok(o) if o.status.success() => {}
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        errors.push(format!(
                            "git worktree remove failed for {}: {}",
                            path.display(),
                            stderr.trim()
                        ));
                    }
                    Err(e) => errors.push(format!(
                        "failed to run git worktree remove for {}: {}",
                        path.display(),
                        e
                    )),
                }
            }
            Ok(false) => {
                if path.exists() {
                    if let Err(e) = fs::remove_dir_all(&path) {
                        errors.push(format!(
                            "failed to remove worktree {}: {}",
                            path.display(),
                            e
                        ));
                    }
                }
            }
            Err(e) => errors.push(e),
        }
    }

    if worktrees_dir.exists() {
        if let Err(e) = fs::remove_dir_all(&worktrees_dir) {
            errors.push(format!("failed to remove worktrees dir: {}", e));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Legacy function for backwards compatibility.
/// Cleans up worktrees under `base/worktrees/`.
pub fn cleanup_worktrees(base: &Path) -> Result<(), String> {
    cleanup_worktrees_in(&base.join("worktrees"))
}

/// List worktrees in the specified directory.
pub fn list_worktrees(worktrees_dir: &Path) -> Result<Vec<Worktree>, String> {
    let mut worktrees = Vec::new();

    if !worktrees_dir.exists() {
        return Ok(worktrees);
    }

    let entries = fs::read_dir(worktrees_dir)
        .map_err(|e| format!("failed to read worktrees dir: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Parse directory name: agent-<initial>-<name>
        let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if let Some(rest) = dir_name.strip_prefix("agent-") {
            let parts: Vec<&str> = rest.splitn(2, '-').collect();
            if parts.len() == 2 {
                if let Some(initial) = parts[0].chars().next() {
                    worktrees.push(Worktree {
                        path,
                        initial: initial.to_ascii_uppercase(),
                        name: parts[1].to_string(),
                    });
                }
            }
        }
    }

    worktrees.sort_by(|a, b| a.initial.cmp(&b.initial));
    Ok(worktrees)
}

/// Agent branch info.
#[derive(Debug, Clone)]
pub struct AgentBranch {
    pub initial: char,
    pub name: String,
    pub branch: String,
    pub exists: bool,
}

/// Get the branch name for an agent.
/// Format: agent/<lowercase_name> (e.g., agent/aaron)
pub fn agent_branch_name(initial: char) -> Option<String> {
    let name = crate::agent::name_from_initial(initial)?;
    Some(format!("agent/{}", name.to_lowercase()))
}

/// List agent branches in the repository.
/// Returns branches matching the pattern `agent/<name>`.
pub fn list_agent_branches() -> Result<Vec<AgentBranch>, String> {
    let output = Command::new("git")
        .args(["branch", "--list", "agent/*"])
        .output()
        .map_err(|e| format!("failed to run git branch: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git branch failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut branches = Vec::new();

    for line in stdout.lines() {
        let branch = line.trim().trim_start_matches("* ");
        if let Some(agent_name) = branch.strip_prefix("agent/") {
            // Find the initial for this agent name
            if let Some(initial) = crate::agent::initial_from_name(agent_name) {
                branches.push(AgentBranch {
                    initial,
                    name: agent_name.to_string(),
                    branch: branch.to_string(),
                    exists: true,
                });
            }
        }
    }

    branches.sort_by(|a, b| a.initial.cmp(&b.initial));
    Ok(branches)
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
        let checkout = Command::new("git")
            .args(["checkout", target])
            .output();

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
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect()
        }
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
}
