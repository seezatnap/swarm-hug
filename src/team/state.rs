use std::fs;
use std::path::{Path, PathBuf};

use super::{TEAM_STATE_FILE, SWARM_HUG_DIR};

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

    /// Save team state to disk.
    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directory: {}", e))?;
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

        let key = "\"feature_branch\"";
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

        Ok(None)
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
}
