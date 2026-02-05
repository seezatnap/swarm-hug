use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::cleanup::remove_worktree_by_path;
use super::git::{
    agent_branch_name, create_feature_branch_in, ensure_head, find_worktrees_with_branch,
    git_repo_root, registered_worktrees, repair_worktree_links,
};
use super::Worktree;
use crate::run_context::RunContext;

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
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    let target = abs.canonicalize().unwrap_or_else(|_| abs.clone());

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
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                continue;
            }
            let candidate = PathBuf::from(trimmed);
            let resolved = if candidate.is_absolute() {
                candidate
            } else {
                repo_root.join(candidate)
            };
            if resolved == abs {
                return Ok(true);
            }
            let resolved_canonical = resolved.canonicalize().unwrap_or(resolved);
            if resolved_canonical == target {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Legacy worktree path without namespacing.
/// Format: agent-{INITIAL}-{name} (e.g., agent-A-Aaron)
#[cfg(test)]
pub(super) fn worktree_path(root: &Path, initial: char, name: &str) -> PathBuf {
    root.join(format!("agent-{}-{}", initial, name))
}

/// Worktree path using the namespaced branch name from RunContext.
/// Format: {branch_name} (e.g., greenfield-agent-aaron-a3f8k2)
pub(super) fn worktree_path_with_context(root: &Path, ctx: &RunContext, initial: char) -> PathBuf {
    let branch = ctx.agent_branch(initial);
    root.join(branch)
}

/// Create worktrees in the specified directory with project-namespaced branch names.
///
/// The `worktrees_dir` should be the full path to the worktrees directory
/// (e.g., ".swarm-hug/authentication/worktrees" in multi-team mode).
///
/// The `ctx` parameter provides the project name and run hash used to create
/// namespaced branch and worktree names, ensuring isolation between projects
/// and sprint runs.
///
/// # Arguments
/// * `worktrees_dir` - Directory where worktrees will be created
/// * `assignments` - List of (agent_initial, task_description) tuples
/// * `base_branch` - Branch to base agent branches on
/// * `ctx` - RunContext with project name and run hash for namespacing
///
/// # Examples
/// ```ignore
/// let ctx = RunContext::new("greenfield", 1);
/// let worktrees = create_worktrees_in(
///     Path::new(".swarm-hug/greenfield/worktrees"),
///     &[('A', "Task 1".to_string())],
///     "greenfield-sprint-1-abc123",
///     &ctx,
/// )?;
/// // Creates worktree at: .swarm-hug/greenfield/worktrees/greenfield-agent-aaron-abc123
/// // With branch: greenfield-agent-aaron-abc123
/// ```
pub fn create_worktrees_in(
    worktrees_dir: &Path,
    assignments: &[(char, String)],
    base_branch: &str,
    ctx: &RunContext,
) -> Result<Vec<Worktree>, String> {
    let mut created = Vec::new();
    let mut seen = HashSet::new();

    if assignments.is_empty() {
        return Ok(created);
    }
    let base = base_branch.trim();
    if base.is_empty() {
        return Err("base branch name is empty".to_string());
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

        // Use namespaced branch and path from RunContext
        let branch = agent_branch_name(ctx, upper);
        let path = worktree_path_with_context(&worktrees_dir, ctx, upper);
        let path_str = path.to_string_lossy().to_string();

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

        // Create fresh worktree with new branch from the base branch
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(&repo_root)
            .args(["worktree", "add", "--relative-paths"]);
        let output = cmd
            .args(["-B", &branch, &path_str, base])
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

        repair_worktree_links(&repo_root, &path)
            .map_err(|e| format!("git worktree repair failed for {}: {}", path.display(), e))?;

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
/// The feature branch is created from `source_branch` (the branch to fork from).
pub fn create_feature_worktree_in(
    worktrees_dir: &Path,
    feature_branch: &str,
    source_branch: &str,
) -> Result<PathBuf, String> {
    let feature = feature_branch.trim();
    if feature.is_empty() {
        return Err("feature branch name is empty".to_string());
    }
    let source = source_branch.trim();
    if source.is_empty() {
        return Err("source branch name is empty".to_string());
    }

    let repo_root = git_repo_root()?;
    ensure_head(&repo_root)?;
    let worktrees_dir = worktrees_dir_abs(worktrees_dir, &repo_root);

    fs::create_dir_all(&worktrees_dir)
        .map_err(|e| format!("failed to create worktrees dir: {}", e))?;

    create_feature_branch_in(&repo_root, feature, source)?;

    let path = worktrees_dir.join(feature);
    let path_str = path.to_string_lossy().to_string();

    if let Ok(existing) = find_worktrees_with_branch(&repo_root, feature) {
        if existing.iter().any(|p| p == &path_str) {
            repair_worktree_links(&repo_root, &path)
                .map_err(|e| format!("git worktree repair failed for {}: {}", path.display(), e))?;
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

    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(&repo_root)
        .args(["worktree", "add", "--relative-paths"]);
    let output = cmd
        .args([&path_str, feature])
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

    repair_worktree_links(&repo_root, &path)
        .map_err(|e| format!("git worktree repair failed for {}: {}", path.display(), e))?;

    Ok(path)
}

// Note: Legacy create_worktrees() function removed.
// All worktree creation now requires RunContext for proper namespacing.
// Use create_worktrees_in() with a RunContext instead.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use crate::run_context::RunContext;
    use crate::testutil::with_temp_cwd;

    use super::{create_feature_worktree_in, create_worktrees_in, worktree_path, worktree_path_with_context};

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

    fn run_git_in(dir: &Path, args: &[&str]) -> Output {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("failed to run git command");
        assert!(
            output.status.success(),
            "git -C {} {:?} failed\nstdout:\n{}\nstderr:\n{}",
            dir.display(),
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
    fn test_worktree_path_legacy() {
        let root = Path::new("/tmp/worktrees");
        let path = worktree_path(root, 'A', "Aaron");
        assert_eq!(path, Path::new("/tmp/worktrees/agent-A-Aaron"));
    }

    #[test]
    fn test_worktree_path_with_context() {
        let root = Path::new("/tmp/worktrees");
        let ctx = RunContext::new("greenfield", 1);
        let path = worktree_path_with_context(root, &ctx, 'A');

        // Path should be: /tmp/worktrees/greenfield-agent-aaron-{hash}
        let path_str = path.to_string_lossy();
        assert!(path_str.starts_with("/tmp/worktrees/greenfield-agent-aaron-"));
        assert_eq!(
            path_str.len(),
            "/tmp/worktrees/greenfield-agent-aaron-".len() + 6
        );
    }

    #[test]
    fn test_worktree_path_with_context_different_agents() {
        let root = Path::new("/tmp/worktrees");
        let ctx = RunContext::new("greenfield", 1);

        let path_a = worktree_path_with_context(root, &ctx, 'A');
        let path_b = worktree_path_with_context(root, &ctx, 'B');

        assert!(path_a.to_string_lossy().contains("agent-aaron"));
        assert!(path_b.to_string_lossy().contains("agent-betty"));

        // Both should have the same hash
        let hash = ctx.hash();
        assert!(path_a.to_string_lossy().ends_with(hash));
        assert!(path_b.to_string_lossy().ends_with(hash));
    }

    #[test]
    fn test_create_feature_worktree_in_creates_worktree() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["branch", "source-branch"]);

            let worktrees_dir = Path::new(".swarm-hug/alpha/worktrees");
            let path = create_feature_worktree_in(
                worktrees_dir,
                "alpha-sprint-1",
                "source-branch",
            )
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

            let path_again = create_feature_worktree_in(
                worktrees_dir,
                "alpha-sprint-1",
                "source-branch",
            )
            .expect("idempotent create");
            assert_eq!(path, path_again);
        });
    }

    #[test]
    fn test_create_worktrees_in_uses_base_branch() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["checkout", "-b", "alpha-sprint-1"]);
            fs::write("feature.txt", "feature").expect("write feature file");
            run_git(&["add", "."]);
            run_git(&["commit", "-m", "feature commit"]);
            let base_commit = String::from_utf8_lossy(&run_git(&["rev-parse", "HEAD"]).stdout)
                .trim()
                .to_string();

            let ctx = RunContext::new("alpha", 1);
            let worktrees_dir = Path::new(".swarm-hug/alpha/worktrees");
            let assignments = vec![('A', "Task one".to_string())];
            let worktrees = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "alpha-sprint-1",
                &ctx,
            )
            .expect("create worktrees");
            assert_eq!(worktrees.len(), 1);
            let wt_path = &worktrees[0].path;
            assert!(wt_path.exists());

            // Verify the worktree path uses namespaced name
            let path_str = wt_path.to_string_lossy();
            assert!(path_str.contains(&format!("alpha-agent-aaron-{}", ctx.hash())));

            let output = Command::new("git")
                .arg("-C")
                .arg(wt_path)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("failed to run git rev-parse");
            assert!(output.status.success());
            let wt_commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
            assert_eq!(wt_commit, base_commit);

            // Verify branch name is namespaced
            let branch_output = Command::new("git")
                .arg("-C")
                .arg(wt_path)
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .output()
                .expect("failed to run git rev-parse --abbrev-ref");
            let branch = String::from_utf8_lossy(&branch_output.stdout).trim().to_string();
            assert_eq!(branch, ctx.agent_branch('A'));
        });
    }

    #[test]
    fn test_create_worktrees_in_recreates_existing_worktree() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["checkout", "-b", "alpha-sprint-1"]);
            fs::write("feature.txt", "feature").expect("write feature file");
            run_git(&["add", "."]);
            run_git(&["commit", "-m", "feature commit"]);
            let base_commit = String::from_utf8_lossy(&run_git(&["rev-parse", "HEAD"]).stdout)
                .trim()
                .to_string();

            // Use the same context for both creates to test recreation
            let ctx = RunContext::new("alpha", 1);
            let worktrees_dir = Path::new(".swarm-hug/alpha/worktrees");
            let assignments = vec![('A', "Task one".to_string())];
            let worktrees = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "alpha-sprint-1",
                &ctx,
            )
            .expect("create worktrees");
            let wt_path = &worktrees[0].path;

            fs::write(wt_path.join("task.txt"), "task").expect("write task file");
            run_git_in(wt_path, &["add", "."]);
            run_git_in(wt_path, &["commit", "-m", "task commit"]);

            // Recreate with same context - should reset to base_commit
            let worktrees_again = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "alpha-sprint-1",
                &ctx,
            )
            .expect("recreate worktree");
            let wt_path_again = &worktrees_again[0].path;

            let output = Command::new("git")
                .arg("-C")
                .arg(wt_path_again)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("failed to run git rev-parse");
            assert!(output.status.success());
            let wt_commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
            assert_eq!(wt_commit, base_commit);
            assert!(!wt_path_again.join("task.txt").exists());
        });
    }

    #[test]
    fn test_create_feature_worktree_in_forks_from_source_not_target() {
        with_temp_cwd(|| {
            init_repo();
            // Name the default branch explicitly
            run_git(&["branch", "-M", "main"]);

            // Create a "source" branch with a unique commit
            run_git(&["checkout", "-b", "source-branch"]);
            fs::write("source-file.txt", "from source").expect("write source file");
            run_git(&["add", "."]);
            run_git(&["commit", "-m", "source commit"]);
            let source_commit = String::from_utf8_lossy(
                &Command::new("git")
                    .args(["rev-parse", "HEAD"])
                    .output()
                    .expect("rev-parse")
                    .stdout,
            )
            .trim()
            .to_string();

            // Create a "target" branch with a different commit
            run_git(&["checkout", "main"]);
            run_git(&["checkout", "-b", "target-branch"]);
            fs::write("target-file.txt", "from target").expect("write target file");
            run_git(&["add", "."]);
            run_git(&["commit", "-m", "target commit"]);
            let target_commit = String::from_utf8_lossy(
                &Command::new("git")
                    .args(["rev-parse", "HEAD"])
                    .output()
                    .expect("rev-parse")
                    .stdout,
            )
            .trim()
            .to_string();

            assert_ne!(source_commit, target_commit, "source and target should differ");

            // Create feature worktree from source_branch (not target_branch)
            let worktrees_dir = Path::new(".swarm-hug/alpha/worktrees");
            let path = create_feature_worktree_in(
                worktrees_dir,
                "alpha-sprint-1",
                "source-branch",
            )
            .expect("create feature worktree from source");

            // The feature worktree should be at source_commit, not target_commit
            let wt_commit = String::from_utf8_lossy(
                &Command::new("git")
                    .arg("-C")
                    .arg(&path)
                    .args(["rev-parse", "HEAD"])
                    .output()
                    .expect("rev-parse in worktree")
                    .stdout,
            )
            .trim()
            .to_string();

            assert_eq!(
                wt_commit, source_commit,
                "feature worktree should fork from source_branch"
            );
            assert_ne!(
                wt_commit, target_commit,
                "feature worktree should NOT be at target_branch"
            );

            // Verify the source-branch file exists (not target-branch file)
            assert!(
                path.join("source-file.txt").exists(),
                "source file should exist in worktree"
            );
            assert!(
                !path.join("target-file.txt").exists(),
                "target file should NOT exist in worktree"
            );
        });
    }

    #[test]
    fn test_create_worktrees_in_different_projects_no_conflict() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["checkout", "-b", "base-branch"]);

            let ctx1 = RunContext::new("greenfield", 1);
            let ctx2 = RunContext::new("payments", 1);
            let worktrees_dir = Path::new(".swarm-hug/worktrees");
            let assignments = vec![('A', "Task one".to_string())];

            // Create worktree for greenfield
            let worktrees1 = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "base-branch",
                &ctx1,
            )
            .expect("create greenfield worktrees");
            assert_eq!(worktrees1.len(), 1);

            // Create worktree for payments - should succeed without conflict
            let worktrees2 = create_worktrees_in(
                worktrees_dir,
                &assignments,
                "base-branch",
                &ctx2,
            )
            .expect("create payments worktrees");
            assert_eq!(worktrees2.len(), 1);

            // Both worktrees should exist
            assert!(worktrees1[0].path.exists());
            assert!(worktrees2[0].path.exists());

            // They should have different paths
            assert_ne!(worktrees1[0].path, worktrees2[0].path);

            // Verify branch names are different
            let branch1 = ctx1.agent_branch('A');
            let branch2 = ctx2.agent_branch('A');
            assert_ne!(branch1, branch2);
            assert!(branch1.starts_with("greenfield-agent-aaron-"));
            assert!(branch2.starts_with("payments-agent-aaron-"));
        });
    }
}
