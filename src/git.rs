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

/// Commit task assignment changes to git.
///
/// # Arguments
/// * `tasks_file` - Path to the team's tasks.md file
/// * `sprint_history_file` - Path to the team's sprint-history.json file
/// * `team_name` - Formatted team name for commit message (e.g., "Greenfield")
/// * `sprint_number` - The historical sprint number for this team
pub(crate) fn commit_task_assignments(
    worktree_root: &Path,
    tasks_file: &str,
    sprint_history_file: &str,
    team_state_file: &str,
    team_name: &str,
    sprint_number: usize,
) -> Result<(), String> {
    let assignments_path = format!("{}/{}", team::SWARM_HUG_DIR, team::ASSIGNMENTS_FILE);
    let commit_msg = format!("{} Sprint {}: task assignments", team_name, sprint_number);
    if commit_files_in_worktree(
        worktree_root,
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
/// * `tasks_file` - Path to the team's tasks.md file
/// * `team_name` - Formatted team name for commit message (e.g., "Greenfield")
/// * `sprint_number` - The historical sprint number for this team
pub(crate) fn commit_sprint_completion(
    worktree_root: &Path,
    tasks_file: &str,
    team_name: &str,
    sprint_number: usize,
) -> Result<(), String> {
    let assignments_path = format!("{}/{}", team::SWARM_HUG_DIR, team::ASSIGNMENTS_FILE);
    let commit_msg = format!("{} Sprint {}: completed", team_name, sprint_number);
    if commit_files_in_worktree(
        worktree_root,
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
