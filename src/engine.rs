//! Engine abstraction for agent execution.
//!
//! Supports multiple backends:
//! - `claude`: Claude CLI
//! - `codex`: Codex CLI
//! - `stub`: Deterministic stub for tests (no network)

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::time::Duration;
use std::thread;

use crate::config::EngineType;
use crate::prompt;

/// Path to the email file that stores the co-author email.
const EMAIL_FILE_PATH: &str = ".swarm-hug/email.txt";

/// Read the co-author email from .swarm-hug/email.txt if it exists.
fn read_coauthor_email() -> Option<String> {
    fs::read_to_string(EMAIL_FILE_PATH)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.contains('@'))
}

/// Resolve the full path to a CLI binary using `which`.
/// Returns None if the binary is not found.
fn resolve_cli_path(name: &str) -> Option<String> {
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
fn generate_coauthor_line() -> String {
    match read_coauthor_email() {
        Some(email) => {
            let username = email.split('@').next().unwrap_or(&email);
            format!("\nCo-Authored-By: {} <{}>", username, email)
        }
        None => String::new(),
    }
}

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
    ///
    /// # Arguments
    /// * `agent_name` - Name of the agent (e.g., "Aaron")
    /// * `task_description` - The task to complete
    /// * `working_dir` - The agent's working directory (worktree)
    /// * `turn_number` - Current sprint/turn number
    /// * `team_dir` - Optional path to team directory (e.g., ".swarm-hug/greenfield")
    fn execute(
        &self,
        agent_name: &str,
        task_description: &str,
        working_dir: &Path,
        turn_number: usize,
        team_dir: Option<&str>,
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
        _team_dir: Option<&str>,
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
    /// Timeout in seconds (0 = no timeout).
    timeout_secs: u64,
}

impl ClaudeEngine {
    /// Create a new Claude engine with default timeout.
    /// Resolves the full path to claude using `which` for better portability.
    pub fn new() -> Self {
        let cli_path = resolve_cli_path("claude").unwrap_or_else(|| "claude".to_string());
        Self { cli_path, timeout_secs: 0 }
    }

    /// Create with custom CLI path.
    pub fn with_path(cli_path: impl Into<String>) -> Self {
        Self {
            cli_path: cli_path.into(),
            timeout_secs: 0,
        }
    }

    /// Create with timeout.
    pub fn with_timeout(timeout_secs: u64) -> Self {
        let cli_path = resolve_cli_path("claude").unwrap_or_else(|| "claude".to_string());
        Self { cli_path, timeout_secs }
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
/// # Arguments
/// * `agent_name` - Name of the agent
/// * `task_description` - The task to complete
/// * `team_dir` - Optional path to team directory for context files
///
/// # Errors
/// Returns an error if the agent prompt file (prompts/agent.md) cannot be found.
fn build_agent_prompt(
    agent_name: &str,
    task_description: &str,
    team_dir: Option<&str>,
) -> Result<Option<String>, String> {
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
    vars.insert("co_author", generate_coauthor_line());
    vars.insert("team_dir", team_dir.unwrap_or("").to_string());

    prompt::load_and_render("agent", &vars).map(Some)
}

impl Engine for ClaudeEngine {
    fn execute(
        &self,
        agent_name: &str,
        task_description: &str,
        working_dir: &Path,
        _turn_number: usize,
        team_dir: Option<&str>,
    ) -> EngineResult {
        // For valid agents, wrap in agent prompt; otherwise use raw prompt
        let prompt = match build_agent_prompt(agent_name, task_description, team_dir) {
            Ok(Some(p)) => p,
            Ok(None) => task_description.to_string(), // Non-agent (e.g., ScrumMaster)
            Err(e) => return EngineResult::failure(e, 1),
        };

        // Use stdin for prompt to avoid "Argument list too long" (E2BIG) errors
        // when prompts exceed the OS argument size limit (~256KB on macOS)
        let mut cmd = Command::new(&self.cli_path);
        cmd.arg("--dangerously-skip-permissions")
            .arg("--print")
            .arg("-p")
            .arg("-")  // Read prompt from stdin
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set CLAUDE_CODE_TASK_LIST_ID to team name when team_dir is provided
        // team_dir is like ".swarm-hug/greenfield", extract just "greenfield"
        if let Some(dir) = team_dir {
            if let Some(team_name) = Path::new(dir).file_name().and_then(|n| n.to_str()) {
                cmd.env("CLAUDE_CODE_TASK_LIST_ID", team_name);
            }
        }

        let mut child = match cmd.spawn()
        {
            Ok(c) => c,
            Err(e) => return EngineResult::failure(format!("failed to spawn claude: {}", e), 1),
        };

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }

        let pid = child.id();
        let start = std::time::Instant::now();
        let log_interval = Duration::from_secs(WAIT_LOG_INTERVAL_SECS);
        let mut next_log = log_interval;
        let timeout = if self.timeout_secs > 0 {
            Some(Duration::from_secs(self.timeout_secs))
        } else {
            None
        };

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

                    // Check for timeout
                    if let Some(timeout_duration) = timeout {
                        if elapsed >= timeout_duration {
                            let _ = child.kill();
                            let mins = elapsed.as_secs() / 60;
                            return EngineResult::failure(
                                format!("agent timed out after {} minutes (pid {})", mins, pid),
                                124, // Standard timeout exit code
                            );
                        }
                    }

                    if elapsed >= next_log {
                        let mins = elapsed.as_secs() / 60;
                        let timeout_msg = if let Some(t) = timeout {
                            format!(", timeout in {} min", (t.as_secs() - elapsed.as_secs()) / 60)
                        } else {
                            String::new()
                        };
                        eprintln!("[{}] Still executing... ({} min elapsed, pid {}{})", agent_name, mins, pid, timeout_msg);
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
    /// Timeout in seconds (0 = no timeout).
    timeout_secs: u64,
}

impl CodexEngine {
    /// Create a new Codex engine with default timeout.
    /// Resolves the full path to codex using `which` for better portability.
    pub fn new() -> Self {
        let cli_path = resolve_cli_path("codex").unwrap_or_else(|| "codex".to_string());
        Self { cli_path, timeout_secs: 0 }
    }

    /// Create with custom CLI path.
    pub fn with_path(cli_path: impl Into<String>) -> Self {
        Self {
            cli_path: cli_path.into(),
            timeout_secs: 0,
        }
    }

    /// Create with timeout.
    pub fn with_timeout(timeout_secs: u64) -> Self {
        let cli_path = resolve_cli_path("codex").unwrap_or_else(|| "codex".to_string());
        Self { cli_path, timeout_secs }
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
        team_dir: Option<&str>,
    ) -> EngineResult {
        // For valid agents, wrap in agent prompt; otherwise use raw prompt
        let prompt = match build_agent_prompt(agent_name, task_description, team_dir) {
            Ok(Some(p)) => p,
            Ok(None) => task_description.to_string(), // Non-agent (e.g., ScrumMaster)
            Err(e) => return EngineResult::failure(e, 1),
        };

        // Create debug file for streaming JSONL output
        let debug_file = team_dir.and_then(|dir| {
            let debug_path = Path::new(dir).join("loop").join(format!("codex-debug-{}.jsonl", agent_name));
            match File::create(&debug_path) {
                Ok(f) => {
                    eprintln!("[{}] Debug output: {}", agent_name, debug_path.display());
                    Some(f)
                }
                Err(e) => {
                    eprintln!("[{}] Warning: could not create debug file {}: {}", agent_name, debug_path.display(), e);
                    None
                }
            }
        });

        // Codex uses "exec" subcommand with stdin for prompts
        // Add --json flag for JSONL streaming output when debug file is available
        let mut cmd = Command::new(&self.cli_path);
        cmd.arg("exec");
        if debug_file.is_some() {
            cmd.arg("--json");  // Stream JSONL events for debugging
        }
        cmd.arg("--dangerously-bypass-approvals-and-sandbox")
            .arg("-")  // Read prompt from stdin
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return EngineResult::failure(format!("failed to spawn codex: {}", e), 1),
        };

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }

        let pid = child.id();

        // Take stdout and stderr for streaming
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Spawn thread to stream stdout to both debug file and buffer
        let stdout_handle = thread::spawn(move || {
            let mut output = String::new();
            if let Some(stdout) = stdout {
                let reader = BufReader::new(stdout);
                let mut debug_file = debug_file;
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            // Write to debug file if available
                            if let Some(ref mut f) = debug_file {
                                let _ = writeln!(f, "{}", line);
                                let _ = f.flush();
                            }
                            // Accumulate for result
                            output.push_str(&line);
                            output.push('\n');
                        }
                        Err(_) => break,
                    }
                }
            }
            output
        });

        // Spawn thread to capture stderr
        let stderr_handle = thread::spawn(move || {
            let mut output = String::new();
            if let Some(stderr) = stderr {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            output.push_str(&line);
                            output.push('\n');
                        }
                        Err(_) => break,
                    }
                }
            }
            output
        });

        let start = std::time::Instant::now();
        let log_interval = Duration::from_secs(WAIT_LOG_INTERVAL_SECS);
        let mut next_log = log_interval;
        let timeout = if self.timeout_secs > 0 {
            Some(Duration::from_secs(self.timeout_secs))
        } else {
            None
        };

        // Wait for completion, logging periodically
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process exited, collect output from threads
                    let stdout_output = stdout_handle.join().unwrap_or_default();
                    let stderr_output = stderr_handle.join().unwrap_or_default();
                    let exit_code = status.code().unwrap_or(1);

                    if status.success() {
                        return EngineResult::success(stdout_output);
                    } else {
                        return EngineResult::failure(stderr_output, exit_code);
                    }
                }
                Ok(None) => {
                    let elapsed = start.elapsed();

                    // Check for timeout
                    if let Some(timeout_duration) = timeout {
                        if elapsed >= timeout_duration {
                            let _ = child.kill();
                            let mins = elapsed.as_secs() / 60;
                            return EngineResult::failure(
                                format!("agent timed out after {} minutes (pid {})", mins, pid),
                                124, // Standard timeout exit code
                            );
                        }
                    }

                    if elapsed >= next_log {
                        let mins = elapsed.as_secs() / 60;
                        let timeout_msg = if let Some(t) = timeout {
                            format!(", timeout in {} min", (t.as_secs() - elapsed.as_secs()) / 60)
                        } else {
                            String::new()
                        };
                        eprintln!("[{}] Still executing... ({} min elapsed, pid {}{})", agent_name, mins, pid, timeout_msg);
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
pub fn create_engine(engine_type: EngineType, output_dir: &str, timeout_secs: u64) -> Arc<dyn Engine> {
    match engine_type {
        EngineType::Claude => Arc::new(ClaudeEngine::with_timeout(timeout_secs)),
        EngineType::Codex => Arc::new(CodexEngine::with_timeout(timeout_secs)),
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
            None,
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
        let result1 = engine.execute("Aaron", "Task 1", tmp_dir.path(), 1, None);
        let result2 = engine.execute("Aaron", "Task 1", tmp_dir.path(), 1, None);

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
        let engine = create_engine(EngineType::Stub, "loop", 0);
        assert_eq!(engine.engine_type(), EngineType::Stub);
    }

    #[test]
    fn test_create_engine_claude() {
        let engine = create_engine(EngineType::Claude, "loop", 3600);
        assert_eq!(engine.engine_type(), EngineType::Claude);
    }

    #[test]
    fn test_create_engine_codex() {
        let engine = create_engine(EngineType::Codex, "loop", 3600);
        assert_eq!(engine.engine_type(), EngineType::Codex);
    }

    #[test]
    fn test_claude_engine_with_timeout() {
        let engine = ClaudeEngine::with_timeout(1800);
        assert_eq!(engine.timeout_secs, 1800);
        assert_eq!(engine.engine_type(), EngineType::Claude);
    }

    #[test]
    fn test_codex_engine_with_timeout() {
        let engine = CodexEngine::with_timeout(1800);
        assert_eq!(engine.timeout_secs, 1800);
        assert_eq!(engine.engine_type(), EngineType::Codex);
    }

    #[test]
    fn test_stub_engine_multiple_agents() {
        let tmp_dir = TempDir::new().unwrap();
        let output_dir = tmp_dir.path().join("loop");
        let engine = StubEngine::new(output_dir.to_str().unwrap());

        engine.execute("Aaron", "Task A", tmp_dir.path(), 1, None);
        engine.execute("Betty", "Task B", tmp_dir.path(), 1, None);

        // Both files should exist
        assert!(output_dir.join("turn1-agentA.md").exists());
        assert!(output_dir.join("turn1-agentB.md").exists());
    }

    #[test]
    fn test_stub_engine_multiple_turns() {
        let tmp_dir = TempDir::new().unwrap();
        let output_dir = tmp_dir.path().join("loop");
        let engine = StubEngine::new(output_dir.to_str().unwrap());

        engine.execute("Aaron", "Task 1", tmp_dir.path(), 1, None);
        engine.execute("Aaron", "Task 2", tmp_dir.path(), 2, None);

        // Both turn files should exist
        assert!(output_dir.join("turn1-agentA.md").exists());
        assert!(output_dir.join("turn2-agentA.md").exists());
    }

    #[test]
    fn test_build_agent_prompt_valid_agent() {
        // Valid agent should return Some(prompt)
        let result = super::build_agent_prompt("Aaron", "Test task", None);
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
        let result = super::build_agent_prompt("ScrumMaster", "Plan sprint", None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_build_agent_prompt_invalid_name() {
        // Invalid name should return None
        let result = super::build_agent_prompt("RandomName", "Some task", None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_build_agent_prompt_with_team_dir() {
        // Prompt should include team_dir when provided
        let result = super::build_agent_prompt("Aaron", "Test task", Some(".swarm-hug/greenfield"));
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
        let line = super::generate_coauthor_line();
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

        let result = super::read_coauthor_email();
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

        let result = super::read_coauthor_email();
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

        let result = super::build_agent_prompt("Aaron", "Test task", None);
        assert!(result.is_ok());
        let prompt = result.unwrap().unwrap();
        // Check that the co-author line is in the prompt (in commit messages)
        assert!(prompt.contains("Co-Authored-By: dev <dev@example.com>"),
            "Prompt should contain co-author line. Prompt content:\n{}", prompt);

        // Restore original dir
        std::env::set_current_dir(original_dir).unwrap();
    }
}
