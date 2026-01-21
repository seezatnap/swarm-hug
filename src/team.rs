//! Team management for multi-team swarm orchestration.
//!
//! Each team operates in isolation within `.swarm-hug/<team-name>/` with:
//! - Its own specs.md, prompt.md, tasks.md
//! - Its own loop/, worktrees/ directories
//! - Its own chat.md
//!
//! Agent assignments are tracked in `.swarm-hug/assignments.toml` to ensure
//! no agent works on multiple teams simultaneously.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Root directory for all swarm-hug configuration and artifacts.
pub const SWARM_HUG_DIR: &str = ".swarm-hug";

/// Filename for agent-to-team assignments.
pub const ASSIGNMENTS_FILE: &str = "assignments.toml";

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

/// Agent assignment tracking.
/// Maps agent initials to team names, ensuring exclusive assignment.
#[derive(Debug, Clone, Default)]
pub struct Assignments {
    /// Map of agent initial (uppercase) to team name.
    agent_to_team: HashMap<char, String>,
}

impl Assignments {
    /// Load assignments from the assignments.toml file.
    pub fn load() -> Result<Self, String> {
        let path = PathBuf::from(SWARM_HUG_DIR).join(ASSIGNMENTS_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;

        Self::parse(&content)
    }

    /// Parse assignments from TOML content.
    fn parse(content: &str) -> Result<Self, String> {
        let mut assignments = Self::default();
        let mut in_agents_section = false;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line == "[agents]" {
                in_agents_section = true;
                continue;
            } else if line.starts_with('[') {
                in_agents_section = false;
                continue;
            }

            if in_agents_section {
                // Parse lines like: A = "authentication"
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim().trim_matches('"');
                    if key.len() == 1 {
                        if let Some(initial) = key.chars().next() {
                            assignments.agent_to_team.insert(initial.to_ascii_uppercase(), value.to_string());
                        }
                    }
                }
            }
        }

        Ok(assignments)
    }

    /// Save assignments to the assignments.toml file.
    pub fn save(&self) -> Result<(), String> {
        let path = PathBuf::from(SWARM_HUG_DIR).join(ASSIGNMENTS_FILE);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directory: {}", e))?;
        }

        let content = self.to_toml();
        fs::write(&path, content)
            .map_err(|e| format!("failed to write {}: {}", path.display(), e))?;

        Ok(())
    }

    /// Convert assignments to TOML string.
    fn to_toml(&self) -> String {
        let mut lines = vec![
            "# Agent Assignments".to_string(),
            "# Maps agent initials to team names.".to_string(),
            "# An agent can only be assigned to one team at a time.".to_string(),
            "".to_string(),
            "[agents]".to_string(),
        ];

        // Sort by initial for consistent output
        let mut entries: Vec<_> = self.agent_to_team.iter().collect();
        entries.sort_by_key(|(k, _)| *k);

        for (initial, team) in entries {
            lines.push(format!("{} = \"{}\"", initial, team));
        }

        lines.join("\n") + "\n"
    }

    /// Get the team an agent is assigned to.
    pub fn get_team(&self, initial: char) -> Option<&str> {
        self.agent_to_team.get(&initial.to_ascii_uppercase()).map(|s| s.as_str())
    }

    /// Check if an agent is available (not assigned to any team).
    pub fn is_available(&self, initial: char) -> bool {
        !self.agent_to_team.contains_key(&initial.to_ascii_uppercase())
    }

    /// Assign an agent to a team.
    /// Returns an error if the agent is already assigned to a different team.
    pub fn assign(&mut self, initial: char, team: &str) -> Result<(), String> {
        let initial = initial.to_ascii_uppercase();
        if let Some(existing) = self.agent_to_team.get(&initial) {
            if existing != team {
                return Err(format!(
                    "Agent {} is already assigned to team '{}', cannot assign to '{}'",
                    initial, existing, team
                ));
            }
            // Already assigned to this team, no-op
            return Ok(());
        }

        self.agent_to_team.insert(initial, team.to_string());
        Ok(())
    }

    /// Release an agent from their team assignment.
    pub fn release(&mut self, initial: char) {
        self.agent_to_team.remove(&initial.to_ascii_uppercase());
    }

    /// Release all agents assigned to a specific team.
    pub fn release_team(&mut self, team: &str) {
        self.agent_to_team.retain(|_, t| t != team);
    }

    /// Get all agents assigned to a specific team.
    pub fn team_agents(&self, team: &str) -> Vec<char> {
        self.agent_to_team
            .iter()
            .filter(|(_, t)| t.as_str() == team)
            .map(|(i, _)| *i)
            .collect()
    }

    /// Get the next N available agents (in alphabetical order).
    pub fn next_available(&self, count: usize) -> Vec<char> {
        crate::agent::INITIALS
            .iter()
            .filter(|&&i| self.is_available(i))
            .take(count)
            .copied()
            .collect()
    }
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

    Ok(())
}

/// Check if the .swarm-hug directory exists.
pub fn root_exists() -> bool {
    Path::new(SWARM_HUG_DIR).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

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
        with_temp_dir(|| {
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
    fn test_assignments_parse() {
        let content = r#"
# Agent Assignments

[agents]
A = "authentication"
B = "payments"
C = "authentication"
"#;
        let assignments = Assignments::parse(content).unwrap();
        assert_eq!(assignments.get_team('A'), Some("authentication"));
        assert_eq!(assignments.get_team('B'), Some("payments"));
        assert_eq!(assignments.get_team('C'), Some("authentication"));
        assert_eq!(assignments.get_team('D'), None);
    }

    #[test]
    fn test_assignments_to_toml() {
        let mut assignments = Assignments::default();
        assignments.assign('A', "auth").unwrap();
        assignments.assign('B', "payments").unwrap();

        let toml = assignments.to_toml();
        assert!(toml.contains("A = \"auth\""));
        assert!(toml.contains("B = \"payments\""));
    }

    #[test]
    fn test_assignments_exclusive() {
        let mut assignments = Assignments::default();
        assignments.assign('A', "auth").unwrap();

        // Same team is OK
        assert!(assignments.assign('A', "auth").is_ok());

        // Different team is error
        assert!(assignments.assign('A', "payments").is_err());
    }

    #[test]
    fn test_assignments_release() {
        let mut assignments = Assignments::default();
        assignments.assign('A', "auth").unwrap();
        assignments.assign('B', "auth").unwrap();
        assignments.assign('C', "payments").unwrap();

        assignments.release('A');
        assert!(assignments.is_available('A'));
        assert!(!assignments.is_available('B'));

        assignments.release_team("auth");
        assert!(assignments.is_available('B'));
        assert!(!assignments.is_available('C'));
    }

    #[test]
    fn test_team_agents() {
        let mut assignments = Assignments::default();
        assignments.assign('A', "auth").unwrap();
        assignments.assign('B', "auth").unwrap();
        assignments.assign('C', "payments").unwrap();

        let mut auth_agents = assignments.team_agents("auth");
        auth_agents.sort();
        assert_eq!(auth_agents, vec!['A', 'B']);
    }

    #[test]
    fn test_next_available() {
        let mut assignments = Assignments::default();
        assignments.assign('A', "auth").unwrap();
        assignments.assign('C', "payments").unwrap();

        let available = assignments.next_available(3);
        assert_eq!(available, vec!['B', 'D', 'E']);
    }

    #[test]
    fn test_list_teams() {
        with_temp_dir(|| {
            init_root().unwrap();

            Team::new("authentication").init().unwrap();
            Team::new("payments").init().unwrap();

            let teams = list_teams().unwrap();
            assert_eq!(teams.len(), 2);
            assert_eq!(teams[0].name, "authentication");
            assert_eq!(teams[1].name, "payments");
        });
    }
}
