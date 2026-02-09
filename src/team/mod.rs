//! Team management for multi-team swarm orchestration.
//!
//! Each team operates in isolation within `.swarm-hug/<team-name>/` with:
//! - Its own specs.md, prompt.md, tasks.md
//! - Its own loop/, worktrees/ directories
//! - Its own chat.md
//! - Its own sprint-history.json for tracking sprint counts
//! - Its own team-state.json for tracking sprint feature branch

mod state;
mod runtime_state;
mod sprint_history;
#[allow(clippy::module_inception)]
mod team;

pub use runtime_state::RuntimeStatePaths;
pub use state::TeamState;
pub use sprint_history::SprintHistory;
pub use team::Team;

use std::fs;
use std::path::{Path, PathBuf};

/// Root directory for all swarm-hug configuration and artifacts.
pub const SWARM_HUG_DIR: &str = ".swarm-hug";

/// Filename for sprint history within each team directory.
pub const SPRINT_HISTORY_FILE: &str = "sprint-history.json";
/// Filename for team state within each team directory.
pub const TEAM_STATE_FILE: &str = "team-state.json";

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

        // Skip non-directories
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

    // Migration: delete assignments.toml if it exists (obsolete since project-namespaced worktrees)
    let assignments_path = root.join("assignments.toml");
    if assignments_path.exists() {
        let _ = fs::remove_file(&assignments_path);
    }

    // Always write .gitignore (managed by swarm-hug)
    let gitignore_path = root.join(".gitignore");
    let gitignore_content = "# Swarm-hug ignored files\n\
        # This file is managed by swarm-hug. Do not edit.\n\
        # These are transient/local files that shouldn't be committed\n\
        \n\
        # Agent worktrees (recreated each sprint)\n\
        */worktrees/\n\
        \n\
        # Target-branch runtime state (variation-scoped)\n\
        */runs/\n\
        \n\
        # Agent logs (local debugging)\n\
        */loop/\n\
        \n\
        # Chat logs (local coordination)\n\
        */chat.md\n";
    fs::write(&gitignore_path, gitignore_content)
        .map_err(|e| format!("failed to create .gitignore: {}", e))?;

    Ok(())
}

/// Check if the .swarm-hug directory exists.
pub fn root_exists() -> bool {
    Path::new(SWARM_HUG_DIR).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::with_temp_cwd;

    #[test]
    fn test_list_teams() {
        with_temp_cwd(|| {
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
        with_temp_cwd(|| {
            init_root().unwrap();

            let gitignore_path = PathBuf::from(SWARM_HUG_DIR).join(".gitignore");
            assert!(gitignore_path.exists(), ".gitignore should be created");

            let content = fs::read_to_string(&gitignore_path).unwrap();
            assert!(content.contains("*/worktrees/"), ".gitignore should ignore worktrees");
            assert!(content.contains("*/runs/"), ".gitignore should ignore runtime state");
            assert!(content.contains("*/loop/"), ".gitignore should ignore loop logs");
            assert!(content.contains("*/chat.md"), ".gitignore should ignore chat.md");
            assert!(content.contains("Do not edit"), ".gitignore should warn against edits");
        });
    }

    #[test]
    fn test_init_root_overwrites_existing_gitignore() {
        with_temp_cwd(|| {
            // Create .swarm-hug directory and custom .gitignore
            fs::create_dir_all(SWARM_HUG_DIR).unwrap();
            let gitignore_path = PathBuf::from(SWARM_HUG_DIR).join(".gitignore");
            let custom_content = "# Custom gitignore\n*.custom\n";
            fs::write(&gitignore_path, custom_content).unwrap();

            // init_root should overwrite existing .gitignore
            init_root().unwrap();

            let content = fs::read_to_string(&gitignore_path).unwrap();
            assert_ne!(content, custom_content, "existing .gitignore should be overwritten");
            assert!(content.contains("*/worktrees/"), ".gitignore should contain swarm-hug defaults");
            assert!(content.contains("*/runs/"), ".gitignore should contain runtime-state ignore");
        });
    }

    #[test]
    fn test_init_root_deletes_assignments_toml() {
        with_temp_cwd(|| {
            // Create .swarm-hug directory with legacy assignments.toml
            fs::create_dir_all(SWARM_HUG_DIR).unwrap();
            let assignments_path = PathBuf::from(SWARM_HUG_DIR).join("assignments.toml");
            fs::write(&assignments_path, "[agents]\nA = \"test\"\n").unwrap();
            assert!(assignments_path.exists(), "assignments.toml should exist before init");

            // init_root should delete assignments.toml (migration)
            init_root().unwrap();

            assert!(!assignments_path.exists(), "assignments.toml should be deleted by init_root");
        });
    }
}
