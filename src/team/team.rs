use std::fs;
use std::path::PathBuf;

use super::{SPRINT_HISTORY_FILE, SWARM_HUG_DIR};

/// A team's configuration and paths.
#[derive(Debug, Clone)]
pub struct Team {
    /// Team name (e.g., "authentication", "payments").
    pub name: String,
    /// Root path for this team's artifacts.
    pub root: PathBuf,
}

impl Team {
    /// Create a new team with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            root: PathBuf::from(SWARM_HUG_DIR).join(name),
        }
    }

    /// Path to team's tasks.md file.
    pub fn tasks_path(&self) -> PathBuf {
        self.root.join("tasks.md")
    }

    /// Path to team's chat.md file.
    pub fn chat_path(&self) -> PathBuf {
        self.root.join("chat.md")
    }

    /// Path to team's specs.md file.
    pub fn specs_path(&self) -> PathBuf {
        self.root.join("specs.md")
    }

    /// Path to team's prompt.md file.
    pub fn prompt_path(&self) -> PathBuf {
        self.root.join("prompt.md")
    }

    /// Path to team's loop/ directory.
    pub fn loop_dir(&self) -> PathBuf {
        self.root.join("loop")
    }

    /// Path to team's worktrees/ directory.
    pub fn worktrees_dir(&self) -> PathBuf {
        self.root.join("worktrees")
    }

    /// Path to team's sprint-history.json file.
    pub fn sprint_history_path(&self) -> PathBuf {
        self.root.join(SPRINT_HISTORY_FILE)
    }

    /// Check if this team exists (has been initialized).
    pub fn exists(&self) -> bool {
        self.root.exists()
    }

    /// Initialize this team's directory structure.
    pub fn init(&self) -> Result<(), String> {
        // Create root directory
        fs::create_dir_all(&self.root)
            .map_err(|e| format!("failed to create team directory {}: {}", self.root.display(), e))?;

        // Create subdirectories
        fs::create_dir_all(self.loop_dir())
            .map_err(|e| format!("failed to create loop dir: {}", e))?;
        fs::create_dir_all(self.worktrees_dir())
            .map_err(|e| format!("failed to create worktrees dir: {}", e))?;

        // Create default files if they don't exist
        if !self.tasks_path().exists() {
            let default_tasks = "# Tasks\n\n- [ ] Add your tasks here\n";
            fs::write(self.tasks_path(), default_tasks)
                .map_err(|e| format!("failed to create tasks.md: {}", e))?;
        }

        if !self.chat_path().exists() {
            fs::write(self.chat_path(), "")
                .map_err(|e| format!("failed to create chat.md: {}", e))?;
        }

        if !self.specs_path().exists() {
            let default_specs = format!("# Specifications: {}\n\nAdd your specifications here.\n", self.name);
            fs::write(self.specs_path(), default_specs)
                .map_err(|e| format!("failed to create specs.md: {}", e))?;
        }

        if !self.prompt_path().exists() {
            let default_prompt = format!("# Prompt: {}\n\nDescribe what this team should accomplish.\n", self.name);
            fs::write(self.prompt_path(), default_prompt)
                .map_err(|e| format!("failed to create prompt.md: {}", e))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_team_paths() {
        let team = Team::new("authentication");
        assert_eq!(team.name, "authentication");
        assert_eq!(team.root, PathBuf::from(".swarm-hug/authentication"));
        assert_eq!(team.tasks_path(), PathBuf::from(".swarm-hug/authentication/tasks.md"));
        assert_eq!(team.chat_path(), PathBuf::from(".swarm-hug/authentication/chat.md"));
        assert_eq!(team.loop_dir(), PathBuf::from(".swarm-hug/authentication/loop"));
        assert_eq!(team.worktrees_dir(), PathBuf::from(".swarm-hug/authentication/worktrees"));
    }

    #[test]
    fn test_team_init() {
        super::super::with_temp_dir(|| {
            let team = Team::new("payments");
            team.init().unwrap();

            assert!(team.root.exists());
            assert!(team.tasks_path().exists());
            assert!(team.chat_path().exists());
            assert!(team.specs_path().exists());
            assert!(team.prompt_path().exists());
            assert!(team.loop_dir().exists());
            assert!(team.worktrees_dir().exists());
        });
    }

    #[test]
    fn test_team_sprint_history_path() {
        let team = Team::new("myteam");
        assert_eq!(
            team.sprint_history_path(),
            PathBuf::from(".swarm-hug/myteam/sprint-history.json")
        );
    }
}
