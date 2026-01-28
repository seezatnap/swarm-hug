use std::collections::HashMap;
use std::path::Path;

use crate::config::EngineType;
use crate::engine::{self, Engine, EngineResult};
use crate::prompt;

/// Generate the merge agent prompt for feature-to-target branch merges.
pub fn generate_merge_agent_prompt(
    feature_branch: &str,
    target_branch: &str,
) -> Result<String, String> {
    let feature = normalize_branch("feature", feature_branch)?;
    let target = normalize_branch("target", target_branch)?;

    let mut vars = HashMap::new();
    vars.insert("feature_branch", feature);
    vars.insert("target_branch", target);
    vars.insert("co_author", engine::coauthor_line());

    prompt::load_and_render("merge_agent", &vars)
}

/// Run the merge agent to merge a feature branch into the target branch.
///
/// Returns the engine result so callers can inspect success and output.
pub fn run_merge_agent(
    engine: &dyn Engine,
    feature_branch: &str,
    target_branch: &str,
    repo_root: &Path,
) -> Result<EngineResult, String> {
    let prompt = generate_merge_agent_prompt(feature_branch, target_branch)?;

    if engine.engine_type() == EngineType::Stub {
        let message = format!(
            "Stub merge agent: {} -> {}",
            feature_branch.trim(),
            target_branch.trim()
        );
        return Ok(EngineResult::success(message));
    }

    Ok(engine.execute("MergeAgent", &prompt, repo_root, 0, None))
}

fn normalize_branch(label: &str, branch: &str) -> Result<String, String> {
    let trimmed = branch.trim();
    if trimmed.is_empty() {
        return Err(format!("{} branch name is empty", label));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    use crate::engine::StubEngine;
    use crate::testutil::with_temp_cwd;

    #[test]
    fn test_generate_merge_agent_prompt_renders_vars() {
        with_temp_cwd(|| {
            fs::create_dir_all(".swarm-hug").unwrap();
            fs::write(".swarm-hug/email.txt", "dev@example.com").unwrap();

            let prompt = generate_merge_agent_prompt("feature-1", "main").unwrap();
            assert!(prompt.contains("feature-1"));
            assert!(prompt.contains("main"));
            assert!(prompt.contains("Co-Authored-By: dev <dev@example.com>"));
            assert!(!prompt.contains("{{feature_branch}}"));
            assert!(!prompt.contains("{{target_branch}}"));
        });
    }

    #[test]
    fn test_generate_merge_agent_prompt_rejects_empty_branch() {
        assert!(generate_merge_agent_prompt("", "main").is_err());
        assert!(generate_merge_agent_prompt("feature", " ").is_err());
    }

    #[test]
    fn test_run_merge_agent_stub() {
        with_temp_cwd(|| {
            let engine = StubEngine::new("loop");
            let result = run_merge_agent(&engine, "feature-x", "main", Path::new("."))
                .expect("run merge agent");
            assert!(result.success);
            assert!(result.output.contains("Stub merge agent"));
        });
    }
}
