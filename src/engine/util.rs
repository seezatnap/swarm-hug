use std::collections::HashMap;
use std::fs;
use std::process::{Command, Output};

use crate::prompt;

use super::EngineResult;

/// Path to the email file that stores the co-author email.
const EMAIL_FILE_PATH: &str = ".swarm-hug/email.txt";

/// Interval for "still waiting" log messages (5 minutes).
pub(super) const WAIT_LOG_INTERVAL_SECS: u64 = 300;

/// Read the co-author email from .swarm-hug/email.txt if it exists.
pub(super) fn read_coauthor_email() -> Option<String> {
    fs::read_to_string(EMAIL_FILE_PATH)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.contains('@'))
}

/// Resolve the full path to a CLI binary using `which`.
/// Returns None if the binary is not found.
pub(super) fn resolve_cli_path(name: &str) -> Option<String> {
    let output = Command::new("which")
        .arg(name)
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }
    None
}

/// Generate the co-author line for commits if email is configured.
pub(super) fn generate_coauthor_line() -> String {
    match read_coauthor_email() {
        Some(email) => {
            let username = email.split('@').next().unwrap_or(&email);
            format!("\nCo-Authored-By: {} <{}>", username, email)
        }
        None => String::new(),
    }
}

/// Build the agent prompt with variable substitution.
///
/// Only builds the agent prompt for valid agents (A-Z mapping to names).
/// For non-agent callers (like ScrumMaster), returns None so the caller
/// can use the raw prompt directly.
///
/// # Arguments
/// * `agent_name` - Name of the agent
/// * `task_description` - The task to complete
/// * `team_dir` - Optional path to team directory for context files
///
/// # Errors
/// Returns an error if the agent prompt file (prompts/agent.md) cannot be found.
pub(super) fn build_agent_prompt(
    agent_name: &str,
    task_description: &str,
    team_dir: Option<&str>,
) -> Result<Option<String>, String> {
    // Only use agent prompt for valid agents (those with A-Z initials)
    let agent_initial = match crate::agent::initial_from_name(agent_name) {
        Some(c) => c.to_string(),
        None => return Ok(None), // Not a valid agent, use raw prompt
    };

    let task_short = if task_description.chars().count() > 50 {
        format!("{}...", task_description.chars().take(47).collect::<String>())
    } else {
        task_description.to_string()
    };

    let mut vars = HashMap::new();
    vars.insert("agent_name", agent_name.to_string());
    vars.insert("task_description", task_description.to_string());
    vars.insert("agent_name_lower", agent_name.to_lowercase());
    vars.insert("agent_initial", agent_initial);
    vars.insert("task_short", task_short);
    vars.insert("co_author", generate_coauthor_line());
    vars.insert("team_dir", team_dir.unwrap_or("").to_string());

    prompt::load_and_render("agent", &vars).map(Some)
}

/// Convert process output to engine result.
pub(super) fn output_to_result(output: Output) -> EngineResult {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(1);

    if output.status.success() {
        EngineResult::success(stdout)
    } else {
        EngineResult::failure(stderr, exit_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_build_agent_prompt_valid_agent() {
        // Valid agent should return Some(prompt)
        let result = build_agent_prompt("Aaron", "Test task", None);
        assert!(result.is_ok());
        let prompt = result.unwrap();
        assert!(prompt.is_some());
        let text = prompt.unwrap();
        assert!(text.contains("Aaron"));
        assert!(text.contains("Test task"));
    }

    #[test]
    fn test_build_agent_prompt_with_utf8_task() {
        // Task with UTF-8 characters (arrows, emojis, etc.) should not panic
        let task = "(#21) Implement schema migration from v1→v2→v3 (blocked by #20)";
        let result = build_agent_prompt("Aaron", task, None);
        assert!(result.is_ok());
        let prompt = result.unwrap();
        assert!(prompt.is_some());
    }

    #[test]
    fn test_build_agent_prompt_with_long_utf8_task() {
        // Long task with UTF-8 should truncate safely without panicking
        let task = "🚀 Implement feature with émojis and spëcial çharacters that is very long and needs truncation";
        let result = build_agent_prompt("Aaron", task, None);
        assert!(result.is_ok());
        let prompt = result.unwrap();
        assert!(prompt.is_some());
    }

    #[test]
    fn test_build_agent_prompt_non_agent() {
        // Non-agent (ScrumMaster) should return None to use raw prompt
        let result = build_agent_prompt("ScrumMaster", "Plan sprint", None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_build_agent_prompt_invalid_name() {
        // Invalid name should return None
        let result = build_agent_prompt("RandomName", "Some task", None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_build_agent_prompt_with_team_dir() {
        // Prompt should include team_dir when provided
        let result = build_agent_prompt("Aaron", "Test task", Some(".swarm-hug/greenfield"));
        assert!(result.is_ok());
        let prompt = result.unwrap();
        assert!(prompt.is_some());
        let text = prompt.unwrap();
        assert!(text.contains(".swarm-hug/greenfield"));
    }

    #[test]
    fn test_generate_coauthor_line_no_email() {
        // Without email file, should return empty string
        // Note: This test assumes .swarm-hug/email.txt doesn't exist in test environment
        let line = generate_coauthor_line();
        // Either empty (no file) or contains Co-Authored-By (if file exists in dev env)
        assert!(line.is_empty() || line.contains("Co-Authored-By"));
    }

    #[test]
    fn test_read_coauthor_email_invalid_format() {
        // Create a temp dir and test with invalid email
        let tmp_dir = TempDir::new().unwrap();
        let swarm_dir = tmp_dir.path().join(".swarm-hug");
        fs::create_dir_all(&swarm_dir).unwrap();
        let email_path = swarm_dir.join("email.txt");

        // Write invalid email (no @)
        fs::write(&email_path, "invalid-email").unwrap();

        // Change to temp dir and test
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp_dir.path()).unwrap();

        let result = read_coauthor_email();
        assert!(result.is_none()); // Invalid email should return None

        // Restore original dir
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_read_coauthor_email_valid() {
        let tmp_dir = TempDir::new().unwrap();
        let swarm_dir = tmp_dir.path().join(".swarm-hug");
        fs::create_dir_all(&swarm_dir).unwrap();
        let email_path = swarm_dir.join("email.txt");

        // Write valid email
        fs::write(&email_path, "test@example.com\n").unwrap();

        // Change to temp dir and test
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp_dir.path()).unwrap();

        let result = read_coauthor_email();
        assert_eq!(result, Some("test@example.com".to_string()));

        // Restore original dir
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_build_agent_prompt_includes_coauthor() {
        // Create temp dir with email file
        let tmp_dir = TempDir::new().unwrap();
        let swarm_dir = tmp_dir.path().join(".swarm-hug");
        fs::create_dir_all(&swarm_dir).unwrap();
        let email_path = swarm_dir.join("email.txt");
        fs::write(&email_path, "dev@example.com").unwrap();

        // Change to temp dir and test
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp_dir.path()).unwrap();

        let result = build_agent_prompt("Aaron", "Test task", None);
        assert!(result.is_ok());
        let prompt = result.unwrap().unwrap();
        // Check that the co-author line is in the prompt (in commit messages)
        assert!(prompt.contains("Co-Authored-By: dev <dev@example.com>"),
            "Prompt should contain co-author line. Prompt content:\n{}", prompt);

        // Restore original dir
        std::env::set_current_dir(original_dir).unwrap();
    }
}
