use std::fs;
use std::path::Path;

use super::types::{Config, ConfigError, EngineType};

pub(super) fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Config, ConfigError> {
    let content = fs::read_to_string(&path).map_err(|e| ConfigError::Io(e.to_string()))?;
    Config::parse_toml(&content)
}

pub(super) fn parse_toml(content: &str) -> Result<Config, ConfigError> {
    let mut config = Config::default();
    let mut current_section = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Handle section headers like [agents]
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].to_string();
            continue;
        }

        if let Some((key, value)) = parse_toml_line(line) {
            // Build full key with section prefix
            let full_key = if current_section.is_empty() {
                key.to_string()
            } else {
                format!("{}.{}", current_section, key)
            };

            match full_key.as_str() {
                "agents.max_count" => {
                    config.agents_max_count = value
                        .parse()
                        .map_err(|_| ConfigError::Parse(format!("invalid agents.max_count: {}", value)))?;
                }
                "agents.tasks_per_agent" => {
                    config.agents_tasks_per_agent = value
                        .parse()
                        .map_err(|_| {
                            ConfigError::Parse(format!(
                                "invalid agents.tasks_per_agent: {}",
                                value
                            ))
                        })?;
                }
                "agents.timeout" => {
                    config.agent_timeout_secs = value
                        .parse()
                        .map_err(|_| ConfigError::Parse(format!("invalid agents.timeout: {}", value)))?;
                }
                "files.tasks" => {
                    config.files_tasks = value.trim_matches('"').to_string();
                }
                "files.chat" => {
                    config.files_chat = value.trim_matches('"').to_string();
                }
                "files.log_dir" => {
                    config.files_log_dir = value.trim_matches('"').to_string();
                }
                "engine.type" => {
                    let engine_str = value.trim_matches('"');
                    config.engine_types = EngineType::parse_list(engine_str)
                        .ok_or_else(|| ConfigError::Parse(format!("invalid engine.type: {}", engine_str)))?;
                }
                "engine.stub_mode" => {
                    config.engine_stub_mode = value == "true";
                }
                "sprints.max" => {
                    config.sprints_max = value
                        .parse()
                        .map_err(|_| ConfigError::Parse(format!("invalid sprints.max: {}", value)))?;
                }
                "worktree.relative_paths" => {
                    let normalized = value.trim_matches('"').to_lowercase();
                    match normalized.as_str() {
                        "true" => config.worktree_relative_paths = Some(true),
                        "false" => config.worktree_relative_paths = Some(false),
                        _ => {
                            return Err(ConfigError::Parse(format!(
                                "invalid worktree.relative_paths: {}",
                                value
                            )))
                        }
                    }
                }
                _ => {} // Ignore unknown keys
            }
        }
    }

    Ok(config)
}

/// Parse a TOML line into key-value pair.
/// Handles dotted keys like "agents.max_count = 4".
fn parse_toml_line(line: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return None;
    }
    Some((parts[0].trim(), parts[1].trim()))
}
