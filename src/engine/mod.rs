//! Engine abstraction for agent execution.
//!
//! Supports multiple backends:
//! - `claude`: Claude CLI
//! - `codex`: Codex CLI
//! - `openrouter_<model>`: Claude CLI via OpenRouter
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

/// Get the configured co-author line for commit messages.
pub(crate) fn coauthor_line() -> String {
    util::generate_coauthor_line()
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

/// Create an engine from config.
/// Returns Arc for thread-safe sharing across parallel agent execution.
pub fn create_engine(engine_type: EngineType, output_dir: &str, timeout_secs: u64) -> Arc<dyn Engine> {
    match engine_type {
        EngineType::Claude => Arc::new(ClaudeEngine::with_timeout(timeout_secs)),
        EngineType::Codex => Arc::new(CodexEngine::with_timeout(timeout_secs)),
        EngineType::OpenRouter { model } => {
            Arc::new(ClaudeEngine::with_timeout(timeout_secs).with_openrouter_model(model))
        }
        EngineType::Stub => Arc::new(StubEngine::new(output_dir)),
    }
}

/// Create an engine with random selection from a list of engine types.
///
/// This function encapsulates the per-task engine selection logic:
/// - If `stub_mode` is true, always returns a Stub engine
/// - If the engine list is empty, defaults to Claude
/// - If the engine list has one entry, uses that engine
/// - If the engine list has multiple entries, randomly selects one
///
/// Returns a tuple of (engine, selected_engine_type) so callers can log
/// which engine was selected.
pub fn create_random_engine(
    engine_types: &[EngineType],
    stub_mode: bool,
    output_dir: &str,
    timeout_secs: u64,
) -> (Arc<dyn Engine>, EngineType) {
    let selected_type = select_engine_type(engine_types, stub_mode);
    let engine = create_engine(selected_type.clone(), output_dir, timeout_secs);
    (engine, selected_type)
}

/// Select an engine type from the configured list with equal probability.
///
/// This is the core random selection helper that implements per-task engine selection:
/// - If `stub_mode` is true, always returns `Stub` regardless of the list
/// - If the list is empty, defaults to `Claude`
/// - If the list has one entry, returns that entry
/// - If the list has multiple entries, randomly selects one with equal probability
///
/// # Arguments
/// * `engine_types` - List of available engine types
/// * `stub_mode` - If true, always return Stub regardless of the list
///
/// # Returns
/// The selected engine type
///
/// # Example
/// ```
/// use swarm::engine::select_engine_type;
/// use swarm::config::EngineType;
///
/// // With multiple engines, selection is random
/// let types = vec![EngineType::Claude, EngineType::Codex];
/// let selected = select_engine_type(&types, false);
/// // selected is either Claude or Codex with equal probability
///
/// // Stub mode always returns Stub
/// let selected = select_engine_type(&types, true);
/// assert_eq!(selected, EngineType::Stub);
/// ```
pub fn select_engine_type(engine_types: &[EngineType], stub_mode: bool) -> EngineType {
    if stub_mode {
        EngineType::Stub
    } else if engine_types.is_empty() {
        EngineType::Claude
    } else if engine_types.len() == 1 {
        engine_types[0].clone()
    } else {
        use rand::seq::SliceRandom;
        engine_types.choose(&mut rand::thread_rng()).cloned().unwrap()
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

    #[test]
    fn test_create_engine_openrouter() {
        let engine = create_engine(
            EngineType::OpenRouter { model: "moonshotai/kimi-k2.5".to_string() },
            "loop",
            3600,
        );
        assert_eq!(
            engine.engine_type(),
            EngineType::OpenRouter { model: "moonshotai/kimi-k2.5".to_string() }
        );
    }

    #[test]
    fn test_select_engine_type_stub_mode() {
        // Stub mode always returns Stub regardless of the list
        let types = vec![EngineType::Claude, EngineType::Codex];
        assert_eq!(select_engine_type(&types, true), EngineType::Stub);
    }

    #[test]
    fn test_select_engine_type_empty_list() {
        // Empty list defaults to Claude
        assert_eq!(select_engine_type(&[], false), EngineType::Claude);
    }

    #[test]
    fn test_select_engine_type_single_entry() {
        // Single entry returns that entry
        assert_eq!(select_engine_type(&[EngineType::Codex], false), EngineType::Codex);
        assert_eq!(select_engine_type(&[EngineType::Claude], false), EngineType::Claude);
    }

    #[test]
    fn test_select_engine_type_multiple_entries() {
        // Multiple entries should return one of them
        let types = vec![EngineType::Claude, EngineType::Codex];
        for _ in 0..20 {
            let selected = select_engine_type(&types, false);
            assert!(selected == EngineType::Claude || selected == EngineType::Codex);
        }
    }

    #[test]
    fn test_create_random_engine_stub_mode() {
        let types = vec![EngineType::Claude, EngineType::Codex];
        let (engine, selected_type) = create_random_engine(&types, true, "loop", 3600);
        assert_eq!(engine.engine_type(), EngineType::Stub);
        assert_eq!(selected_type, EngineType::Stub);
    }

    #[test]
    fn test_create_random_engine_empty_list() {
        let (engine, selected_type) = create_random_engine(&[], false, "loop", 3600);
        assert_eq!(engine.engine_type(), EngineType::Claude);
        assert_eq!(selected_type, EngineType::Claude);
    }

    #[test]
    fn test_create_random_engine_single_entry() {
        let (engine, selected_type) = create_random_engine(&[EngineType::Codex], false, "loop", 3600);
        assert_eq!(engine.engine_type(), EngineType::Codex);
        assert_eq!(selected_type, EngineType::Codex);
    }

    #[test]
    fn test_create_random_engine_returns_matching_type() {
        // Verify the returned engine type matches the selected type
        let types = vec![EngineType::Claude, EngineType::Codex];
        for _ in 0..20 {
            let (engine, selected_type) = create_random_engine(&types, false, "loop", 3600);
            assert_eq!(engine.engine_type(), selected_type);
        }
    }
}
