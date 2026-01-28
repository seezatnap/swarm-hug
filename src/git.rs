use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use swarm::team;

fn git_repo_root() -> Result<PathBuf, String> {
    let output = process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("git rev-parse failed: {}", e))?;

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

fn resolve_repo_relative_path(
    path: &str,
    cwd: &Path,
    repo_root: &Path,
) -> Result<Option<(PathBuf, PathBuf)>, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let raw = Path::new(trimmed);
    let source = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    };

    if !source.exists() {
        return Ok(None);
    }

    let source = source
        .canonicalize()
        .map_err(|e| format!("failed to resolve {}: {}", source.display(), e))?;
    let repo_root = repo_root
        .canonicalize()
        .map_err(|e| format!("failed to resolve repo root: {}", e))?;

    let relative = source.strip_prefix(&repo_root).map_err(|_| {
        format!(
            "path '{}' is outside repo root '{}'",
            source.display(),
            repo_root.display()
        )
    })?;

    Ok(Some((relative.to_path_buf(), source)))
}

pub(crate) fn sync_paths_to_worktree(
    worktree_root: &Path,
    paths: &[&str],
) -> Result<Vec<String>, String> {
    let repo_root = git_repo_root()?;
    let cwd = std::env::current_dir().map_err(|e| format!("failed to get cwd: {}", e))?;
    let mut synced = Vec::new();

    for path in paths {
        let Some((relative, source)) = resolve_repo_relative_path(path, &cwd, &repo_root)? else {
            continue;
        };

        let dest = worktree_root.join(&relative);
        if dest != source {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create {}: {}", parent.display(), e))?;
            }
            fs::copy(&source, &dest)
                .map_err(|e| format!("failed to sync {}: {}", source.display(), e))?;
        }

        synced.push(relative.to_string_lossy().to_string());
    }

    Ok(synced)
}

pub(crate) fn commit_files_in(
    repo_dir: &Path,
    paths: &[&str],
    message: &str,
) -> Result<bool, String> {
    let existing: Vec<String> = paths
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .filter(|p| {
            let path = Path::new(p);
            let candidate = if path.is_absolute() {
                path.to_path_buf()
            } else {
                repo_dir.join(path)
            };
            candidate.exists()
        })
        .map(|p| p.to_string())
        .collect();

    if existing.is_empty() {
        return Ok(false);
    }

    let add_result = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .arg("add")
        .args(&existing)
        .output();

    match add_result {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git add failed: {}", stderr));
        }
        Err(e) => return Err(format!("git add failed: {}", e)),
    }

    // Check if there are staged changes
    let diff_result = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["diff", "--cached", "--quiet"])
        .output();

    let has_changes = match diff_result {
        Ok(output) => !output.status.success(), // exit code 1 means changes exist
        Err(_) => false,
    };

    if !has_changes {
        return Ok(false); // No changes to commit
    }

    // Commit the changes
    let commit_result = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["commit", "-m", message])
        .env("GIT_AUTHOR_NAME", "Swarm ScrumMaster")
        .env("GIT_AUTHOR_EMAIL", "swarm@local")
        .env("GIT_COMMITTER_NAME", "Swarm ScrumMaster")
        .env("GIT_COMMITTER_EMAIL", "swarm@local")
        .output();

    match commit_result {
        Ok(output) if output.status.success() => Ok(true),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't fail if there's nothing to commit
            if stderr.contains("nothing to commit") {
                Ok(false)
            } else {
                Err(format!("git commit failed: {}", stderr))
            }
        }
        Err(e) => Err(format!("git commit failed: {}", e)),
    }
}

pub(crate) fn commit_files_in_worktree(
    worktree_root: &Path,
    paths: &[&str],
    message: &str,
) -> Result<bool, String> {
    let synced = sync_paths_to_worktree(worktree_root, paths)?;
    let synced_refs: Vec<&str> = synced.iter().map(String::as_str).collect();
    commit_files_in(worktree_root, &synced_refs, message)
}

fn ensure_branch_checked_out(repo_dir: &Path, branch: &str) -> Result<(), String> {
    let target = branch.trim();
    if target.is_empty() {
        return Err("branch name is empty".to_string());
    }

    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(|e| format!("git rev-parse failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git rev-parse failed: {}", stderr.trim()));
    }

    let current = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if current == target {
        return Ok(());
    }

    let checkout = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["checkout", target])
        .output()
        .map_err(|e| format!("git checkout failed: {}", e))?;

    if checkout.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&checkout.stderr);
        Err(format!("git checkout failed: {}", stderr.trim()))
    }
}

pub(crate) fn commit_files_in_worktree_on_branch(
    worktree_root: &Path,
    branch: &str,
    paths: &[&str],
    message: &str,
) -> Result<bool, String> {
    ensure_branch_checked_out(worktree_root, branch)?;
    commit_files_in_worktree(worktree_root, paths, message)
}

/// Commit task assignment changes to git.
///
/// # Arguments
/// * `sprint_branch` - Sprint/feature branch name to commit on
/// * `tasks_file` - Path to the team's tasks.md file
/// * `sprint_history_file` - Path to the team's sprint-history.json file
/// * `team_name` - Formatted team name for commit message (e.g., "Greenfield")
/// * `sprint_number` - The historical sprint number for this team
pub(crate) fn commit_task_assignments(
    worktree_root: &Path,
    sprint_branch: &str,
    tasks_file: &str,
    sprint_history_file: &str,
    team_state_file: &str,
    team_name: &str,
    sprint_number: usize,
) -> Result<(), String> {
    let assignments_path = format!("{}/{}", team::SWARM_HUG_DIR, team::ASSIGNMENTS_FILE);
    let commit_msg = format!("{} Sprint {}: task assignments", team_name, sprint_number);
    if commit_files_in_worktree_on_branch(
        worktree_root,
        sprint_branch,
        &[
            tasks_file,
            sprint_history_file,
            team_state_file,
            assignments_path.as_str(),
        ],
        &commit_msg,
    )? {
        println!("  Committed task assignments to git.");
    }
    Ok(())
}

/// Commit sprint completion (updated tasks and released assignments).
///
/// # Arguments
/// * `sprint_branch` - Sprint/feature branch name to commit on
/// * `tasks_file` - Path to the team's tasks.md file
/// * `team_name` - Formatted team name for commit message (e.g., "Greenfield")
/// * `sprint_number` - The historical sprint number for this team
pub(crate) fn commit_sprint_completion(
    worktree_root: &Path,
    sprint_branch: &str,
    tasks_file: &str,
    team_name: &str,
    sprint_number: usize,
) -> Result<(), String> {
    let assignments_path = format!("{}/{}", team::SWARM_HUG_DIR, team::ASSIGNMENTS_FILE);
    let commit_msg = format!("{} Sprint {}: completed", team_name, sprint_number);
    if commit_files_in_worktree_on_branch(
        worktree_root,
        sprint_branch,
        &[tasks_file, assignments_path.as_str()],
        &commit_msg,
    )? {
        println!("  Committed sprint completion to git.");
    }
    Ok(())
}

/// Get the current git commit hash from a specific repo/worktree.
pub(crate) fn get_current_commit_in(repo_dir: &Path) -> Option<String> {
    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get git log between two commits (messages and stats, no diffs) for a specific repo/worktree.
pub(crate) fn get_git_log_range_in(
    repo_dir: &Path,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let range = format!("{}..{}", from, to);
    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["log", "--stat", &range])
        .output()
        .map_err(|e| format!("failed to run git log: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        // If range is invalid (no commits), return empty string
        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::ensure_branch_checked_out;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn run_git(repo_dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_dir)
            .args(args)
            .output()
            .expect("failed to run git");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    #[test]
    fn test_ensure_branch_checked_out_switches_branch() {
        let temp = TempDir::new().expect("temp dir");
        let repo_dir = temp.path();

        run_git(repo_dir, &["init"]);
        run_git(repo_dir, &["config", "user.name", "Swarm Test"]);
        run_git(repo_dir, &["config", "user.email", "swarm-test@example.com"]);

        fs::write(repo_dir.join("README.md"), "hello").expect("write file");
        run_git(repo_dir, &["add", "."]);
        run_git(repo_dir, &["commit", "-m", "init"]);
        run_git(repo_dir, &["branch", "feature"]);

        ensure_branch_checked_out(repo_dir, "feature")
            .expect("should checkout feature branch");
        let branch = run_git(repo_dir, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(branch.trim(), "feature");
    }
}
