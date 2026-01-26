//! Engine abstraction for agent execution.
//!
//! Supports multiple backends:
//! - `claude`: Claude CLI
//! - `codex`: Codex CLI
//! - `stub`: Deterministic stub for tests (no network)

use std::path::Path;
use std::sync::Arc;

use crate::config::EngineType;

mod claude;
mod codex;
mod stub;
mod util;

pub use claude::ClaudeEngine;
pub use codex::CodexEngine;
pub use stub::StubEngine;

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
}
