use std::path::Path;
use std::process;

use swarm::team;

pub(crate) fn commit_files(paths: &[&str], message: &str) -> Result<bool, String> {
    let existing: Vec<&str> = paths
        .iter()
        .copied()
        .filter(|p| !p.is_empty() && Path::new(p).exists())
        .collect();

    if existing.is_empty() {
        return Ok(false);
    }

    let mut add_args = vec!["add"];
    add_args.extend(existing);
    let add_result = process::Command::new("git").args(add_args).output();

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

/// Commit task assignment changes to git.
///
/// # Arguments
/// * `tasks_file` - Path to the team's tasks.md file
/// * `sprint_history_file` - Path to the team's sprint-history.json file
/// * `team_name` - Formatted team name for commit message (e.g., "Greenfield")
/// * `sprint_number` - The historical sprint number for this team
pub(crate) fn commit_task_assignments(
    tasks_file: &str,
    sprint_history_file: &str,
    team_name: &str,
    sprint_number: usize,
) -> Result<(), String> {
    let assignments_path = format!("{}/{}", team::SWARM_HUG_DIR, team::ASSIGNMENTS_FILE);
    let commit_msg = format!("{} Sprint {}: task assignments", team_name, sprint_number);
    if commit_files(
        &[tasks_file, sprint_history_file, assignments_path.as_str()],
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
    tasks_file: &str,
    team_name: &str,
    sprint_number: usize,
) -> Result<(), String> {
    let assignments_path = format!("{}/{}", team::SWARM_HUG_DIR, team::ASSIGNMENTS_FILE);
    let commit_msg = format!("{} Sprint {}: completed", team_name, sprint_number);
    if commit_files(&[tasks_file, assignments_path.as_str()], &commit_msg)? {
        println!("  Committed sprint completion to git.");
    }
    Ok(())
}

/// Get the current git commit hash.
pub(crate) fn get_current_commit() -> Option<String> {
    let output = process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get git log between two commits (messages and stats, no diffs).
pub(crate) fn get_git_log_range(from: &str, to: &str) -> Result<String, String> {
    let range = format!("{}..{}", from, to);
    let output = process::Command::new("git")
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
