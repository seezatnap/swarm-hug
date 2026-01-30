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
    let target = normalize_target_branch(target_branch)?;
    find_target_branch_worktree_in_normalized(repo_root, target)
}

fn normalize_target_branch(target_branch: &str) -> Result<&str, String> {
    let target = target_branch.trim();
    if target.is_empty() {
        return Err("target branch name is empty".to_string());
    }
    let target = target.strip_prefix("refs/heads/").unwrap_or(target);
    if target.is_empty() {
        return Err("target branch name is empty".to_string());
    }
    Ok(target)
}

/// Validate that the target branch worktree (if any) is under the shared root.
///
/// Returns the worktree path when it exists under the shared root, Ok(None) if
/// no worktree exists, and Err if the worktree exists outside the shared root.
pub fn validate_target_branch_worktree(target_branch: &str) -> Result<Option<PathBuf>, String> {
    let repo_root = git_repo_root()?;
    validate_target_branch_worktree_in(&repo_root, target_branch)
}

/// Validate that the target branch worktree (if any) is under the shared root.
pub fn validate_target_branch_worktree_in(
    repo_root: &Path,
    target_branch: &str,
) -> Result<Option<PathBuf>, String> {
    let target = normalize_target_branch(target_branch)?;
    let shared_root = ensure_shared_worktrees_root(repo_root)?;
    let existing = find_target_branch_worktree_in_normalized(repo_root, target)?;

    if let Some(path) = existing {
        if path_is_under_root(&path, &shared_root) {
            return Ok(Some(path));
        }
        return Err(format!(
            "target branch '{}' already has a worktree at '{}' outside shared worktrees root '{}'",
            target,
            path.display(),
            shared_root.display()
        ));
    }

    Ok(None)
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

fn find_target_branch_worktree_in_normalized(
    repo_root: &Path,
    target_branch: &str,
) -> Result<Option<PathBuf>, String> {
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
    Ok(parse_target_worktree_path(&stdout, target_branch, repo_root))
}

fn path_is_under_root(path: &Path, root: &Path) -> bool {
    if path.starts_with(root) {
        return true;
    }

    let path_canonical = path.canonicalize().ok();
    let root_canonical = root.canonicalize().ok();

    match (path_canonical, root_canonical) {
        (Some(path), Some(root)) => path.starts_with(&root),
        (Some(path), None) => path.starts_with(root),
        (None, Some(root)) => path.starts_with(&root),
        (None, None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
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

    #[test]
    fn test_normalize_target_branch_strips_refs_heads() {
        let normalized = normalize_target_branch("refs/heads/main").expect("normalize branch");
        assert_eq!(normalized, "main");
    }

    #[test]
    fn test_normalize_target_branch_rejects_empty_ref() {
        let err = normalize_target_branch("refs/heads/").expect_err("should error");
        assert_eq!(err, "target branch name is empty");
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("failed to run git command");
        assert!(
            output.status.success(),
            "git -C {} {:?} failed\nstdout:\n{}\nstderr:\n{}",
            repo.display(),
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(repo: &Path) {
        run_git(repo, &["init"]);
        run_git(repo, &["config", "user.name", "Swarm Test"]);
        run_git(repo, &["config", "user.email", "swarm-test@example.com"]);
        std::fs::write(repo.join("README.md"), "init").expect("write README");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "init"]);
    }

    #[test]
    fn test_validate_target_branch_worktree_reuses_shared_root() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);

        let shared_root = ensure_shared_worktrees_root(repo).expect("create shared root");
        let worktree_path = shared_root.join("target-branch");
        run_git(
            repo,
            &[
                "worktree",
                "add",
                worktree_path.to_str().expect("worktree path"),
                "target-branch",
            ],
        );

        let result = validate_target_branch_worktree_in(repo, "target-branch")
            .expect("validate target worktree");
        assert_eq!(result, Some(worktree_path));
    }

    #[test]
    fn test_validate_target_branch_worktree_errors_outside_shared_root() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);

        let outside_dir = TempDir::new().expect("outside dir");
        let outside_path = outside_dir.path().join("target-branch");
        run_git(
            repo,
            &[
                "worktree",
                "add",
                outside_path.to_str().expect("outside path"),
                "target-branch",
            ],
        );

        let err = validate_target_branch_worktree_in(repo, "target-branch")
            .expect_err("should error for outside worktree");
        assert!(
            err.contains("outside shared worktrees root"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_validate_target_branch_worktree_none_when_absent() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);

        let result = validate_target_branch_worktree_in(repo, "target-branch")
            .expect("validate target worktree");
        assert!(result.is_none());
    }
}
