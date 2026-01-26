use std::fs;
use std::path::Path;
use std::process::Command;

use super::Worktree;

/// List worktrees in the specified directory.
pub fn list_worktrees(worktrees_dir: &Path) -> Result<Vec<Worktree>, String> {
    let mut worktrees = Vec::new();

    if !worktrees_dir.exists() {
        return Ok(worktrees);
    }

    let entries = fs::read_dir(worktrees_dir)
        .map_err(|e| format!("failed to read worktrees dir: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Parse directory name: agent-<initial>-<name>
        let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if let Some(rest) = dir_name.strip_prefix("agent-") {
            let parts: Vec<&str> = rest.splitn(2, '-').collect();
            if parts.len() == 2 {
                if let Some(initial) = parts[0].chars().next() {
                    worktrees.push(Worktree {
                        path,
                        initial: initial.to_ascii_uppercase(),
                        name: parts[1].to_string(),
                    });
                }
            }
        }
    }

    worktrees.sort_by(|a, b| a.initial.cmp(&b.initial));
    Ok(worktrees)
}

/// Agent branch info.
#[derive(Debug, Clone)]
pub struct AgentBranch {
    pub initial: char,
    pub name: String,
    pub branch: String,
    pub exists: bool,
}

/// List agent branches in the repository.
/// Returns branches matching the pattern `agent/<name>`.
pub fn list_agent_branches() -> Result<Vec<AgentBranch>, String> {
    let output = Command::new("git")
        .args(["branch", "--list", "agent/*"])
        .output()
        .map_err(|e| format!("failed to run git branch: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git branch failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut branches = Vec::new();

    for line in stdout.lines() {
        let branch = line.trim().trim_start_matches("* ");
        if let Some(agent_name) = branch.strip_prefix("agent/") {
            // Find the initial for this agent name (may be None for non-standard branches)
            let initial = crate::agent::initial_from_name(agent_name).unwrap_or('?');
            branches.push(AgentBranch {
                initial,
                name: agent_name.to_string(),
                branch: branch.to_string(),
                exists: true,
            });
        }
    }

    branches.sort_by(|a, b| a.initial.cmp(&b.initial));
    Ok(branches)
}
