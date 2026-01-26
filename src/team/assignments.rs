use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::{ASSIGNMENTS_FILE, SWARM_HUG_DIR};

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

    /// Get the next N agents available to a specific team.
    /// Includes unassigned agents and agents already assigned to the team.
    pub fn available_for_team(&self, team: &str, count: usize) -> Vec<char> {
        crate::agent::INITIALS
            .iter()
            .filter(|&&i| {
                self.is_available(i)
                    || self.get_team(i).map(|t| t == team).unwrap_or(false)
            })
            .take(count)
            .copied()
            .collect()
    }

    // Project-based aliases (same as team methods)

    /// Get the project an agent is assigned to (alias for get_team).
    pub fn get_project(&self, initial: char) -> Option<&str> {
        self.get_team(initial)
    }

    /// Release all agents assigned to a specific project (alias for release_team).
    pub fn release_project(&mut self, project: &str) {
        self.release_team(project)
    }

    /// Get all agents assigned to a specific project (alias for team_agents).
    pub fn project_agents(&self, project: &str) -> Vec<char> {
        self.team_agents(project)
    }

    /// Get the next N agents available to a specific project (alias for available_for_team).
    pub fn available_for_project(&self, project: &str, count: usize) -> Vec<char> {
        self.available_for_team(project, count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_available_for_team_includes_team_and_unassigned() {
        let mut assignments = Assignments::default();
        assignments.assign('A', "auth").unwrap();
        assignments.assign('B', "payments").unwrap();

        let available = assignments.available_for_team("auth", 4);
        assert_eq!(available, vec!['A', 'C', 'D', 'E']);
    }
}
