use std::fs;
use std::path::Path;
use std::process::Command;

use super::create::{worktree_is_registered, worktree_path, worktrees_dir_abs};
use super::git::{agent_branch_name, delete_agent_branch, find_worktrees_with_branch, git_repo_root};
use super::list::list_worktrees;

/// Remove a worktree by its path (used when cleaning up worktrees with a specific branch).
pub(super) fn remove_worktree_by_path(repo_root: &Path, worktree_path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "remove", "--force", worktree_path])
        .output()
        .map_err(|e| format!("failed to run git worktree remove: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git worktree remove failed: {}", stderr.trim()))
    }
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

fn delete_branch_in(repo_root: &Path, branch_name: &str) -> Result<bool, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["branch", "-D", branch_name])
        .output()
        .map_err(|e| format!("failed to run git branch -D: {}", e))?;

    if output.status.success() {
        Ok(true)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not found") {
            Ok(false)
        } else {
            Err(format!("git branch -D failed: {}", stderr.trim()))
        }
    }
}

/// Delete a branch by full name.
pub fn delete_branch(branch_name: &str) -> Result<bool, String> {
    let repo_root = git_repo_root()?;
    delete_branch_in(&repo_root, branch_name)
}

/// Clean up a specific agent's worktree in the given directory.
/// Removes the worktree and optionally deletes the branch.
pub fn cleanup_agent_worktree(
    worktrees_dir: &Path,
    initial: char,
    delete_branch: bool,
) -> Result<(), String> {
    let repo_root = git_repo_root()?;
    let worktrees_dir = worktrees_dir_abs(worktrees_dir, &repo_root);

    let name = crate::agent::name_from_initial(initial)
        .ok_or_else(|| format!("invalid agent initial: {}", initial))?;
    let path = worktree_path(&worktrees_dir, initial, name);

    // Remove the worktree if it exists
    if path.exists() {
        let is_registered = worktree_is_registered(&repo_root, &path)?;
        if is_registered {
            let path_str = path.to_string_lossy().to_string();
            let output = Command::new("git")
                .arg("-C")
                .arg(&repo_root)
                .args(["worktree", "remove", "--force", &path_str])
                .output()
                .map_err(|e| format!("failed to run git worktree remove: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("git worktree remove failed: {}", stderr.trim()));
            }
        } else {
            // Not registered, just remove the directory
            fs::remove_dir_all(&path)
                .map_err(|e| format!("failed to remove worktree dir: {}", e))?;
        }
    }

    // Optionally delete the branch
    if delete_branch {
        // Before deleting the branch, remove any worktrees that have it checked out
        // (this handles multi-team scenarios where another team's worktree uses this branch)
        let branch = agent_branch_name(initial)
            .ok_or_else(|| format!("invalid agent initial: {}", initial))?;
        if let Ok(worktrees_with_branch) = find_worktrees_with_branch(&repo_root, &branch) {
            for wt_path in worktrees_with_branch {
                let _ = remove_worktree_by_path(&repo_root, &wt_path);
            }
        }
        delete_agent_branch(initial)?;
    }

    Ok(())
}

/// Clean up a feature/sprint worktree in the given directory.
/// Removes the worktree and optionally deletes the branch.
pub fn cleanup_feature_worktree(
    worktrees_dir: &Path,
    feature_branch: &str,
    delete_branch: bool,
) -> Result<(), String> {
    let feature = feature_branch.trim();
    if feature.is_empty() {
        return Err("feature branch name is empty".to_string());
    }

    let repo_root = git_repo_root()?;
    let worktrees_dir = worktrees_dir_abs(worktrees_dir, &repo_root);
    let path = worktrees_dir.join(feature);

    if path.exists() {
        let is_registered = worktree_is_registered(&repo_root, &path)?;
        if is_registered {
            let path_str = path.to_string_lossy().to_string();
            let output = Command::new("git")
                .arg("-C")
                .arg(&repo_root)
                .args(["worktree", "remove", "--force", &path_str])
                .output()
                .map_err(|e| format!("failed to run git worktree remove: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("git worktree remove failed: {}", stderr.trim()));
            }
        } else {
            fs::remove_dir_all(&path)
                .map_err(|e| format!("failed to remove worktree dir: {}", e))?;
        }
    }

    if delete_branch {
        if let Ok(worktrees_with_branch) = find_worktrees_with_branch(&repo_root, feature) {
            for wt_path in worktrees_with_branch {
                let _ = remove_worktree_by_path(&repo_root, &wt_path);
            }
        }
        let _ = delete_branch_in(&repo_root, feature)?;
    }

    Ok(())
}

/// Clean up worktrees for multiple agents.
/// Returns a summary of cleanup results.
#[derive(Debug, Default)]
pub struct CleanupSummary {
    pub cleaned: Vec<char>,
    pub errors: Vec<(char, String)>,
}

impl CleanupSummary {
    pub fn cleaned_count(&self) -> usize {
        self.cleaned.len()
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Clean up worktrees for specific agents.
pub fn cleanup_agent_worktrees(
    worktrees_dir: &Path,
    initials: &[char],
    delete_branches: bool,
) -> CleanupSummary {
    let mut summary = CleanupSummary::default();

    for &initial in initials {
        match cleanup_agent_worktree(worktrees_dir, initial, delete_branches) {
            Ok(()) => summary.cleaned.push(initial),
            Err(e) => summary.errors.push((initial, e)),
        }
    }

    summary
}
