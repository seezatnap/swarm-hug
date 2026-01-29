use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::run_context::RunContext;

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

/// Get the branch name for an agent with project namespace and run hash.
/// Format: {project}-agent-{name}-{hash} (e.g., greenfield-agent-aaron-a3f8k2)
///
/// This is the preferred function for creating agent branch names when you have
/// a `RunContext`. For legacy code paths that don't yet have context, use
/// `agent_branch_name_legacy()`.
pub fn agent_branch_name(ctx: &RunContext, initial: char) -> String {
    ctx.agent_branch(initial)
}

/// Legacy agent branch name without project namespace.
/// Format: agent-<lowercase_name> (e.g., agent-aaron)
///
/// This function is deprecated and will be removed once all callers are
/// updated to use `agent_branch_name()` with `RunContext`.
/// Used by internal functions that haven't been migrated to RunContext yet.
pub(super) fn agent_branch_name_legacy(initial: char) -> Option<String> {
    let name = crate::agent::name_from_initial(initial)?;
    Some(format!("agent-{}", name.to_lowercase()))
}

fn agent_branch_exists_in(repo_root: &Path, initial: char) -> bool {
    let branch = match agent_branch_name_legacy(initial) {
        Some(b) => b,
        None => return false,
    };

    branch_exists(repo_root, &branch).unwrap_or(false)
}

/// Check if an agent branch exists.
pub fn agent_branch_exists(initial: char) -> bool {
    let repo_root = match git_repo_root() {
        Ok(root) => root,
        Err(_) => return false,
    };
    agent_branch_exists_in(&repo_root, initial)
}

/// Create a feature/sprint branch from the target branch.
/// Returns Ok(true) if created, Ok(false) if it already exists.
pub fn create_feature_branch(feature_branch: &str, target_branch: &str) -> Result<bool, String> {
    let repo_root = git_repo_root()?;
    create_feature_branch_in(&repo_root, feature_branch, target_branch)
}

/// Create a feature/sprint branch from the target branch in the specified repo.
/// Returns Ok(true) if created, Ok(false) if it already exists.
pub fn create_feature_branch_in(
    repo_root: &Path,
    feature_branch: &str,
    target_branch: &str,
) -> Result<bool, String> {
    let feature = feature_branch.trim();
    if feature.is_empty() {
        return Err("feature branch name is empty".to_string());
    }
    let target = target_branch.trim();
    if target.is_empty() {
        return Err("target branch name is empty".to_string());
    }

    ensure_head(repo_root)?;

    if !branch_exists(repo_root, target)? {
        return Err(format!("target branch '{}' not found", target));
    }
    if branch_exists(repo_root, feature)? {
        return Ok(false);
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["branch", feature, target])
        .output()
        .map_err(|e| format!("failed to run git branch: {}", e))?;

    if output.status.success() {
        Ok(true)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git branch failed: {}", stderr.trim()))
    }
}

fn branch_exists(repo_root: &Path, branch: &str) -> Result<bool, String> {
    let ref_name = format!("refs/heads/{}", branch);
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["show-ref", "--verify", "--quiet", &ref_name])
        .output()
        .map_err(|e| format!("failed to run git show-ref: {}", e))?;

    if output.status.success() {
        return Ok(true);
    }
    match output.status.code() {
        Some(1) => Ok(false),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("git show-ref failed: {}", stderr.trim()))
        }
    }
}

fn branch_has_changes_in(
    repo_root: &Path,
    source_branch: &str,
    target_branch: &str,
) -> Result<bool, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args([
            "rev-list",
            "--count",
            &format!("{}..{}", target_branch, source_branch),
        ])
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
    let repo_root = git_repo_root()?;
    agent_branch_has_changes_in(&repo_root, initial, target)
}

fn agent_branch_has_changes_in(
    repo_root: &Path,
    initial: char,
    target: &str,
) -> Result<bool, String> {
    let branch = agent_branch_name_legacy(initial)
        .ok_or_else(|| format!("invalid agent initial: {}", initial))?;
    branch_has_changes_in(repo_root, &branch, target)
}

fn agent_branch_has_changes_with_ctx(
    repo_root: &Path,
    ctx: &RunContext,
    initial: char,
    target: &str,
) -> Result<bool, String> {
    let branch = agent_branch_name(ctx, initial);
    branch_has_changes_in(repo_root, &branch, target)
}

/// Merge an agent branch into the current branch.
/// Returns MergeResult indicating success, conflict, or error.
pub fn merge_agent_branch(initial: char, target_branch: Option<&str>) -> MergeResult {
    let repo_root = match git_repo_root() {
        Ok(root) => root,
        Err(e) => return MergeResult::Error(e),
    };
    merge_agent_branch_in(&repo_root, initial, target_branch)
}

/// Merge an agent branch into the target branch within the specified repo/worktree.
/// Returns MergeResult indicating success, conflict, or error.
pub fn merge_agent_branch_in(
    repo_root: &Path,
    initial: char,
    target_branch: Option<&str>,
) -> MergeResult {
    let branch = match agent_branch_name_legacy(initial) {
        Some(b) => b,
        None => return MergeResult::Error(format!("invalid agent initial: {}", initial)),
    };

    // Check if branch exists
    match branch_exists(repo_root, &branch) {
        Ok(true) => {}
        Ok(false) => return MergeResult::NoBranch,
        Err(e) => return MergeResult::Error(e),
    }

    // If target branch specified, checkout first
    if let Some(target) = target_branch {
        let checkout = Command::new("git")
            .arg("-C")
            .arg(repo_root)
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
        match agent_branch_has_changes_in(repo_root, initial, target) {
            Ok(false) => return MergeResult::NoChanges,
            Err(e) => return MergeResult::Error(e),
            Ok(true) => {}
        }
    }

    // Get agent name for commit message
    let agent_name = crate::agent::name_from_initial(initial).unwrap_or("Unknown");

    // Attempt merge with --no-ff
    let merge = Command::new("git")
        .arg("-C")
        .arg(repo_root)
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
            let conflicts = get_merge_conflicts_in(repo_root);
            if !conflicts.is_empty() {
                // Abort the merge
                let _ = Command::new("git")
                    .arg("-C")
                    .arg(repo_root)
                    .args(["merge", "--abort"])
                    .output();
                MergeResult::Conflict(conflicts)
            } else {
                MergeResult::Error("merge failed".to_string())
            }
        }
    }
}

/// Merge an agent branch into the target branch using RunContext for namespaced branch names.
/// Returns MergeResult indicating success, conflict, or error.
pub fn merge_agent_branch_in_with_ctx(
    repo_root: &Path,
    ctx: &RunContext,
    initial: char,
    target_branch: Option<&str>,
) -> MergeResult {
    let branch = agent_branch_name(ctx, initial);

    // Check if branch exists
    match branch_exists(repo_root, &branch) {
        Ok(true) => {}
        Ok(false) => return MergeResult::NoBranch,
        Err(e) => return MergeResult::Error(e),
    }

    // If target branch specified, checkout first
    if let Some(target) = target_branch {
        let checkout = Command::new("git")
            .arg("-C")
            .arg(repo_root)
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
        match agent_branch_has_changes_with_ctx(repo_root, ctx, initial, target) {
            Ok(false) => return MergeResult::NoChanges,
            Err(e) => return MergeResult::Error(e),
            Ok(true) => {}
        }
    }

    // Get agent name for commit message
    let agent_name = crate::agent::name_from_initial(initial).unwrap_or("Unknown");

    // Attempt merge with --no-ff
    let merge = Command::new("git")
        .arg("-C")
        .arg(repo_root)
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
            let conflicts = get_merge_conflicts_in(repo_root);
            if !conflicts.is_empty() {
                // Abort the merge
                let _ = Command::new("git")
                    .arg("-C")
                    .arg(repo_root)
                    .args(["merge", "--abort"])
                    .output();
                MergeResult::Conflict(conflicts)
            } else {
                MergeResult::Error("merge failed".to_string())
            }
        }
    }
}

/// Check whether a source branch has been merged into a target branch.
pub fn branch_is_merged(source_branch: &str, target_branch: &str) -> Result<bool, String> {
    let repo_root = git_repo_root()?;
    branch_is_merged_in(&repo_root, source_branch, target_branch)
}

fn branch_is_merged_in(
    repo_root: &Path,
    source_branch: &str,
    target_branch: &str,
) -> Result<bool, String> {
    let source = source_branch.trim();
    if source.is_empty() {
        return Err("source branch name is empty".to_string());
    }
    let target = target_branch.trim();
    if target.is_empty() {
        return Err("target branch name is empty".to_string());
    }

    ensure_head(repo_root)?;

    if !branch_exists(repo_root, source)? {
        return Err(format!("source branch '{}' not found", source));
    }
    if !branch_exists(repo_root, target)? {
        return Err(format!("target branch '{}' not found", target));
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["merge-base", "--is-ancestor", source, target])
        .output()
        .map_err(|e| format!("failed to run git merge-base: {}", e))?;

    if output.status.success() {
        Ok(true)
    } else if output.status.code() == Some(1) {
        Ok(false)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git merge-base failed: {}", stderr.trim()))
    }
}

/// Merge a feature branch into a target branch in the main repo.
pub fn merge_feature_branch(feature_branch: &str, target_branch: &str) -> MergeResult {
    let repo_root = match git_repo_root() {
        Ok(root) => root,
        Err(e) => return MergeResult::Error(e),
    };
    merge_feature_branch_in(&repo_root, feature_branch, target_branch)
}

fn merge_feature_branch_in(
    repo_root: &Path,
    feature_branch: &str,
    target_branch: &str,
) -> MergeResult {
    let feature = feature_branch.trim();
    if feature.is_empty() {
        return MergeResult::Error("feature branch name is empty".to_string());
    }
    let target = target_branch.trim();
    if target.is_empty() {
        return MergeResult::Error("target branch name is empty".to_string());
    }

    if let Err(e) = ensure_head(repo_root) {
        return MergeResult::Error(e);
    }

    match branch_exists(repo_root, feature) {
        Ok(true) => {}
        Ok(false) => return MergeResult::NoBranch,
        Err(e) => return MergeResult::Error(e),
    }
    match branch_exists(repo_root, target) {
        Ok(true) => {}
        Ok(false) => {
            return MergeResult::Error(format!("target branch '{}' not found", target));
        }
        Err(e) => return MergeResult::Error(e),
    }

    let checkout = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["checkout", target])
        .output();

    if let Err(e) = checkout {
        return MergeResult::Error(format!("checkout failed: {}", e));
    }
    let checkout = checkout.unwrap();
    if !checkout.status.success() {
        let stderr = String::from_utf8_lossy(&checkout.stderr);
        return MergeResult::Error(format!("checkout failed: {}", stderr.trim()));
    }

    match branch_has_changes_in(repo_root, feature, target) {
        Ok(false) => return MergeResult::NoChanges,
        Ok(true) => {}
        Err(e) => return MergeResult::Error(e),
    }

    if let Err(e) = cleanup_untracked_swarm_hug_files(repo_root) {
        return MergeResult::Error(e);
    }

    let merge = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args([
            "merge",
            "--autostash",
            "--no-ff",
            "-m",
            &format!("Merge branch '{}' into {}", feature, target),
            feature,
        ])
        .env("GIT_AUTHOR_NAME", "Swarm ScrumMaster")
        .env("GIT_AUTHOR_EMAIL", "scrummaster@swarm.local")
        .env("GIT_COMMITTER_NAME", "Swarm ScrumMaster")
        .env("GIT_COMMITTER_EMAIL", "scrummaster@swarm.local")
        .output();

    match merge {
        Err(e) => MergeResult::Error(format!("merge command failed: {}", e)),
        Ok(output) if output.status.success() => MergeResult::Success,
        Ok(output) => {
            let conflicts = get_merge_conflicts_in(repo_root);
            if !conflicts.is_empty() {
                let _ = Command::new("git")
                    .arg("-C")
                    .arg(repo_root)
                    .args(["merge", "--abort"])
                    .output();
                MergeResult::Conflict(conflicts)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let detail = stderr.trim();
                if detail.is_empty() {
                    MergeResult::Error("merge failed".to_string())
                } else {
                    MergeResult::Error(format!("merge failed: {}", detail))
                }
            }
        }
    }
}

fn cleanup_untracked_swarm_hug_files(repo_root: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["ls-files", "--others", "--exclude-standard", "--", ".swarm-hug"])
        .output()
        .map_err(|e| format!("failed to list untracked files: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git ls-files failed while scanning untracked files: {}",
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let rel = line.trim();
        if rel.is_empty() {
            continue;
        }
        let path = repo_root.join(rel);
        if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| format!("failed to remove {}: {}", path.display(), e))?;
        } else if path.is_file() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("failed to remove {}: {}", path.display(), e))?;
        }
    }

    Ok(())
}

/// Get list of files with merge conflicts in the specified repo/worktree.
fn get_merge_conflicts_in(repo_root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
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
    let branch = agent_branch_name_legacy(initial)
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
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    use super::{
        branch_is_merged_in,
        create_feature_branch_in,
        merge_agent_branch_in,
        merge_feature_branch_in,
        parse_worktrees_with_branch,
        MergeResult,
    };

    fn run_git(repo: &Path, args: &[&str]) -> Output {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("failed to run git command");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn init_repo(repo: &Path) {
        run_git(repo, &["init"]);
        run_git(repo, &["config", "user.name", "Swarm Test"]);
        run_git(repo, &["config", "user.email", "swarm-test@example.com"]);
        fs::write(repo.join("README.md"), "init").expect("write README");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "init"]);
    }

    fn commit_file(repo: &Path, filename: &str, message: &str) {
        fs::write(repo.join(filename), "change").expect("write file");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", message]);
    }

    fn rev_parse(repo: &Path, rev: &str) -> String {
        let output = run_git(repo, &["rev-parse", rev]);
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn branch_exists(repo: &Path, branch: &str) -> bool {
        let ref_name = format!("refs/heads/{}", branch);
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["show-ref", "--verify", "--quiet", &ref_name])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn test_parse_worktrees_with_branch_finds_match() {
        // Simulated git worktree list --porcelain output
        let porcelain = "\\
worktree /repo
HEAD abc123
branch refs/heads/main

worktree /repo/.swarm-hug/greenfield/worktrees/agent-D-Diana
HEAD def456
branch refs/heads/agent-diana

worktree /repo/.swarm-hug/phase-one/worktrees/agent-A-Aaron
HEAD ghi789
branch refs/heads/agent-aaron

";
        let result = parse_worktrees_with_branch(porcelain, "agent-diana");
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
branch refs/heads/agent-aaron

";
        let result = parse_worktrees_with_branch(porcelain, "agent-diana");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_worktrees_with_branch_multiple_matches() {
        // Scenario: same branch checked out in multiple worktrees (shouldn't happen but test anyway)
        let porcelain = "\\
worktree /repo/.swarm-hug/team1/worktrees/agent-D-Diana
HEAD abc123
branch refs/heads/agent-diana

worktree /repo/.swarm-hug/team2/worktrees/agent-D-Diana
HEAD abc123
branch refs/heads/agent-diana

";
        let result = parse_worktrees_with_branch(porcelain, "agent-diana");
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
        // Should not crash and should return empty for agent-aaron
        let result = parse_worktrees_with_branch(porcelain, "agent-aaron");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_worktrees_with_branch_empty_output() {
        let result = parse_worktrees_with_branch("", "agent-diana");
        assert!(result.is_empty());
    }

    #[test]
    fn test_create_feature_branch_from_target() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);

        // Create target branch at initial commit
        run_git(repo, &["branch", "target-branch"]);

        // Advance current branch so HEAD differs from target
        commit_file(repo, "extra.txt", "extra commit");

        let head_rev = rev_parse(repo, "HEAD");
        let target_rev = rev_parse(repo, "target-branch");
        assert_ne!(head_rev, target_rev);

        let created =
            create_feature_branch_in(repo, "greenfield-sprint-1", "target-branch").unwrap();
        assert!(created);
        assert!(branch_exists(repo, "greenfield-sprint-1"));

        let feature_rev = rev_parse(repo, "greenfield-sprint-1");
        assert_eq!(feature_rev, target_rev);
        assert_ne!(feature_rev, head_rev);
    }

    #[test]
    fn test_create_feature_branch_noop_when_exists() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);

        let created = create_feature_branch_in(repo, "greenfield-sprint-1", "target-branch")
            .expect("create feature branch");
        assert!(created);

        let created_again = create_feature_branch_in(repo, "greenfield-sprint-1", "target-branch")
            .expect("second create should not error");
        assert!(!created_again);
    }

    #[test]
    fn test_create_feature_branch_missing_target() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);

        let err =
            create_feature_branch_in(repo, "greenfield-sprint-1", "missing-branch").unwrap_err();
        assert!(
            err.contains("target branch 'missing-branch' not found"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_merge_agent_branch_in_creates_no_ff_merge_commit() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);

        run_git(repo, &["checkout", "-b", "alpha-sprint-1"]);
        run_git(repo, &["checkout", "-b", "agent-aaron"]);
        commit_file(repo, "task.txt", "task commit");

        let result = merge_agent_branch_in(repo, 'A', Some("alpha-sprint-1"));
        assert!(matches!(result, MergeResult::Success));

        let output = run_git(repo, &["rev-list", "--parents", "-n", "1", "HEAD"]);
        let output_str = String::from_utf8_lossy(&output.stdout);
        let parents: Vec<&str> = output_str.split_whitespace().collect();
        assert_eq!(parents.len(), 3, "expected merge commit with two parents");

        let content = fs::read_to_string(repo.join("task.txt")).expect("read file");
        assert_eq!(content, "change");
    }

    #[test]
    fn test_branch_is_merged_in_tracks_feature_merge() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);

        run_git(repo, &["branch", "-M", "main"]);
        run_git(repo, &["checkout", "-b", "feature-branch"]);
        commit_file(repo, "feature.txt", "feature commit");
        run_git(repo, &["checkout", "main"]);

        let merged_before = branch_is_merged_in(repo, "feature-branch", "main")
            .expect("merge check before");
        assert!(!merged_before);

        let merge_result = merge_feature_branch_in(repo, "feature-branch", "main");
        assert!(matches!(merge_result, MergeResult::Success));

        let merged_after = branch_is_merged_in(repo, "feature-branch", "main")
            .expect("merge check after");
        assert!(merged_after);

        let output = run_git(repo, &["rev-list", "--parents", "-n", "1", "HEAD"]);
        let output_str = String::from_utf8_lossy(&output.stdout);
        let parents: Vec<&str> = output_str.split_whitespace().collect();
        assert_eq!(parents.len(), 3, "expected merge commit with two parents");
    }
}
