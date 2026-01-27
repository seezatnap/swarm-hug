use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::config::EngineType;
use crate::engine::Engine;
use crate::prompt;

/// Result of PRD to tasks conversion.
#[derive(Debug)]
pub struct PrdConversionResult {
    /// The generated tasks in markdown format.
    pub tasks_markdown: String,
    /// Raw LLM response (for debugging).
    pub raw_response: String,
    /// Whether the operation succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
}

impl PrdConversionResult {
    /// Create a successful result.
    pub fn success(tasks_markdown: String, raw_response: String) -> Self {
        Self {
            tasks_markdown,
            raw_response,
            success: true,
            error: None,
        }
    }

    /// Create a failed result.
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            tasks_markdown: String::new(),
            raw_response: String::new(),
            success: false,
            error: Some(error.into()),
        }
    }
}

/// Generate the PRD-to-tasks conversion prompt.
///
/// # Errors
/// Returns an error if the prd_to_tasks.md prompt file is missing.
pub fn generate_prd_prompt(prd_content: &str) -> Result<String, String> {
    let mut vars = HashMap::new();
    vars.insert("prd_content", prd_content.to_string());

    prompt::load_and_render("prd_to_tasks", &vars)
}

/// Parse the response from PRD conversion to extract the tasks markdown.
///
/// The response should already be in markdown format with sections and tasks.
/// We clean it up by removing any markdown code fences if present.
pub fn parse_prd_response(response: &str) -> String {
    let mut result = response.to_string();

    // Remove markdown code fences if present
    if ["```markdown", "```md", "```"]
        .iter()
        .any(|prefix| result.starts_with(prefix))
    {
        if let Some(first_newline) = result.find('\n') {
            result = result[first_newline + 1..].to_string();
        }
    }

    // Remove trailing code fence
    if result.trim_end().ends_with("```") {
        if let Some(last_fence) = result.rfind("```") {
            result = result[..last_fence].to_string();
        }
    }

    result.trim().to_string()
}

/// Convert a PRD document to a task list using LLM.
///
/// Uses the engine to intelligently break down a PRD into actionable tasks,
/// organized by work area and sized at approximately 3 story points each.
pub fn convert_prd_to_tasks(
    engine: &dyn Engine,
    prd_content: &str,
    log_dir: &Path,
) -> PrdConversionResult {
    // For stub engine, return deterministic stub tasks
    if engine.engine_type() == EngineType::Stub {
        return stub_prd_conversion(prd_content);
    }

    let prompt = match generate_prd_prompt(prd_content) {
        Ok(p) => p,
        Err(e) => return PrdConversionResult::failure(e),
    };

    let result = engine.execute(
        "ScrumMaster",
        &prompt,
        log_dir,
        0,    // turn 0 for PRD conversion
        None, // ScrumMaster doesn't need team context
    );

    if !result.success {
        return PrdConversionResult::failure(
            result
                .error
                .unwrap_or_else(|| "PRD conversion failed".to_string()),
        );
    }

    let tasks_markdown = parse_prd_response(&result.output);

    // Validate that we got some tasks
    if !tasks_markdown.contains("- [ ]") {
        // Log the failed response for debugging
        let debug_path = log_dir.join("prd_conversion_response.log");
        let _ = fs::write(&debug_path, &result.output);
        return PrdConversionResult::failure("No tasks found in LLM response");
    }

    PrdConversionResult::success(tasks_markdown, result.output)
}

/// Generate stub PRD conversion (deterministic for testing).
fn stub_prd_conversion(prd_content: &str) -> PrdConversionResult {
    // Generate a simple task list based on the PRD content
    let lines: Vec<&str> = prd_content.lines().collect();
    let word_count = prd_content.split_whitespace().count();

    // Generate a number of tasks proportional to PRD length
    let task_count = (word_count / 50).clamp(3, 10);

    let mut tasks = String::new();
    tasks.push_str("## Implementation\n\n");

    let mut task_num = 1;
    for i in 1..=task_count {
        tasks.push_str(&format!("- [ ] (#{})", task_num));
        tasks.push_str(&format!(" Implement feature {} from PRD\n", i));
        task_num += 1;
    }

    tasks.push_str("\n## Testing\n\n");
    // Testing tasks depend on the first implementation task
    tasks.push_str(&format!("- [ ] (#{})", task_num));
    tasks.push_str(" Write unit tests for new features (blocked by #1)\n");
    task_num += 1;
    tasks.push_str(&format!("- [ ] (#{})", task_num));
    tasks.push_str(&format!(" Write integration tests (blocked by #{})\n", task_num - 1));

    // Include first non-empty line from PRD as context in response
    let first_line = lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .copied()
        .unwrap_or("(empty PRD)");

    let response = format!(
        "Generated tasks from PRD starting with: {}\n\n{}",
        first_line, tasks
    );

    PrdConversionResult::success(tasks, response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_prd_prompt() {
        let prd = "# My Feature\n\nThis is a product requirement.";
        let result = generate_prd_prompt(prd);
        // If prompts dir not found, this will be an error - that's fine for CI
        if let Ok(prompt) = result {
            assert!(prompt.contains("My Feature"));
            assert!(prompt.contains("product requirement"));
            assert!(prompt.contains("3 story points"));
        }
    }

    #[test]
    fn test_parse_prd_response_clean() {
        let response = "## Backend\n\n- [ ] Task one\n- [ ] Task two";
        let result = parse_prd_response(response);
        assert_eq!(result, "## Backend\n\n- [ ] Task one\n- [ ] Task two");
    }

    #[test]
    fn test_parse_prd_response_with_code_fence() {
        let response = "```markdown\n## Backend\n\n- [ ] Task one\n```";
        let result = parse_prd_response(response);
        assert_eq!(result, "## Backend\n\n- [ ] Task one");
    }

    #[test]
    fn test_parse_prd_response_with_md_fence() {
        let response = "```md\n## Backend\n\n- [ ] Task one\n```";
        let result = parse_prd_response(response);
        assert_eq!(result, "## Backend\n\n- [ ] Task one");
    }

    #[test]
    fn test_parse_prd_response_with_plain_fence() {
        let response = "```\n## Backend\n\n- [ ] Task one\n```";
        let result = parse_prd_response(response);
        assert_eq!(result, "## Backend\n\n- [ ] Task one");
    }

    #[test]
    fn test_stub_prd_conversion() {
        let prd = "# Feature X\n\nThis is a long description of the feature that spans multiple words and lines.\nIt should generate several tasks based on the content length.";
        let result = stub_prd_conversion(prd);

        assert!(result.success);
        assert!(result.tasks_markdown.contains("## Implementation"));
        assert!(result.tasks_markdown.contains("## Testing"));
        assert!(result.tasks_markdown.contains("- [ ] (#"));
        // Check that blocking info is present
        assert!(result.tasks_markdown.contains("(blocked by #"));
    }

    #[test]
    fn test_stub_prd_conversion_short_prd() {
        let prd = "# Short\n\nBrief description.";
        let result = stub_prd_conversion(prd);

        assert!(result.success);
        // Should still generate minimum 3 implementation tasks with numbers
        assert!(result.tasks_markdown.contains("(#1)"));
        assert!(result.tasks_markdown.contains("(#2)"));
        assert!(result.tasks_markdown.contains("(#3)"));
        assert!(result.tasks_markdown.contains("Implement feature 1"));
        assert!(result.tasks_markdown.contains("Implement feature 2"));
        assert!(result.tasks_markdown.contains("Implement feature 3"));
    }

    #[test]
    fn test_prd_conversion_result_success() {
        let result = PrdConversionResult::success(
            "## Tasks\n- [ ] Task".to_string(),
            "raw response".to_string(),
        );
        assert!(result.success);
        assert!(result.error.is_none());
        assert_eq!(result.tasks_markdown, "## Tasks\n- [ ] Task");
    }

    #[test]
    fn test_prd_conversion_result_failure() {
        let result = PrdConversionResult::failure("something went wrong");
        assert!(!result.success);
        assert_eq!(result.error, Some("something went wrong".to_string()));
        assert!(result.tasks_markdown.is_empty());
    }
}
