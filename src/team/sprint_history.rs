use std::fs;
use std::path::PathBuf;

use super::{SPRINT_HISTORY_FILE, SWARM_HUG_DIR};

/// Sprint history tracking for a team.
///
/// Tracks the total number of sprints run for a team, persisted to
/// `.swarm-hug/<team>/sprint-history.json`. This enables commit messages
/// like "TeamName Sprint 42: task assignments" that reflect historical context.
#[derive(Debug, Clone)]
pub struct SprintHistory {
    /// Team name.
    pub team_name: String,
    /// Total sprints completed (ever) for this team.
    pub total_sprints: usize,
    /// Path to the sprint history file.
    path: PathBuf,
}

impl SprintHistory {
    /// Load sprint history for a team.
    ///
    /// Creates a new history with 0 sprints if the file doesn't exist.
    pub fn load(team_name: &str) -> Result<Self, String> {
        let path = PathBuf::from(SWARM_HUG_DIR)
            .join(team_name)
            .join(SPRINT_HISTORY_FILE);

        let total_sprints = if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
            Self::parse_json(&content)?
        } else {
            0
        };

        Ok(Self {
            team_name: team_name.to_string(),
            total_sprints,
            path,
        })
    }

    /// Parse the total_sprints from JSON content.
    fn parse_json(content: &str) -> Result<usize, String> {
        // Simple JSON parsing for {"total_sprints": N}
        // We avoid pulling in serde_json for this simple case
        let content = content.trim();
        if !content.starts_with('{') || !content.ends_with('}') {
            return Err("invalid sprint history JSON".to_string());
        }

        // Find "total_sprints": N
        if let Some(idx) = content.find("\"total_sprints\"") {
            let after_key = &content[idx + 15..]; // Skip past "total_sprints"
            if let Some(colon_idx) = after_key.find(':') {
                let after_colon = after_key[colon_idx + 1..].trim();
                // Extract the number (ends at comma, brace, or whitespace)
                let num_str: String = after_colon
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if !num_str.is_empty() {
                    return num_str
                        .parse()
                        .map_err(|_| "invalid total_sprints value".to_string());
                }
            }
        }

        Err("missing total_sprints in sprint history".to_string())
    }

    /// Increment the sprint count and return the new sprint number.
    ///
    /// This should be called at the START of a sprint. The returned value
    /// is the sprint number to use for this sprint's commits.
    pub fn next_sprint(&mut self) -> usize {
        self.total_sprints += 1;
        self.total_sprints
    }

    /// Save the sprint history to disk.
    pub fn save(&self) -> Result<(), String> {
        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directory: {}", e))?;
        }

        let content = self.to_json();
        fs::write(&self.path, content)
            .map_err(|e| format!("failed to write {}: {}", self.path.display(), e))?;

        Ok(())
    }

    /// Convert to JSON string.
    fn to_json(&self) -> String {
        format!(
            "{{\n  \"team\": \"{}\",\n  \"total_sprints\": {}\n}}\n",
            self.team_name, self.total_sprints
        )
    }

    /// Get the formatted team name for commit messages.
    ///
    /// Converts team-name to "Team Name" (title case with spaces).
    pub fn formatted_team_name(&self) -> String {
        self.team_name
            .split(['-', '_'])
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            })
            .collect::<Vec<String>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Team;
    use crate::testutil::with_temp_cwd;

    #[test]
    fn test_sprint_history_load_new() {
        with_temp_cwd(|| {
            // Load from non-existent file should create new history with 0 sprints
            let history = SprintHistory::load("new-team").unwrap();
            assert_eq!(history.team_name, "new-team");
            assert_eq!(history.total_sprints, 0);
        });
    }

    #[test]
    fn test_sprint_history_next_sprint() {
        with_temp_cwd(|| {
            let mut history = SprintHistory::load("test-team").unwrap();
            assert_eq!(history.total_sprints, 0);

            // First sprint
            let sprint1 = history.next_sprint();
            assert_eq!(sprint1, 1);
            assert_eq!(history.total_sprints, 1);

            // Second sprint
            let sprint2 = history.next_sprint();
            assert_eq!(sprint2, 2);
            assert_eq!(history.total_sprints, 2);
        });
    }

    #[test]
    fn test_sprint_history_save_and_load() {
        with_temp_cwd(|| {
            // Initialize team directory
            Team::new("persistent-team").init().unwrap();

            // Create and save history
            let mut history = SprintHistory::load("persistent-team").unwrap();
            history.next_sprint();
            history.next_sprint();
            history.next_sprint();
            history.save().unwrap();

            // Load again and verify
            let loaded = SprintHistory::load("persistent-team").unwrap();
            assert_eq!(loaded.total_sprints, 3);
        });
    }

    #[test]
    fn test_sprint_history_parse_json() {
        // Valid JSON
        let result = SprintHistory::parse_json(r#"{"team": "test", "total_sprints": 42}"#);
        assert_eq!(result.unwrap(), 42);

        // With whitespace
        let result = SprintHistory::parse_json(r#"{
            "team": "test",
            "total_sprints": 100
        }"#);
        assert_eq!(result.unwrap(), 100);

        // Invalid JSON (not an object)
        let result = SprintHistory::parse_json("not json");
        assert!(result.is_err());

        // Missing total_sprints
        let result = SprintHistory::parse_json(r#"{"team": "test"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_sprint_history_to_json() {
        with_temp_cwd(|| {
            let mut history = SprintHistory::load("json-team").unwrap();
            history.next_sprint();
            history.next_sprint();

            let json = history.to_json();
            assert!(json.contains("\"team\": \"json-team\""));
            assert!(json.contains("\"total_sprints\": 2"));
        });
    }

    #[test]
    fn test_sprint_history_formatted_team_name() {
        with_temp_cwd(|| {
            // Hyphenated name
            let history = SprintHistory::load("my-awesome-team").unwrap();
            assert_eq!(history.formatted_team_name(), "My Awesome Team");

            // Underscored name
            let history = SprintHistory::load("another_team_name").unwrap();
            assert_eq!(history.formatted_team_name(), "Another Team Name");

            // Simple name
            let history = SprintHistory::load("greenfield").unwrap();
            assert_eq!(history.formatted_team_name(), "Greenfield");

            // Mixed separators
            let history = SprintHistory::load("api-v2_backend").unwrap();
            assert_eq!(history.formatted_team_name(), "Api V2 Backend");
        });
    }

    #[test]
    fn test_sprint_history_persistence_across_sessions() {
        with_temp_cwd(|| {
            // Initialize team
            Team::new("session-team").init().unwrap();

            // Session 1: Run 5 sprints
            {
                let mut history = SprintHistory::load("session-team").unwrap();
                for _ in 0..5 {
                    history.next_sprint();
                }
                history.save().unwrap();
            }

            // Session 2: Run 3 more sprints
            {
                let mut history = SprintHistory::load("session-team").unwrap();
                assert_eq!(history.total_sprints, 5, "Should persist from session 1");

                let sprint6 = history.next_sprint();
                assert_eq!(sprint6, 6, "Should continue from where we left off");

                history.next_sprint();
                history.next_sprint();
                history.save().unwrap();
            }

            // Session 3: Verify final count
            {
                let history = SprintHistory::load("session-team").unwrap();
                assert_eq!(history.total_sprints, 8);
            }
        });
    }
}
