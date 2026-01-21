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

/// Default agent prompt (fallback if prompts/agent.md not found).
const DEFAULT_AGENT_PROMPT: &str = r#"You are agent {{agent_name}}. Complete the following task:

{{task_description}}

## Git workflow (CRITICAL)
You are working in a dedicated git worktree on branch `agent/{{agent_name_lower}}`.

After completing your task:
1. Stage all your changes: `git add -A`
2. Commit with your agent name:
   ```bash
   git commit -m "{{task_short}}" --author="Agent {{agent_name}} <agent-{{agent_initial}}@swarm.local>"
   ```
3. Find the main repo and merge your branch:
   ```bash
   MAIN_REPO=$(git rev-parse --path-format=absolute --git-common-dir | sed 's|/.git$||')
   git -C "$MAIN_REPO" merge --no-ff agent/{{agent_name_lower}} -m "Merge Agent {{agent_name}}: {{task_short}}"
   ```
4. Reset your branch to match master for the next task:
   ```bash
   git fetch origin master:master 2>/dev/null || git fetch origin main:main 2>/dev/null
   git reset --hard master 2>/dev/null || git reset --hard main
   ```

All steps are REQUIRED so your work is integrated and you're ready for the next task."#;

/// Build the agent prompt with variable substitution.
fn build_agent_prompt(agent_name: &str, task_description: &str) -> String {
    let task_short = if task_description.len() > 50 {
        format!("{}...", &task_description[..47])
    } else {
        task_description.to_string()
    };

    // Get initial from agent name
    let agent_initial = crate::agent::initial_from_name(agent_name)
        .map(|c| c.to_string())
        .unwrap_or_else(|| "?".to_string());

    let mut vars = HashMap::new();
    vars.insert("agent_name", agent_name.to_string());
    vars.insert("task_description", task_description.to_string());
    vars.insert("agent_name_lower", agent_name.to_lowercase());
    vars.insert("agent_initial", agent_initial);
    vars.insert("task_short", task_short);

    prompt::load_and_render("agent", &vars, DEFAULT_AGENT_PROMPT)
}

impl Engine for ClaudeEngine {
    fn execute(
        &self,
        agent_name: &str,
        task_description: &str,
        working_dir: &Path,
        _turn_number: usize,
    ) -> EngineResult {
        let prompt = build_agent_prompt(agent_name, task_description);

        let result = Command::new(&self.cli_path)
            .arg("--dangerously-skip-permissions")
            .arg("--print")
            .arg(&prompt)
            .current_dir(working_dir)
            .stdin(Stdio::null())
            .output();

        match result {
            Ok(output) => output_to_result(output),
            Err(e) => EngineResult::failure(format!("failed to execute claude: {}", e), 1),
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
        let prompt = build_agent_prompt(agent_name, task_description);

        // Codex uses "exec" subcommand with stdin for prompts
        let result = Command::new(&self.cli_path)
            .arg("exec")
            .arg("--dangerously-bypass-approvals-and-sandbox")
            .arg("-")  // Read prompt from stdin
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match result {
            Ok(mut child) => {
                // Write prompt to stdin
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin.write_all(prompt.as_bytes());
                }
                // Wait for completion
                match child.wait_with_output() {
                    Ok(output) => output_to_result(output),
                    Err(e) => EngineResult::failure(format!("codex wait failed: {}", e), 1),
                }
            }
            Err(e) => EngineResult::failure(format!("failed to execute codex: {}", e), 1),
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
}
