use std::fs;
use std::path::{Path, PathBuf};

use super::{SWARM_HUG_DIR, TEAM_STATE_FILE};

/// Persisted team state for merge operations.
#[derive(Debug, Clone)]
pub struct TeamState {
    /// Team name.
    pub team_name: String,
    /// Current feature/sprint branch name.
    pub feature_branch: Option<String>,
    path: PathBuf,
}

impl TeamState {
    /// Load team state for a team.
    ///
    /// Creates a new state with no feature branch if the file doesn't exist.
    pub fn load(team_name: &str) -> Result<Self, String> {
        let path = PathBuf::from(SWARM_HUG_DIR)
            .join(team_name)
            .join(TEAM_STATE_FILE);

        let feature_branch = if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
            Self::parse_json(&content)?
        } else {
            None
        };

        Ok(Self {
            team_name: team_name.to_string(),
            feature_branch: feature_branch.filter(|branch| !branch.trim().is_empty()),
            path,
        })
    }

    /// Load team state from an explicit path.
    ///
    /// Creates a new state with no feature branch if the file doesn't exist.
    /// The team name is extracted from the JSON content if the file exists,
    /// or derived from the parent directory name if it doesn't.
    pub fn load_from(path: &Path) -> Result<Self, String> {
        if path.exists() {
            let content = fs::read_to_string(path)
                .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
            let (team_name, feature_branch) = match Self::parse_json_full(&content) {
                Ok(parsed) => parsed,
                Err(err) if err == "missing team field in team state" => {
                    // Legacy compatibility: old team-state.json files may omit "team".
                    // Fall back to deriving team from the path and parsing feature branch only.
                    let team_name = derive_team_name_from_path(path)?;
                    let feature_branch = Self::parse_json(&content)?;
                    (team_name, feature_branch)
                }
                Err(err) => return Err(err),
            };
            Ok(Self {
                team_name,
                feature_branch: feature_branch.filter(|branch| !branch.trim().is_empty()),
                path: path.to_path_buf(),
            })
        } else {
            // Derive team name from parent directory (path is .swarm-hug/<team>/team-state.json)
            let team_name = derive_team_name_from_path(path)?;

            Ok(Self {
                team_name,
                feature_branch: None,
                path: path.to_path_buf(),
            })
        }
    }

    /// Save team state to disk.
    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("failed to create directory: {}", e))?;
        }

        let content = self.to_json();
        fs::write(&self.path, content)
            .map_err(|e| format!("failed to write {}: {}", self.path.display(), e))?;
        Ok(())
    }

    /// Update the feature branch name.
    pub fn set_feature_branch(&mut self, branch: &str) -> Result<(), String> {
        let trimmed = branch.trim();
        if trimmed.is_empty() {
            return Err("feature branch name is empty".to_string());
        }
        self.feature_branch = Some(trimmed.to_string());
        Ok(())
    }

    /// Clear the feature branch name.
    pub fn clear_feature_branch(&mut self) {
        self.feature_branch = None;
    }

    /// Path to the team state file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn parse_json(content: &str) -> Result<Option<String>, String> {
        let content = content.trim();
        if !content.starts_with('{') || !content.ends_with('}') {
            return Err("invalid team state JSON".to_string());
        }

        // "feature_branch" is canonical; "sprint_branch" is a legacy alias.
        for key in ["\"feature_branch\"", "\"sprint_branch\""] {
            if let Some(idx) = content.find(key) {
                let after_key = &content[idx + key.len()..];
                if let Some(colon_idx) = after_key.find(':') {
                    let after_colon = after_key[colon_idx + 1..].trim_start();
                    if after_colon.starts_with("null") {
                        return Ok(None);
                    }
                    if after_colon.starts_with('"') {
                        let value = parse_json_string(after_colon)?;
                        return Ok(Some(value));
                    }
                }
                return Err("invalid feature_branch value".to_string());
            }
        }

        Ok(None)
    }

    /// Parse JSON content and extract both team name and feature branch.
    fn parse_json_full(content: &str) -> Result<(String, Option<String>), String> {
        let content = content.trim();
        if !content.starts_with('{') || !content.ends_with('}') {
            return Err("invalid team state JSON".to_string());
        }

        // Extract team name
        let team_key = "\"team\"";
        let team_name = if let Some(idx) = content.find(team_key) {
            let after_key = &content[idx + team_key.len()..];
            if let Some(colon_idx) = after_key.find(':') {
                let after_colon = after_key[colon_idx + 1..].trim_start();
                if after_colon.starts_with('"') {
                    parse_json_string(after_colon)?
                } else {
                    return Err("invalid team value".to_string());
                }
            } else {
                return Err("invalid team field".to_string());
            }
        } else {
            return Err("missing team field in team state".to_string());
        };

        // Extract feature branch (reuse existing logic)
        let feature_branch = Self::parse_json(content)?;

        Ok((team_name, feature_branch))
    }

    fn to_json(&self) -> String {
        let team = escape_json_string(&self.team_name);
        let feature = match &self.feature_branch {
            Some(branch) => format!("\"{}\"", escape_json_string(branch)),
            None => "null".to_string(),
        };
        format!(
            "{{\n  \"team\": \"{}\",\n  \"feature_branch\": {}\n}}\n",
            team, feature
        )
    }
}

fn derive_team_name_from_path(path: &Path) -> Result<String, String> {
    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("cannot derive team name from path: {}", path.display()))
}

fn escape_json_string(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn parse_json_string(input: &str) -> Result<String, String> {
    let mut chars = input.chars();
    if chars.next() != Some('"') {
        return Err("expected JSON string".to_string());
    }

    let mut out = String::new();
    let mut escaped = false;
    for ch in chars {
        if escaped {
            let decoded = match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '"' => '"',
                other => other,
            };
            out.push(decoded);
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == '"' {
            return Ok(out);
        }

        out.push(ch);
    }

    Err("unterminated JSON string".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::with_temp_cwd;

    #[test]
    fn test_team_state_load_new() {
        with_temp_cwd(|| {
            let state = TeamState::load("new-team").unwrap();
            assert_eq!(state.team_name, "new-team");
            assert_eq!(state.feature_branch, None);
        });
    }

    #[test]
    fn test_team_state_save_and_load() {
        with_temp_cwd(|| {
            let mut state = TeamState::load("beta").unwrap();
            state.set_feature_branch("beta-sprint-1").unwrap();
            state.save().unwrap();

            let loaded = TeamState::load("beta").unwrap();
            assert_eq!(loaded.feature_branch, Some("beta-sprint-1".to_string()));
        });
    }

    #[test]
    fn test_team_state_parse_json() {
        let feature =
            TeamState::parse_json(r#"{"team":"t","feature_branch":"alpha-sprint-1"}"#).unwrap();
        assert_eq!(feature, Some("alpha-sprint-1".to_string()));

        let none = TeamState::parse_json(r#"{"team":"t","feature_branch":null}"#).unwrap();
        assert_eq!(none, None);
    }

    #[test]
    fn test_team_state_to_json() {
        with_temp_cwd(|| {
            let mut state = TeamState::load("gamma").unwrap();
            state.set_feature_branch("gamma-sprint-2").unwrap();
            let json = state.to_json();
            assert!(json.contains("\"team\": \"gamma\""));
            assert!(json.contains("\"feature_branch\": \"gamma-sprint-2\""));
        });
    }

    #[test]
    fn test_team_state_load_from_existing() {
        with_temp_cwd(|| {
            // Create a team state file via the normal load/save path
            let mut state = TeamState::load("load-from-team").unwrap();
            state.set_feature_branch("load-from-sprint-1").unwrap();
            state.save().unwrap();

            // Now load using load_from with explicit path
            let path = PathBuf::from(SWARM_HUG_DIR)
                .join("load-from-team")
                .join(TEAM_STATE_FILE);
            let loaded = TeamState::load_from(&path).unwrap();

            assert_eq!(loaded.team_name, "load-from-team");
            assert_eq!(
                loaded.feature_branch,
                Some("load-from-sprint-1".to_string())
            );
            assert_eq!(loaded.path(), &path);
        });
    }

    #[test]
    fn test_team_state_load_from_nonexistent() {
        with_temp_cwd(|| {
            // Create the parent directory but not the file
            let team_dir = PathBuf::from(SWARM_HUG_DIR).join("new-load-from-team");
            fs::create_dir_all(&team_dir).unwrap();

            let path = team_dir.join(TEAM_STATE_FILE);
            let loaded = TeamState::load_from(&path).unwrap();

            // Team name should be derived from parent directory
            assert_eq!(loaded.team_name, "new-load-from-team");
            assert_eq!(loaded.feature_branch, None);
            assert_eq!(loaded.path(), &path);
        });
    }

    #[test]
    fn test_team_state_load_from_save_and_reload() {
        with_temp_cwd(|| {
            // Create parent directory
            let team_dir = PathBuf::from(SWARM_HUG_DIR).join("save-reload-team");
            fs::create_dir_all(&team_dir).unwrap();
            let path = team_dir.join(TEAM_STATE_FILE);

            // Load from nonexistent file (creates default state)
            let mut state = TeamState::load_from(&path).unwrap();
            assert_eq!(state.team_name, "save-reload-team");
            assert_eq!(state.feature_branch, None);

            // Modify and save
            state.set_feature_branch("save-reload-sprint-1").unwrap();
            state.save().unwrap();

            // Reload and verify
            let reloaded = TeamState::load_from(&path).unwrap();
            assert_eq!(reloaded.team_name, "save-reload-team");
            assert_eq!(
                reloaded.feature_branch,
                Some("save-reload-sprint-1".to_string())
            );
        });
    }

    #[test]
    fn test_team_state_parse_json_full() {
        // Valid JSON with feature branch
        let (team, feature) =
            TeamState::parse_json_full(r#"{"team": "alpha", "feature_branch": "alpha-sprint-1"}"#)
                .unwrap();
        assert_eq!(team, "alpha");
        assert_eq!(feature, Some("alpha-sprint-1".to_string()));

        // Valid JSON with null feature branch
        let (team, feature) =
            TeamState::parse_json_full(r#"{"team": "beta", "feature_branch": null}"#).unwrap();
        assert_eq!(team, "beta");
        assert_eq!(feature, None);

        // Missing team field should error
        let result = TeamState::parse_json_full(r#"{"feature_branch": "sprint-1"}"#);
        assert!(result.is_err());

        // Invalid JSON should error
        let result = TeamState::parse_json_full("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_team_state_load_from_legacy_without_team_field() {
        with_temp_cwd(|| {
            let team_dir = PathBuf::from(SWARM_HUG_DIR).join("legacy-team");
            fs::create_dir_all(&team_dir).unwrap();
            let path = team_dir.join(TEAM_STATE_FILE);

            // Legacy shape: no "team" key.
            fs::write(&path, r#"{"feature_branch":"legacy-sprint-2"}"#).unwrap();

            let loaded = TeamState::load_from(&path).unwrap();
            assert_eq!(loaded.team_name, "legacy-team");
            assert_eq!(loaded.feature_branch, Some("legacy-sprint-2".to_string()));
        });
    }

    #[test]
    fn test_team_state_load_from_legacy_sprint_branch_key() {
        with_temp_cwd(|| {
            let team_dir = PathBuf::from(SWARM_HUG_DIR).join("legacy-key-team");
            fs::create_dir_all(&team_dir).unwrap();
            let path = team_dir.join(TEAM_STATE_FILE);

            // Legacy key alias: "sprint_branch".
            fs::write(&path, r#"{"sprint_branch":"legacy-sprint-3"}"#).unwrap();

            let loaded = TeamState::load_from(&path).unwrap();
            assert_eq!(loaded.team_name, "legacy-key-team");
            assert_eq!(loaded.feature_branch, Some("legacy-sprint-3".to_string()));
        });
    }
}
