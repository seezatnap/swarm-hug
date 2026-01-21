//! Minimal worktree management.
//!
//! Creates placeholder worktree directories for each agent. This is a
//! foundation for the full git worktree implementation.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub initial: char,
    pub name: String,
}

fn worktrees_root(base: &Path) -> PathBuf {
    base.join("worktrees")
}

fn worktree_path(base: &Path, initial: char, name: &str) -> PathBuf {
    worktrees_root(base).join(format!("agent-{}-{}", initial, name))
}

pub fn create_worktrees(
    base: &Path,
    assignments: &[(char, String)],
) -> Result<Vec<Worktree>, String> {
    let mut created = Vec::new();
    let mut seen = HashSet::new();

    if assignments.is_empty() {
        return Ok(created);
    }

    let root = worktrees_root(base);
    fs::create_dir_all(&root)
        .map_err(|e| format!("failed to create worktrees root: {}", e))?;

    for (initial, _task) in assignments {
        let upper = initial.to_ascii_uppercase();
        if !seen.insert(upper) {
            continue;
        }
        let name = crate::agent::name_from_initial(upper).unwrap_or("Unknown");
        let path = worktree_path(base, upper, name);
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

pub fn cleanup_worktrees(base: &Path) -> Result<(), String> {
    let root = worktrees_root(base);
    if root.exists() {
        fs::remove_dir_all(&root)
            .map_err(|e| format!("failed to remove worktrees: {}", e))?;
    }
    Ok(())
}
