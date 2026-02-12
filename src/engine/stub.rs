use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::config::EngineType;

use super::{Engine, EngineResult};

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
        format!(
            "{}/turn{}-agent{}.md",
            self.output_dir, turn_number, agent_initial
        )
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
        let initial = crate::agent::initial_from_name(agent_name).unwrap_or('?');

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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_stub_engine_execute() {
        let tmp_dir = TempDir::new().unwrap();
        let output_dir = tmp_dir.path().join("loop");
        let engine = StubEngine::new(output_dir.to_str().unwrap());

        let result = engine.execute("Aaron", "Write tests", tmp_dir.path(), 1, None);

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
}
