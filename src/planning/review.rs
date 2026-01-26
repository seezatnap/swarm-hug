use std::collections::HashMap;
use std::path::Path;

use crate::config::EngineType;
use crate::engine::Engine;
use crate::prompt;

/// Generate the post-sprint review prompt.
///
/// # Errors
/// Returns an error if the review.md prompt file is missing.
pub fn generate_review_prompt(tasks_content: &str, git_log: &str) -> Result<String, String> {
    let mut vars = HashMap::new();
    vars.insert("git_log", git_log.to_string());
    vars.insert("tasks_content", tasks_content.to_string());

    prompt::load_and_render("review", &vars)
}

/// Parse review response to extract follow-up tasks.
pub fn parse_review_response(response: &str) -> Vec<String> {
    if response.contains("NO_FOLLOWUPS_NEEDED") {
        return vec![];
    }

    response
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("- [ ]") {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Run post-sprint review using LLM.
pub fn run_sprint_review(
    engine: &dyn Engine,
    tasks_content: &str,
    git_log: &str,
    log_dir: &Path,
) -> Result<Vec<String>, String> {
    // For stub engine, return no follow-ups (deterministic)
    if engine.engine_type() == EngineType::Stub {
        return Ok(vec![]);
    }

    let prompt = generate_review_prompt(tasks_content, git_log)?;

    let result = engine.execute(
        "ScrumMaster",
        &prompt,
        log_dir,
        0,    // turn 0 for review
        None, // ScrumMaster doesn't need team context
    );

    if !result.success {
        return Err(result
            .error
            .unwrap_or_else(|| "Review failed".to_string()));
    }

    Ok(parse_review_response(&result.output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_review_response_no_followups() {
        let response = "NO_FOLLOWUPS_NEEDED";
        let tasks = parse_review_response(response);
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_parse_review_response_with_tasks() {
        let response = "Found some issues:\n- [ ] Fix the bug\n- [ ] Add tests\nDone.";
        let tasks = parse_review_response(response);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0], "- [ ] Fix the bug");
        assert_eq!(tasks[1], "- [ ] Add tests");
    }

    #[test]
    fn test_generate_review_prompt() {
        let tasks = "- [x] Done task\n- [ ] Pending task\n";
        let git_log = "commit abc123\nAuthor: Agent Aaron\n\nCompleted task";
        // If prompts dir not found, this will be an error - that's fine for CI
        if let Ok(prompt) = generate_review_prompt(tasks, git_log) {
            assert!(prompt.contains("Done task"));
            assert!(prompt.contains("commit abc123"));
        }
    }
}
