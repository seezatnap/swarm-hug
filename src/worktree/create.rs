use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::cleanup::remove_worktree_by_path;
use super::git::{
    agent_branch_name, create_feature_branch_in, ensure_head, find_worktrees_with_branch,
    git_repo_root, registered_worktrees,
};
use super::Worktree;

pub(super) fn worktrees_dir_abs(worktrees_dir: &Path, repo_root: &Path) -> PathBuf {
    if worktrees_dir.is_absolute() {
        worktrees_dir.to_path_buf()
    } else {
        repo_root.join(worktrees_dir)
    }
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

pub(super) fn worktree_is_registered(repo_root: &Path, path: &Path) -> Result<bool, String> {
    let registered = registered_worktrees(repo_root)?;
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    Ok(is_registered_path(&registered, &abs))
}

pub(super) fn worktree_path(root: &Path, initial: char, name: &str) -> PathBuf {
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

        let branch = agent_branch_name(upper)
            .ok_or_else(|| format!("invalid agent initial: {}", upper))?;

        // If worktree already exists, remove it first to ensure a fresh start
        if is_registered_path(&registered, &path) {
            let _ = Command::new("git")
                .arg("-C")
                .arg(&repo_root)
                .args(["worktree", "remove", "--force", &path_str])
                .output();
            registered.remove(&path_str);
        }

        // If path exists but not registered, remove the directory
        if path.exists() {
            fs::remove_dir_all(&path)
                .map_err(|e| format!("failed to remove stale worktree dir {}: {}", path.display(), e))?;
        }

        // Before deleting the branch, remove any worktrees that have it checked out
        // (this handles multi-team scenarios where another team's worktree uses this branch)
        if let Ok(worktrees_with_branch) = find_worktrees_with_branch(&repo_root, &branch) {
            for wt_path in worktrees_with_branch {
                // Don't fail if removal fails - we'll get the error on branch delete or worktree add
                let _ = remove_worktree_by_path(&repo_root, &wt_path);
            }
        }

        // Delete the branch if it exists (to ensure fresh start from HEAD)
        let _ = Command::new("git")
            .arg("-C")
            .arg(&repo_root)
            .args(["branch", "-D", &branch])
            .output();

        // Create fresh worktree with new branch from HEAD
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

/// Create a feature/sprint worktree under the specified worktrees directory.
/// The worktree path is `<worktrees_dir>/<feature_branch>`.
pub fn create_feature_worktree_in(
    worktrees_dir: &Path,
    feature_branch: &str,
    target_branch: &str,
) -> Result<PathBuf, String> {
    let feature = feature_branch.trim();
    if feature.is_empty() {
        return Err("feature branch name is empty".to_string());
    }
    let target = target_branch.trim();
    if target.is_empty() {
        return Err("target branch name is empty".to_string());
    }

    let repo_root = git_repo_root()?;
    ensure_head(&repo_root)?;
    let worktrees_dir = worktrees_dir_abs(worktrees_dir, &repo_root);

    fs::create_dir_all(&worktrees_dir)
        .map_err(|e| format!("failed to create worktrees dir: {}", e))?;

    create_feature_branch_in(&repo_root, feature, target)?;

    let path = worktrees_dir.join(feature);
    let path_str = path.to_string_lossy().to_string();

    if let Ok(existing) = find_worktrees_with_branch(&repo_root, feature) {
        if existing.iter().any(|p| p == &path_str) {
            return Ok(path);
        }
        if !existing.is_empty() {
            return Err(format!(
                "feature branch '{}' already checked out in another worktree: {}",
                feature,
                existing.join(", ")
            ));
        }
    }

    if worktree_is_registered(&repo_root, &path)? {
        return Err(format!(
            "worktree path '{}' is already registered for another branch",
            path.display()
        ));
    }

    if path.exists() {
        fs::remove_dir_all(&path)
            .map_err(|e| format!("failed to remove stale worktree dir {}: {}", path.display(), e))?;
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(&repo_root)
        .args(["worktree", "add", &path_str, feature])
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

    Ok(path)
}

/// Legacy function for backwards compatibility.
/// Creates worktrees under `base/worktrees/`.
pub fn create_worktrees(base: &Path, assignments: &[(char, String)]) -> Result<Vec<Worktree>, String> {
    create_worktrees_in(&base.join("worktrees"), assignments)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use crate::testutil::with_temp_cwd;

    use super::{create_feature_worktree_in, worktree_path};

    fn run_git(args: &[&str]) -> Output {
        let output = Command::new("git")
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

    fn init_repo() {
        run_git(&["init"]);
        run_git(&["config", "user.name", "Swarm Test"]);
        run_git(&["config", "user.email", "swarm-test@example.com"]);
        fs::write("README.md", "init").expect("write README");
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "init"]);
    }

    #[test]
    fn test_worktree_path() {
        let root = Path::new("/tmp/worktrees");
        let path = worktree_path(root, 'A', "Aaron");
        assert_eq!(path, Path::new("/tmp/worktrees/agent-A-Aaron"));
    }

    #[test]
    fn test_create_feature_worktree_in_creates_worktree() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["branch", "target-branch"]);

            let worktrees_dir = Path::new(".swarm-hug/alpha/worktrees");
            let path = create_feature_worktree_in(worktrees_dir, "alpha-sprint-1", "target-branch")
                .expect("create feature worktree");

            assert!(path.ends_with("alpha-sprint-1"));
            assert!(path.exists());

            let output = Command::new("git")
                .arg("-C")
                .arg(&path)
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .output()
                .expect("failed to run git rev-parse");
            assert!(output.status.success());
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            assert_eq!(branch, "alpha-sprint-1");

            let path_again =
                create_feature_worktree_in(worktrees_dir, "alpha-sprint-1", "target-branch")
                    .expect("idempotent create");
            assert_eq!(path, path_again);
        });
    }
}
