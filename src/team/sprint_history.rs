use std::fs;
use std::path::{Path, PathBuf};

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

    /// Load sprint history from an explicit path.
    ///
    /// This method loads sprint history from a specified path instead of
    /// deriving it from the team name. Useful when loading from a worktree
    /// or non-standard location.
    ///
    /// Creates a new history with 0 sprints if the file doesn't exist,
    /// supporting the first-sprint case where no history file exists yet.
    ///
    /// The team name is extracted from the JSON content if the file exists,
    /// otherwise it defaults to "unknown" (callers should set it if needed
    /// before saving).
    pub fn load_from(path: &Path) -> Result<Self, String> {
        let (total_sprints, team_name) = if path.exists() {
            let content = fs::read_to_string(path)
                .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
            let sprints = Self::parse_json(&content)?;
            let team = Self::parse_team_name(&content).unwrap_or_else(|| "unknown".to_string());
            (sprints, team)
        } else {
            (0, "unknown".to_string())
        };

        Ok(Self {
            team_name,
            total_sprints,
            path: path.to_path_buf(),
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

        // "total_sprints" is canonical. "sprint_count" and "sprint" are legacy aliases.
        for key in ["\"total_sprints\"", "\"sprint_count\"", "\"sprint\""] {
            if let Some(idx) = content.find(key) {
                let after_key = &content[idx + key.len()..];
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
                return Err("invalid total_sprints value".to_string());
            }
        }

        Err("missing total_sprints in sprint history".to_string())
    }

    /// Parse the team name from JSON content.
    ///
    /// Returns None if the team field is not found or cannot be parsed.
    fn parse_team_name(content: &str) -> Option<String> {
        let content = content.trim();

        // Find "team": "value"
        if let Some(idx) = content.find("\"team\"") {
            let after_key = &content[idx + 6..]; // Skip past "team"
            if let Some(colon_idx) = after_key.find(':') {
                let after_colon = after_key[colon_idx + 1..].trim_start();
                // Parse the string value
                if let Some(stripped) = after_colon.strip_prefix('"') {
                    let mut chars = stripped.chars();
                    let mut result = String::new();
                    let mut escaped = false;
                    for ch in chars.by_ref() {
                        if escaped {
                            let decoded = match ch {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                '\\' => '\\',
                                '"' => '"',
                                other => other,
                            };
                            result.push(decoded);
                            escaped = false;
                            continue;
                        }
                        if ch == '\\' {
                            escaped = true;
                            continue;
                        }
                        if ch == '"' {
                            return Some(result);
                        }
                        result.push(ch);
                    }
                }
            }
        }

        None
    }

    /// Peek at the next sprint number without mutating state.
    ///
    /// Returns `total_sprints + 1`, which is the sprint number that would be
    /// assigned if `increment()` were called. Use this to determine the sprint
    /// branch name before any files are written.
    pub fn peek_next_sprint(&self) -> usize {
        self.total_sprints + 1
    }

    /// Increment the sprint count.
    ///
    /// This should be called after the sprint branch is created but before
    /// saving the history. Separates mutation from `peek_next_sprint()` to
    /// allow determining the sprint number before committing to it.
    pub fn increment(&mut self) {
        self.total_sprints += 1;
    }

    /// Increment the sprint count and return the new sprint number.
    ///
    /// This should be called at the START of a sprint. The returned value
    /// is the sprint number to use for this sprint's commits.
    ///
    /// Note: This is a convenience method that combines `increment()` and
    /// returns the result. For the new sprint initialization flow, prefer
    /// using `peek_next_sprint()` followed by `increment()` separately.
    pub fn next_sprint(&mut self) -> usize {
        self.increment();
        self.total_sprints
    }

    /// Save the sprint history to disk.
    pub fn save(&self) -> Result<(), String> {
        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("failed to create directory: {}", e))?;
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
    use super::super::Team;
    use super::*;
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
        let result = SprintHistory::parse_json(
            r#"{
            "team": "test",
            "total_sprints": 100
        }"#,
        );
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

    #[test]
    fn test_peek_next_sprint_does_not_mutate() {
        with_temp_cwd(|| {
            let history = SprintHistory::load("peek-team").unwrap();
            assert_eq!(history.total_sprints, 0);

            // Peek should return 1 (next sprint number)
            assert_eq!(history.peek_next_sprint(), 1);
            // State should remain unchanged
            assert_eq!(history.total_sprints, 0);

            // Peek again - same result, still no mutation
            assert_eq!(history.peek_next_sprint(), 1);
            assert_eq!(history.total_sprints, 0);
        });
    }

    #[test]
    fn test_peek_next_sprint_after_increments() {
        with_temp_cwd(|| {
            let mut history = SprintHistory::load("peek-increment-team").unwrap();

            // Initially at 0, peek shows 1
            assert_eq!(history.peek_next_sprint(), 1);

            // After increment, peek shows 2
            history.increment();
            assert_eq!(history.total_sprints, 1);
            assert_eq!(history.peek_next_sprint(), 2);

            // After another increment, peek shows 3
            history.increment();
            assert_eq!(history.total_sprints, 2);
            assert_eq!(history.peek_next_sprint(), 3);
        });
    }

    #[test]
    fn test_increment_mutates_state() {
        with_temp_cwd(|| {
            let mut history = SprintHistory::load("increment-team").unwrap();
            assert_eq!(history.total_sprints, 0);

            history.increment();
            assert_eq!(history.total_sprints, 1);

            history.increment();
            assert_eq!(history.total_sprints, 2);

            history.increment();
            assert_eq!(history.total_sprints, 3);
        });
    }

    #[test]
    fn test_peek_and_increment_equivalent_to_next_sprint() {
        with_temp_cwd(|| {
            // Verify that peek + increment gives same result as next_sprint
            let mut history1 = SprintHistory::load("equiv-team1").unwrap();
            let mut history2 = SprintHistory::load("equiv-team2").unwrap();

            // Using next_sprint
            let sprint_num1 = history1.next_sprint();

            // Using peek + increment
            let peeked = history2.peek_next_sprint();
            history2.increment();

            assert_eq!(sprint_num1, peeked);
            assert_eq!(history1.total_sprints, history2.total_sprints);

            // Repeat for second sprint
            let sprint_num1 = history1.next_sprint();
            let peeked = history2.peek_next_sprint();
            history2.increment();

            assert_eq!(sprint_num1, peeked);
            assert_eq!(history1.total_sprints, history2.total_sprints);
        });
    }

    #[test]
    fn test_load_from_nonexistent_file() {
        with_temp_cwd(|| {
            // Load from a path that doesn't exist (first sprint case)
            let path = PathBuf::from("custom/path/sprint-history.json");
            let history = SprintHistory::load_from(&path).unwrap();

            // Should create default history with 0 sprints
            assert_eq!(history.total_sprints, 0);
            assert_eq!(history.team_name, "unknown");
            assert_eq!(history.path, path);
        });
    }

    #[test]
    fn test_load_from_existing_file() {
        with_temp_cwd(|| {
            // Create a sprint history file at a custom path
            let custom_dir = PathBuf::from("worktree/swarm-hug/my-team");
            fs::create_dir_all(&custom_dir).unwrap();
            let path = custom_dir.join("sprint-history.json");

            // Write existing history
            let content = r#"{
  "team": "my-team",
  "total_sprints": 5
}
"#;
            fs::write(&path, content).unwrap();

            // Load from explicit path
            let history = SprintHistory::load_from(&path).unwrap();

            assert_eq!(history.total_sprints, 5);
            assert_eq!(history.team_name, "my-team");
            assert_eq!(history.path, path);
        });
    }

    #[test]
    fn test_load_from_and_save() {
        with_temp_cwd(|| {
            // Create directory for custom path
            let custom_dir = PathBuf::from("sprint-worktree/data");
            fs::create_dir_all(&custom_dir).unwrap();
            let path = custom_dir.join("sprint-history.json");

            // Load from non-existent file (first sprint)
            let mut history = SprintHistory::load_from(&path).unwrap();
            assert_eq!(history.total_sprints, 0);

            // Increment and save
            history.increment();
            history.increment();
            history.save().unwrap();

            // Reload and verify
            let reloaded = SprintHistory::load_from(&path).unwrap();
            assert_eq!(reloaded.total_sprints, 2);
        });
    }

    #[test]
    fn test_load_from_preserves_team_name_from_file() {
        with_temp_cwd(|| {
            let custom_dir = PathBuf::from("alt-location");
            fs::create_dir_all(&custom_dir).unwrap();
            let path = custom_dir.join("sprint-history.json");

            // Write history with specific team name
            let content = r#"{"team": "alpha-squad", "total_sprints": 10}"#;
            fs::write(&path, content).unwrap();

            let history = SprintHistory::load_from(&path).unwrap();
            assert_eq!(history.team_name, "alpha-squad");
            assert_eq!(history.total_sprints, 10);
        });
    }

    #[test]
    fn test_parse_team_name() {
        // Valid JSON with team
        let result = SprintHistory::parse_team_name(r#"{"team": "test-team", "total_sprints": 1}"#);
        assert_eq!(result, Some("test-team".to_string()));

        // With whitespace
        let result = SprintHistory::parse_team_name(
            r#"{
            "team": "spaced-team",
            "total_sprints": 5
        }"#,
        );
        assert_eq!(result, Some("spaced-team".to_string()));

        // Missing team field
        let result = SprintHistory::parse_team_name(r#"{"total_sprints": 1}"#);
        assert_eq!(result, None);

        // Empty team name
        let result = SprintHistory::parse_team_name(r#"{"team": "", "total_sprints": 1}"#);
        assert_eq!(result, Some("".to_string()));

        // Team name with escaped characters
        let result =
            SprintHistory::parse_team_name(r#"{"team": "test\"team", "total_sprints": 1}"#);
        assert_eq!(result, Some("test\"team".to_string()));
    }

    #[test]
    fn test_parse_json_supports_legacy_sprint_keys() {
        let count = SprintHistory::parse_json(r#"{"team":"legacy","sprint_count":7}"#)
            .expect("parse legacy sprint_count");
        assert_eq!(count, 7);

        let sprint = SprintHistory::parse_json(r#"{"team":"legacy","sprint":3}"#)
            .expect("parse legacy sprint");
        assert_eq!(sprint, 3);
    }
}
