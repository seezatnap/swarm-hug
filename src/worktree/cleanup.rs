use std::fs;
use std::path::Path;
use std::process::Command;

use super::create::{worktree_is_registered, worktree_path_with_context, worktrees_dir_abs};
use super::git::{find_worktrees_with_branch, git_repo_root};
use super::list::list_worktrees;
use crate::run_context::RunContext;

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
///
/// Uses the `RunContext` to determine the namespaced branch and worktree path,
/// ensuring cleanup only affects the current run's artifacts (matched by hash).
///
/// # Arguments
/// * `worktrees_dir` - Directory containing agent worktrees
/// * `initial` - Agent's initial (A-Z)
/// * `delete_branch` - Whether to also delete the agent's branch
/// * `ctx` - RunContext with project name and run hash for matching
pub fn cleanup_agent_worktree(
    worktrees_dir: &Path,
    initial: char,
    delete_branch: bool,
    ctx: &RunContext,
) -> Result<(), String> {
    let repo_root = git_repo_root()?;
    let worktrees_dir = worktrees_dir_abs(worktrees_dir, &repo_root);

    // Validate agent initial
    crate::agent::name_from_initial(initial)
        .ok_or_else(|| format!("invalid agent initial: {}", initial))?;

    // Use namespaced path from RunContext
    let path = worktree_path_with_context(&worktrees_dir, ctx, initial);
    let branch = ctx.agent_branch(initial);

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
        if let Ok(worktrees_with_branch) = find_worktrees_with_branch(&repo_root, &branch) {
            for wt_path in worktrees_with_branch {
                let _ = remove_worktree_by_path(&repo_root, &wt_path);
            }
        }
        // Delete the namespaced branch
        delete_branch_in(&repo_root, &branch)?;
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
///
/// Uses the `RunContext` to determine the namespaced branch and worktree paths,
/// ensuring cleanup only affects the current run's artifacts (matched by hash).
///
/// # Arguments
/// * `worktrees_dir` - Directory containing agent worktrees
/// * `initials` - List of agent initials to clean up
/// * `delete_branches` - Whether to also delete the agents' branches
/// * `ctx` - RunContext with project name and run hash for matching
pub fn cleanup_agent_worktrees(
    worktrees_dir: &Path,
    initials: &[char],
    delete_branches: bool,
    ctx: &RunContext,
) -> CleanupSummary {
    let mut summary = CleanupSummary::default();

    for &initial in initials {
        match cleanup_agent_worktree(worktrees_dir, initial, delete_branches, ctx) {
            Ok(()) => summary.cleaned.push(initial),
            Err(e) => summary.errors.push((initial, e)),
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use crate::run_context::RunContext;
    use crate::testutil::with_temp_cwd;

    use super::super::create::create_worktrees_in;
    use super::{cleanup_agent_worktree, cleanup_agent_worktrees};

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

    fn branch_exists(branch: &str) -> bool {
        let ref_name = format!("refs/heads/{}", branch);
        Command::new("git")
            .args(["show-ref", "--verify", "--quiet", &ref_name])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn test_cleanup_agent_worktree_removes_worktree() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["checkout", "-b", "base-branch"]);

            let ctx = RunContext::new("greenfield", 1);
            let worktrees_dir = Path::new(".swarm-hug/greenfield/worktrees");
            let assignments = vec![('A', "Task one".to_string())];

            // Create the worktree
            let worktrees = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "base-branch",
                &ctx,
            )
            .expect("create worktrees");
            let wt_path = &worktrees[0].path;
            assert!(wt_path.exists(), "worktree should exist before cleanup");

            // Clean up the worktree (without deleting branch)
            cleanup_agent_worktree(worktrees_dir, 'A', false, &ctx)
                .expect("cleanup should succeed");

            assert!(!wt_path.exists(), "worktree should not exist after cleanup");
            // Branch should still exist since we didn't delete it
            let branch = ctx.agent_branch('A');
            assert!(branch_exists(&branch), "branch should still exist");
        });
    }

    #[test]
    fn test_cleanup_agent_worktree_deletes_branch() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["checkout", "-b", "base-branch"]);

            let ctx = RunContext::new("greenfield", 1);
            let worktrees_dir = Path::new(".swarm-hug/greenfield/worktrees");
            let assignments = vec![('A', "Task one".to_string())];

            // Create the worktree
            let worktrees = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "base-branch",
                &ctx,
            )
            .expect("create worktrees");
            let wt_path = &worktrees[0].path;
            let branch = ctx.agent_branch('A');

            assert!(wt_path.exists(), "worktree should exist before cleanup");
            assert!(branch_exists(&branch), "branch should exist before cleanup");

            // Clean up with branch deletion
            cleanup_agent_worktree(worktrees_dir, 'A', true, &ctx)
                .expect("cleanup should succeed");

            assert!(!wt_path.exists(), "worktree should not exist after cleanup");
            assert!(!branch_exists(&branch), "branch should not exist after cleanup");
        });
    }

    #[test]
    fn test_cleanup_agent_worktree_only_affects_matching_hash() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["checkout", "-b", "base-branch"]);

            // Create worktrees with two different contexts (simulating different runs)
            let ctx1 = RunContext::new("greenfield", 1);
            let ctx2 = RunContext::new("greenfield", 1); // Different hash
            let worktrees_dir = Path::new(".swarm-hug/greenfield/worktrees");
            let assignments = vec![('A', "Task one".to_string())];

            let worktrees1 = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "base-branch",
                &ctx1,
            )
            .expect("create worktrees for run 1");
            let worktrees2 = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "base-branch",
                &ctx2,
            )
            .expect("create worktrees for run 2");

            let wt_path1 = &worktrees1[0].path;
            let wt_path2 = &worktrees2[0].path;
            assert!(wt_path1.exists(), "worktree 1 should exist");
            assert!(wt_path2.exists(), "worktree 2 should exist");

            // Clean up only ctx1's worktree
            cleanup_agent_worktree(worktrees_dir, 'A', true, &ctx1)
                .expect("cleanup should succeed");

            assert!(!wt_path1.exists(), "worktree 1 should be removed");
            assert!(wt_path2.exists(), "worktree 2 should still exist");
        });
    }

    #[test]
    fn test_cleanup_agent_worktrees_cleans_multiple() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["checkout", "-b", "base-branch"]);

            let ctx = RunContext::new("greenfield", 1);
            let worktrees_dir = Path::new(".swarm-hug/greenfield/worktrees");
            let assignments = vec![
                ('A', "Task one".to_string()),
                ('B', "Task two".to_string()),
            ];

            // Create the worktrees
            let worktrees = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "base-branch",
                &ctx,
            )
            .expect("create worktrees");
            assert_eq!(worktrees.len(), 2);

            let wt_path_a = &worktrees[0].path;
            let wt_path_b = &worktrees[1].path;
            assert!(wt_path_a.exists());
            assert!(wt_path_b.exists());

            // Clean up both worktrees
            let summary = cleanup_agent_worktrees(worktrees_dir, &['A', 'B'], true, &ctx);

            assert_eq!(summary.cleaned_count(), 2);
            assert!(!summary.has_errors());
            assert!(!wt_path_a.exists());
            assert!(!wt_path_b.exists());
        });
    }

    #[test]
    fn test_cleanup_agent_worktrees_summary_tracks_results() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["checkout", "-b", "base-branch"]);

            let ctx = RunContext::new("greenfield", 1);
            let worktrees_dir = Path::new(".swarm-hug/greenfield/worktrees");
            let assignments = vec![('A', "Task one".to_string())];

            // Create only one worktree (for 'A')
            create_worktrees_in(
                worktrees_dir,
                &assignments,
                "base-branch",
                &ctx,
            )
            .expect("create worktrees");

            // Try to clean up 'A' and 'B' - 'B' doesn't exist but shouldn't error
            let summary = cleanup_agent_worktrees(worktrees_dir, &['A', 'B'], true, &ctx);

            // 'A' should be cleaned successfully
            assert!(summary.cleaned.contains(&'A'));
            // 'B' should also succeed (cleaning non-existent worktree is OK)
            assert!(summary.cleaned.contains(&'B'));
            assert!(!summary.has_errors());
        });
    }

    #[test]
    fn test_cleanup_agent_worktree_invalid_initial() {
        with_temp_cwd(|| {
            init_repo();

            let ctx = RunContext::new("greenfield", 1);
            let worktrees_dir = Path::new(".swarm-hug/greenfield/worktrees");

            // Invalid initial should return an error
            let result = cleanup_agent_worktree(worktrees_dir, '1', false, &ctx);
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("invalid agent initial"));
        });
    }

    #[test]
    fn test_cleanup_agent_worktree_noop_for_nonexistent() {
        with_temp_cwd(|| {
            init_repo();

            let ctx = RunContext::new("greenfield", 1);
            let worktrees_dir = Path::new(".swarm-hug/greenfield/worktrees");

            // Cleaning a non-existent worktree should succeed (no-op)
            let result = cleanup_agent_worktree(worktrees_dir, 'A', false, &ctx);
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_cleanup_different_projects_isolated() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["checkout", "-b", "base-branch"]);

            // Create worktrees for two different projects
            let ctx_greenfield = RunContext::new("greenfield", 1);
            let ctx_payments = RunContext::new("payments", 1);
            let worktrees_dir = Path::new(".swarm-hug/worktrees");
            let assignments = vec![('A', "Task one".to_string())];

            let worktrees_greenfield = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "base-branch",
                &ctx_greenfield,
            )
            .expect("create greenfield worktrees");
            let worktrees_payments = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "base-branch",
                &ctx_payments,
            )
            .expect("create payments worktrees");

            let wt_greenfield = &worktrees_greenfield[0].path;
            let wt_payments = &worktrees_payments[0].path;
            assert!(wt_greenfield.exists());
            assert!(wt_payments.exists());

            // Cleanup greenfield should not affect payments
            cleanup_agent_worktree(worktrees_dir, 'A', true, &ctx_greenfield)
                .expect("cleanup greenfield");

            assert!(!wt_greenfield.exists(), "greenfield worktree should be removed");
            assert!(wt_payments.exists(), "payments worktree should still exist");

            // Cleanup payments
            cleanup_agent_worktree(worktrees_dir, 'A', true, &ctx_payments)
                .expect("cleanup payments");

            assert!(!wt_payments.exists(), "payments worktree should be removed");
        });
    }
}
