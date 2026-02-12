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
                normalize_follow_up_description(trimmed)
            } else {
                None
            }
        })
        .collect()
}

/// Format follow-up tasks in PRD-to-task format with sequential numbering.
pub fn format_follow_up_tasks(start_number: usize, follow_ups: &[String]) -> Vec<String> {
    let mut task_number = start_number;
    let mut formatted = Vec::new();

    for follow_up in follow_ups {
        if let Some(desc) = normalize_follow_up_description(follow_up) {
            formatted.push(format!("- [ ] (#{}) {}", task_number, desc));
            task_number += 1;
        }
    }

    formatted
}

fn normalize_follow_up_description(text: &str) -> Option<String> {
    let mut rest = text.trim();
    if rest.starts_with("- [ ]") {
        rest = rest.trim_start_matches("- [ ]").trim();
    }
    let rest = strip_task_number_prefix(rest).trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

fn strip_task_number_prefix(text: &str) -> &str {
    let trimmed = text.trim_start();
    let Some(after_prefix) = trimmed.strip_prefix("(#") else {
        return trimmed;
    };

    let mut digits_len = 0;
    for ch in after_prefix.chars() {
        if ch.is_ascii_digit() {
            digits_len += ch.len_utf8();
        } else {
            break;
        }
    }

    if digits_len == 0 {
        return trimmed;
    }

    let Some(after_digits) = after_prefix.get(digits_len..) else {
        return trimmed;
    };

    if let Some(stripped) = after_digits.strip_prefix(')') {
        return stripped.trim_start();
    }

    trimmed
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
        return Err(result.error.unwrap_or_else(|| "Review failed".to_string()));
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
        let response =
            "Found some issues:\n- [ ] Fix the bug\n- [ ] (#9) Add tests (blocked by #2)\nDone.";
        let tasks = parse_review_response(response);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0], "Fix the bug");
        assert_eq!(tasks[1], "Add tests (blocked by #2)");
    }

    #[test]
    fn test_format_follow_up_tasks_numbers_and_preserves_blockers() {
        let follow_ups = vec![
            "Fix the bug".to_string(),
            "- [ ] (#5) Add tests (blocked by #2)".to_string(),
        ];
        let formatted = format_follow_up_tasks(7, &follow_ups);
        assert_eq!(formatted.len(), 2);
        assert_eq!(formatted[0], "- [ ] (#7) Fix the bug");
        assert_eq!(formatted[1], "- [ ] (#8) Add tests (blocked by #2)");
    }

    #[test]
    fn test_follow_up_tasks_use_prd_format_and_sequential_numbers() {
        let response = "- [ ] Investigate timeouts (blocked by #2, #3)\n- [ ] (#9) Write docs";
        let follow_ups = parse_review_response(response);
        let formatted = format_follow_up_tasks(12, &follow_ups);
        assert_eq!(formatted.len(), 2);
        assert_eq!(
            formatted[0],
            "- [ ] (#12) Investigate timeouts (blocked by #2, #3)"
        );
        assert_eq!(formatted[1], "- [ ] (#13) Write docs");
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
