//! Engine abstraction for agent execution.
//!
//! Supports multiple backends:
//! - `claude`: Claude CLI
//! - `codex`: Codex CLI
//! - `stub`: Deterministic stub for tests (no network)

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::time::Duration;
use std::thread;

use crate::config::EngineType;
use crate::prompt;

/// Result of engine execution.
#[derive(Debug)]
pub struct EngineResult {
    /// Whether execution succeeded.
    pub success: bool,
    /// Output content (stdout for real engines, stub content for stub).
    pub output: String,
    /// Error message if failed.
    pub error: Option<String>,
    /// Exit code (0 for stub success).
    pub exit_code: i32,
}

impl EngineResult {
    /// Create a successful result.
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            error: None,
            exit_code: 0,
        }
    }

    /// Create a failed result.
    pub fn failure(error: impl Into<String>, exit_code: i32) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(error.into()),
            exit_code,
        }
    }
}

/// Engine trait for agent execution backends.
pub trait Engine: Send + Sync {
    /// Execute a prompt for the given agent and task.
    fn execute(
        &self,
        agent_name: &str,
        task_description: &str,
        working_dir: &Path,
        turn_number: usize,
    ) -> EngineResult;

    /// Get the engine type.
    fn engine_type(&self) -> EngineType;
}

/// Stub engine for testing.
///
/// Writes deterministic output files without network calls.
pub struct StubEngine {
    /// Directory to write stub output files.
    output_dir: String,
}

impl StubEngine {
    /// Create a new stub engine.
    pub fn new(output_dir: impl Into<String>) -> Self {
        Self {
            output_dir: output_dir.into(),
        }
    }

    /// Get the output file path for a given turn and agent.
    fn output_path(&self, turn_number: usize, agent_initial: char) -> String {
        format!("{}/turn{}-agent{}.md", self.output_dir, turn_number, agent_initial)
    }
}

impl Engine for StubEngine {
    fn execute(
        &self,
        agent_name: &str,
        task_description: &str,
        _working_dir: &Path,
        turn_number: usize,
    ) -> EngineResult {
        // Get agent initial from name
        let initial = crate::agent::initial_from_name(agent_name)
            .unwrap_or('?');

        // Ensure output directory exists
        if let Err(e) = fs::create_dir_all(&self.output_dir) {
            return EngineResult::failure(format!("failed to create output dir: {}", e), 1);
        }

        // Write deterministic output file
        let output_path = self.output_path(turn_number, initial);
        let content = format!(
            "# Stub Output\n\nAgent: {}\nTask: {}\nTurn: {}\n\nOK\n",
            agent_name, task_description, turn_number
        );

        match File::create(&output_path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(content.as_bytes()) {
                    return EngineResult::failure(format!("failed to write output: {}", e), 1);
                }
            }
            Err(e) => {
                return EngineResult::failure(format!("failed to create output file: {}", e), 1);
            }
        }

        EngineResult::success(content)
    }

    fn engine_type(&self) -> EngineType {
        EngineType::Stub
    }
}

/// Interval for "still waiting" log messages (5 minutes).
const WAIT_LOG_INTERVAL_SECS: u64 = 300;

/// Claude CLI engine.
pub struct ClaudeEngine {
    /// Path to claude CLI binary.
    cli_path: String,
}

impl ClaudeEngine {
    /// Create a new Claude engine.
    pub fn new() -> Self {
        Self {
            cli_path: "claude".to_string(),
        }
    }

    /// Create with custom CLI path.
    pub fn with_path(cli_path: impl Into<String>) -> Self {
        Self {
            cli_path: cli_path.into(),
        }
    }
}

impl Default for ClaudeEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the agent prompt with variable substitution.
///
/// Only builds the agent prompt for valid agents (A-Z mapping to names).
/// For non-agent callers (like ScrumMaster), returns None so the caller
/// can use the raw prompt directly.
///
/// # Errors
/// Returns an error if the agent prompt file (prompts/agent.md) cannot be found.
fn build_agent_prompt(agent_name: &str, task_description: &str) -> Result<Option<String>, String> {
    // Only use agent prompt for valid agents (those with A-Z initials)
    let agent_initial = match crate::agent::initial_from_name(agent_name) {
        Some(c) => c.to_string(),
        None => return Ok(None), // Not a valid agent, use raw prompt
    };

    let task_short = if task_description.len() > 50 {
        format!("{}...", &task_description[..47])
    } else {
        task_description.to_string()
    };

    let mut vars = HashMap::new();
    vars.insert("agent_name", agent_name.to_string());
    vars.insert("task_description", task_description.to_string());
    vars.insert("agent_name_lower", agent_name.to_lowercase());
    vars.insert("agent_initial", agent_initial);
    vars.insert("task_short", task_short);

    prompt::load_and_render("agent", &vars).map(Some)
}

impl Engine for ClaudeEngine {
    fn execute(
        &self,
        agent_name: &str,
        task_description: &str,
        working_dir: &Path,
        _turn_number: usize,
    ) -> EngineResult {
        // For valid agents, wrap in agent prompt; otherwise use raw prompt
        let prompt = match build_agent_prompt(agent_name, task_description) {
            Ok(Some(p)) => p,
            Ok(None) => task_description.to_string(), // Non-agent (e.g., ScrumMaster)
            Err(e) => return EngineResult::failure(e, 1),
        };

        let mut child = match Command::new(&self.cli_path)
            .arg("--dangerously-skip-permissions")
            .arg("--print")
            .arg(&prompt)
            .current_dir(working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return EngineResult::failure(format!("failed to spawn claude: {}", e), 1),
        };

        let pid = child.id();
        let start = std::time::Instant::now();
        let log_interval = Duration::from_secs(WAIT_LOG_INTERVAL_SECS);
        let mut next_log = log_interval;

        // Wait for completion, logging periodically
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    match child.wait_with_output() {
                        Ok(output) => return output_to_result(output),
                        Err(e) => return EngineResult::failure(format!("failed to get output: {}", e), 1),
                    }
                }
                Ok(None) => {
                    // Process still running
                    let elapsed = start.elapsed();
                    if elapsed >= next_log {
                        let mins = elapsed.as_secs() / 60;
                        eprintln!("[{}] Still executing... ({} min elapsed, pid {})", agent_name, mins, pid);
                        next_log += log_interval;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return EngineResult::failure(format!("failed to wait for claude: {}", e), 1);
                }
            }
        }
    }

    fn engine_type(&self) -> EngineType {
        EngineType::Claude
    }
}

/// Codex CLI engine.
pub struct CodexEngine {
    /// Path to codex CLI binary.
    cli_path: String,
}

impl CodexEngine {
    /// Create a new Codex engine.
    pub fn new() -> Self {
        Self {
            cli_path: "codex".to_string(),
        }
    }

    /// Create with custom CLI path.
    pub fn with_path(cli_path: impl Into<String>) -> Self {
        Self {
            cli_path: cli_path.into(),
        }
    }
}

impl Default for CodexEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for CodexEngine {
    fn execute(
        &self,
        agent_name: &str,
        task_description: &str,
        working_dir: &Path,
        _turn_number: usize,
    ) -> EngineResult {
        // For valid agents, wrap in agent prompt; otherwise use raw prompt
        let prompt = match build_agent_prompt(agent_name, task_description) {
            Ok(Some(p)) => p,
            Ok(None) => task_description.to_string(), // Non-agent (e.g., ScrumMaster)
            Err(e) => return EngineResult::failure(e, 1),
        };

        // Codex uses "exec" subcommand with stdin for prompts
        let mut child = match Command::new(&self.cli_path)
            .arg("exec")
            .arg("--dangerously-bypass-approvals-and-sandbox")
            .arg("-")  // Read prompt from stdin
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return EngineResult::failure(format!("failed to spawn codex: {}", e), 1),
        };

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }

        let pid = child.id();
        let start = std::time::Instant::now();
        let log_interval = Duration::from_secs(WAIT_LOG_INTERVAL_SECS);
        let mut next_log = log_interval;

        // Wait for completion, logging periodically
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    match child.wait_with_output() {
                        Ok(output) => return output_to_result(output),
                        Err(e) => return EngineResult::failure(format!("failed to get output: {}", e), 1),
                    }
                }
                Ok(None) => {
                    let elapsed = start.elapsed();
                    if elapsed >= next_log {
                        let mins = elapsed.as_secs() / 60;
                        eprintln!("[{}] Still executing... ({} min elapsed, pid {})", agent_name, mins, pid);
                        next_log += log_interval;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return EngineResult::failure(format!("failed to wait for codex: {}", e), 1);
                }
            }
        }
    }

    fn engine_type(&self) -> EngineType {
        EngineType::Codex
    }
}

/// Convert process output to engine result.
fn output_to_result(output: Output) -> EngineResult {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(1);

    if output.status.success() {
        EngineResult::success(stdout)
    } else {
        EngineResult::failure(stderr, exit_code)
    }
}

/// Create an engine from config.
/// Returns Arc for thread-safe sharing across parallel agent execution.
pub fn create_engine(engine_type: EngineType, output_dir: &str) -> Arc<dyn Engine> {
    match engine_type {
        EngineType::Claude => Arc::new(ClaudeEngine::new()),
        EngineType::Codex => Arc::new(CodexEngine::new()),
        EngineType::Stub => Arc::new(StubEngine::new(output_dir)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_engine_result_success() {
        let result = EngineResult::success("output");
        assert!(result.success);
        assert_eq!(result.output, "output");
        assert!(result.error.is_none());
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn test_engine_result_failure() {
        let result = EngineResult::failure("error message", 1);
        assert!(!result.success);
        assert!(result.output.is_empty());
        assert_eq!(result.error, Some("error message".to_string()));
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn test_stub_engine_execute() {
        let tmp_dir = TempDir::new().unwrap();
        let output_dir = tmp_dir.path().join("loop");
        let engine = StubEngine::new(output_dir.to_str().unwrap());

        let result = engine.execute(
            "Aaron",
            "Write tests",
            tmp_dir.path(),
            1,
        );

        assert!(result.success);
        assert!(result.output.contains("OK"));
        assert!(result.output.contains("Aaron"));
        assert!(result.output.contains("Write tests"));

        // Verify output file was created
        let output_file = output_dir.join("turn1-agentA.md");
        assert!(output_file.exists());

        let content = fs::read_to_string(&output_file).unwrap();
        assert!(content.contains("OK"));
    }

    #[test]
    fn test_stub_engine_deterministic() {
        let tmp_dir = TempDir::new().unwrap();
        let output_dir = tmp_dir.path().join("loop");
        let engine = StubEngine::new(output_dir.to_str().unwrap());

        // Execute twice with same parameters
        let result1 = engine.execute("Aaron", "Task 1", tmp_dir.path(), 1);
        let result2 = engine.execute("Aaron", "Task 1", tmp_dir.path(), 1);

        // Output should be identical
        assert_eq!(result1.output, result2.output);
    }

    #[test]
    fn test_stub_engine_type() {
        let engine = StubEngine::new("loop");
        assert_eq!(engine.engine_type(), EngineType::Stub);
    }

    #[test]
    fn test_claude_engine_type() {
        let engine = ClaudeEngine::new();
        assert_eq!(engine.engine_type(), EngineType::Claude);
    }

    #[test]
    fn test_codex_engine_type() {
        let engine = CodexEngine::new();
        assert_eq!(engine.engine_type(), EngineType::Codex);
    }

    #[test]
    fn test_create_engine_stub() {
        let engine = create_engine(EngineType::Stub, "loop");
        assert_eq!(engine.engine_type(), EngineType::Stub);
    }

    #[test]
    fn test_create_engine_claude() {
        let engine = create_engine(EngineType::Claude, "loop");
        assert_eq!(engine.engine_type(), EngineType::Claude);
    }

    #[test]
    fn test_create_engine_codex() {
        let engine = create_engine(EngineType::Codex, "loop");
        assert_eq!(engine.engine_type(), EngineType::Codex);
    }

    #[test]
    fn test_stub_engine_multiple_agents() {
        let tmp_dir = TempDir::new().unwrap();
        let output_dir = tmp_dir.path().join("loop");
        let engine = StubEngine::new(output_dir.to_str().unwrap());

        engine.execute("Aaron", "Task A", tmp_dir.path(), 1);
        engine.execute("Betty", "Task B", tmp_dir.path(), 1);

        // Both files should exist
        assert!(output_dir.join("turn1-agentA.md").exists());
        assert!(output_dir.join("turn1-agentB.md").exists());
    }

    #[test]
    fn test_stub_engine_multiple_turns() {
        let tmp_dir = TempDir::new().unwrap();
        let output_dir = tmp_dir.path().join("loop");
        let engine = StubEngine::new(output_dir.to_str().unwrap());

        engine.execute("Aaron", "Task 1", tmp_dir.path(), 1);
        engine.execute("Aaron", "Task 2", tmp_dir.path(), 2);

        // Both turn files should exist
        assert!(output_dir.join("turn1-agentA.md").exists());
        assert!(output_dir.join("turn2-agentA.md").exists());
    }

    #[test]
    fn test_build_agent_prompt_valid_agent() {
        // Valid agent should return Some(prompt)
        let result = super::build_agent_prompt("Aaron", "Test task");
        assert!(result.is_ok());
        let prompt = result.unwrap();
        assert!(prompt.is_some());
        let text = prompt.unwrap();
        assert!(text.contains("Aaron"));
        assert!(text.contains("Test task"));
    }

    #[test]
    fn test_build_agent_prompt_non_agent() {
        // Non-agent (ScrumMaster) should return None to use raw prompt
        let result = super::build_agent_prompt("ScrumMaster", "Plan sprint");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_build_agent_prompt_invalid_name() {
        // Invalid name should return None
        let result = super::build_agent_prompt("RandomName", "Some task");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }
}
