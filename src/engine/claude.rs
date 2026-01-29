use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::config::EngineType;
use crate::process_group::spawn_in_new_process_group;
use crate::process_registry::PROCESS_REGISTRY;

use super::{Engine, EngineResult};
use super::util::{build_agent_prompt, output_to_result, resolve_cli_path, WAIT_LOG_INTERVAL_SECS};

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

        let mut child = match spawn_in_new_process_group(&mut cmd) {
            Ok(c) => c,
            Err(e) => return EngineResult::failure(format!("failed to spawn claude: {}", e), 1),
        };
        let pid = child.id();
        PROCESS_REGISTRY.register(pid);

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }

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
                        Ok(output) => {
                            let result = output_to_result(output);
                            PROCESS_REGISTRY.unregister(pid);
                            return result;
                        }
                        Err(e) => {
                            PROCESS_REGISTRY.unregister(pid);
                            return EngineResult::failure(format!("failed to get output: {}", e), 1);
                        }
                    }
                }
                Ok(None) => {
                    // Process still running
                    let elapsed = start.elapsed();

                    // Check for timeout
                    if let Some(timeout_duration) = timeout {
                        if elapsed >= timeout_duration {
                            let _ = child.kill();
                            let _ = child.wait();
                            PROCESS_REGISTRY.unregister(pid);
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
                    let _ = child.wait();
                    PROCESS_REGISTRY.unregister(pid);
                    return EngineResult::failure(format!("failed to wait for claude: {}", e), 1);
                }
            }
        }
    }

    fn engine_type(&self) -> EngineType {
        EngineType::Claude
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_engine_type() {
        let engine = ClaudeEngine::new();
        assert_eq!(engine.engine_type(), EngineType::Claude);
    }

    #[test]
    fn test_claude_engine_with_timeout() {
        let engine = ClaudeEngine::with_timeout(1800);
        assert_eq!(engine.timeout_secs, 1800);
        assert_eq!(engine.engine_type(), EngineType::Claude);
    }
}
