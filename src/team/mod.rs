//! Team management for multi-team swarm orchestration.
//!
//! Each team operates in isolation within `.swarm-hug/<team-name>/` with:
//! - Its own specs.md, prompt.md, tasks.md
//! - Its own loop/, worktrees/ directories
//! - Its own chat.md
//! - Its own sprint-history.json for tracking sprint counts
//!
//! Agent assignments are tracked in `.swarm-hug/assignments.toml` to ensure
//! no agent works on multiple teams simultaneously.

mod assignments;
mod sprint_history;
mod team;

pub use assignments::Assignments;
pub use sprint_history::SprintHistory;
pub use team::Team;

use std::fs;
use std::path::{Path, PathBuf};

/// Root directory for all swarm-hug configuration and artifacts.
pub const SWARM_HUG_DIR: &str = ".swarm-hug";

/// Filename for agent-to-team assignments.
pub const ASSIGNMENTS_FILE: &str = "assignments.toml";

/// Filename for sprint history within each team directory.
pub const SPRINT_HISTORY_FILE: &str = "sprint-history.json";

#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
use tempfile::TempDir;

#[cfg(test)]
static CWD_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
fn with_temp_dir<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = CWD_LOCK.lock().unwrap();
    let original = std::env::current_dir().unwrap();
    let temp = TempDir::new().unwrap();
    std::env::set_current_dir(temp.path()).unwrap();
    let result = f();
    std::env::set_current_dir(original).unwrap();
    result
}

/// List all teams in the .swarm-hug directory.
pub fn list_teams() -> Result<Vec<Team>, String> {
    let root = PathBuf::from(SWARM_HUG_DIR);
    if !root.exists() {
        return Ok(vec![]);
    }

    let mut teams = Vec::new();
    let entries = fs::read_dir(&root)
        .map_err(|e| format!("failed to read {}: {}", root.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
        let path = entry.path();

        // Skip non-directories and the assignments file
        if !path.is_dir() {
            continue;
        }

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            teams.push(Team::new(name));
        }
    }

    // Sort alphabetically
    teams.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(teams)
}

/// Initialize the .swarm-hug root directory.
pub fn init_root() -> Result<(), String> {
    let root = PathBuf::from(SWARM_HUG_DIR);
    fs::create_dir_all(&root)
        .map_err(|e| format!("failed to create {}: {}", root.display(), e))?;

    // Create empty assignments file if it doesn't exist
    let assignments_path = root.join(ASSIGNMENTS_FILE);
    if !assignments_path.exists() {
        Assignments::default().save()?;
    }

    // Create .gitignore if it doesn't exist
    let gitignore_path = root.join(".gitignore");
    if !gitignore_path.exists() {
        let gitignore_content = "# Swarm-hug ignored files\n\
            # These are transient/local files that shouldn't be committed\n\
            \n\
            # Agent worktrees (recreated each sprint)\n\
            */worktrees/\n\
            \n\
            # Agent logs (local debugging)\n\
            */loop/\n\
            \n\
            # Chat logs (local coordination)\n\
            */chat.md\n";
        fs::write(&gitignore_path, gitignore_content)
            .map_err(|e| format!("failed to create .gitignore: {}", e))?;
    }

    Ok(())
}

/// Check if the .swarm-hug directory exists.
pub fn root_exists() -> bool {
    Path::new(SWARM_HUG_DIR).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_teams() {
        super::with_temp_dir(|| {
            init_root().unwrap();

            Team::new("authentication").init().unwrap();
            Team::new("payments").init().unwrap();

            let teams = list_teams().unwrap();
            assert_eq!(teams.len(), 2);
            assert_eq!(teams[0].name, "authentication");
            assert_eq!(teams[1].name, "payments");
        });
    }

    #[test]
    fn test_init_root_creates_gitignore() {
        super::with_temp_dir(|| {
            init_root().unwrap();

            let gitignore_path = PathBuf::from(SWARM_HUG_DIR).join(".gitignore");
            assert!(gitignore_path.exists(), ".gitignore should be created");

            let content = fs::read_to_string(&gitignore_path).unwrap();
            assert!(content.contains("*/worktrees/"), ".gitignore should ignore worktrees");
            assert!(content.contains("*/loop/"), ".gitignore should ignore loop logs");
            assert!(content.contains("*/chat.md"), ".gitignore should ignore chat.md");
        });
    }

    #[test]
    fn test_init_root_preserves_existing_gitignore() {
        super::with_temp_dir(|| {
            // Create .swarm-hug directory and custom .gitignore
            fs::create_dir_all(SWARM_HUG_DIR).unwrap();
            let gitignore_path = PathBuf::from(SWARM_HUG_DIR).join(".gitignore");
            let custom_content = "# Custom gitignore\n*.custom\n";
            fs::write(&gitignore_path, custom_content).unwrap();

            // init_root should not overwrite existing .gitignore
            init_root().unwrap();

            let content = fs::read_to_string(&gitignore_path).unwrap();
            assert_eq!(content, custom_content, "existing .gitignore should be preserved");
        });
    }
}
