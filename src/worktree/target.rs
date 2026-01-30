use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::git::git_repo_root;

/// Returns the shared worktrees root for target branch operations.
///
/// Path: `./swarm-hub/.shared/worktrees` (relative to the repo root).
pub fn shared_worktrees_root(repo_root: &Path) -> PathBuf {
    repo_root.join("swarm-hub").join(".shared").join("worktrees")
}

/// Ensure the shared worktrees root exists before target worktree operations.
pub fn ensure_shared_worktrees_root(repo_root: &Path) -> Result<PathBuf, String> {
    let root = shared_worktrees_root(repo_root);
    fs::create_dir_all(&root).map_err(|e| {
        format!(
            "failed to create shared worktrees dir {}: {}",
            root.display(),
            e
        )
    })?;
    Ok(root)
}

/// Find the worktree path for the target branch, if any.
pub fn find_target_branch_worktree(target_branch: &str) -> Result<Option<PathBuf>, String> {
    let repo_root = git_repo_root()?;
    find_target_branch_worktree_in(&repo_root, target_branch)
}

/// Find the worktree path for the target branch in the specified repo, if any.
pub fn find_target_branch_worktree_in(
    repo_root: &Path,
    target_branch: &str,
) -> Result<Option<PathBuf>, String> {
    let target = target_branch.trim();
    if target.is_empty() {
        return Err("target branch name is empty".to_string());
    }

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
    Ok(parse_target_worktree_path(&stdout, target, repo_root))
}

/// Parse `git worktree list --porcelain` output to find the worktree path
/// for a specific target branch.
fn parse_target_worktree_path(
    porcelain_output: &str,
    target_branch: &str,
    repo_root: &Path,
) -> Option<PathBuf> {
    let mut current_path: Option<&str> = None;

    for line in porcelain_output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.trim());
        } else if let Some(branch_ref) = line.strip_prefix("branch refs/heads/") {
            if branch_ref.trim() == target_branch {
                if let Some(path) = current_path {
                    let candidate = PathBuf::from(path);
                    return Some(if candidate.is_absolute() {
                        candidate
                    } else {
                        repo_root.join(candidate)
                    });
                }
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_shared_worktrees_root_path() {
        let temp = TempDir::new().expect("temp dir");
        let root = shared_worktrees_root(temp.path());
        let expected = temp
            .path()
            .join("swarm-hub")
            .join(".shared")
            .join("worktrees");
        assert_eq!(root, expected);
    }

    #[test]
    fn test_ensure_shared_worktrees_root_creates_dir() {
        let temp = TempDir::new().expect("temp dir");
        let root = ensure_shared_worktrees_root(temp.path()).expect("create shared root");
        assert!(root.exists(), "shared worktrees root should exist");
        assert!(root.is_dir(), "shared worktrees root should be a directory");
    }

    #[test]
    fn test_parse_target_worktree_path_finds_match() {
        let porcelain = "\\
worktree /repo
HEAD abc123
branch refs/heads/main

worktree /repo/swarm-hub/.shared/worktrees/develop
HEAD def456
branch refs/heads/develop

";
        let repo_root = Path::new("/repo");
        let result =
            parse_target_worktree_path(porcelain, "develop", repo_root).expect("path found");
        assert_eq!(
            result,
            PathBuf::from("/repo/swarm-hub/.shared/worktrees/develop")
        );
    }

    #[test]
    fn test_parse_target_worktree_path_resolves_relative() {
        let porcelain = "\\
worktree swarm-hub/.shared/worktrees/main
HEAD abc123
branch refs/heads/main

";
        let repo_root = Path::new("/repo");
        let result =
            parse_target_worktree_path(porcelain, "main", repo_root).expect("path found");
        assert_eq!(
            result,
            PathBuf::from("/repo/swarm-hub/.shared/worktrees/main")
        );
    }

    #[test]
    fn test_parse_target_worktree_path_no_match() {
        let porcelain = "\\
worktree /repo
HEAD abc123
branch refs/heads/main

";
        let repo_root = Path::new("/repo");
        let result = parse_target_worktree_path(porcelain, "develop", repo_root);
        assert!(result.is_none());
    }
}
