//! Minimal worktree management.
//!
//! Creates placeholder worktree directories for each agent. This is a
//! foundation for the full git worktree implementation.
//!
//! In multi-team mode, worktrees are created under `.swarm-hug/<team>/worktrees/`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub initial: char,
    pub name: String,
}

fn worktree_path(root: &Path, initial: char, name: &str) -> PathBuf {
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

    fs::create_dir_all(worktrees_dir)
        .map_err(|e| format!("failed to create worktrees dir: {}", e))?;

    for (initial, _task) in assignments {
        let upper = initial.to_ascii_uppercase();
        if !seen.insert(upper) {
            continue;
        }
        let name = crate::agent::name_from_initial(upper).unwrap_or("Unknown");
        let path = worktree_path(worktrees_dir, upper, name);
        fs::create_dir_all(&path)
            .map_err(|e| format!("failed to create worktree {}: {}", path.display(), e))?;
        created.push(Worktree {
            path,
            initial: upper,
            name: name.to_string(),
        });
    }

    Ok(created)
}

/// Legacy function for backwards compatibility.
/// Creates worktrees under `base/worktrees/`.
pub fn create_worktrees(
    base: &Path,
    assignments: &[(char, String)],
) -> Result<Vec<Worktree>, String> {
    create_worktrees_in(&base.join("worktrees"), assignments)
}

/// Clean up worktrees in the specified directory.
pub fn cleanup_worktrees_in(worktrees_dir: &Path) -> Result<(), String> {
    if worktrees_dir.exists() {
        fs::remove_dir_all(worktrees_dir)
            .map_err(|e| format!("failed to remove worktrees: {}", e))?;
    }
    Ok(())
}

/// Legacy function for backwards compatibility.
/// Cleans up worktrees under `base/worktrees/`.
pub fn cleanup_worktrees(base: &Path) -> Result<(), String> {
    cleanup_worktrees_in(&base.join("worktrees"))
}

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
