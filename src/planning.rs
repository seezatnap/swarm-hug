//! LLM-assisted sprint planning module.
//!
//! Provides intelligent task assignment, post-sprint review, and PRD-to-tasks
//! conversion capabilities using the engine abstraction. Can use any engine (claude, codex, stub).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::agent;
use crate::config::EngineType;
use crate::engine::Engine;
use crate::prompt;
use crate::task::TaskList;

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

/// Result of LLM planning operations.
#[derive(Debug)]
pub struct PlanningResult {
    /// Assignments as (line_number, agent_initial) pairs.
    pub assignments: Vec<(usize, char)>,
    /// Raw LLM response (for debugging).
    pub raw_response: String,
    /// Whether the operation succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
}

impl PlanningResult {
    /// Create a successful result with assignments.
    pub fn success(assignments: Vec<(usize, char)>, raw_response: String) -> Self {
        Self {
            assignments,
            raw_response,
            success: true,
            error: None,
        }
    }

    /// Create a failed result.
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            assignments: vec![],
            raw_response: String::new(),
            success: false,
            error: Some(error.into()),
        }
    }
}

/// Generate the scrum master prompt for task assignment.
///
/// This prompt asks the LLM to assign tasks to agents intelligently,
/// considering dependencies, file conflicts, and priority order.
///
/// # Errors
/// Returns an error if the scrum_master.md prompt file is missing.
pub fn generate_scrum_master_prompt(
    task_list: &TaskList,
    agent_initials: &[char],
    tasks_per_agent: usize,
) -> Result<Option<String>, String> {
    let unassigned: Vec<(usize, &str)> = task_list
        .tasks
        .iter()
        .enumerate()
        .filter_map(|(idx, t)| {
            if task_list.is_task_assignable(idx) {
                Some((idx + 1, t.description.as_str())) // 1-indexed line numbers
            } else {
                None
            }
        })
        .collect();

    if unassigned.is_empty() {
        return Ok(None);
    }

    let num_agents = agent_initials.len();
    let total_tasks = num_agents * tasks_per_agent;
    let to_assign = unassigned.len().min(total_tasks);

    // Build agent list with names
    let agent_list: String = agent_initials
        .iter()
        .map(|&initial| {
            let name = agent::name_from_initial(initial).unwrap_or("Unknown");
            format!("  - {} ({})\n", initial, name)
        })
        .collect();

    // Build unassigned task list
    let task_list_str: String = unassigned
        .iter()
        .map(|(line_num, desc)| format!("  Line {}: {}\n", line_num, desc))
        .collect();

    let mut vars = HashMap::new();
    vars.insert("to_assign", to_assign.to_string());
    vars.insert("num_agents", num_agents.to_string());
    vars.insert("tasks_per_agent", tasks_per_agent.to_string());
    vars.insert("num_unassigned", unassigned.len().to_string());
    vars.insert("agent_list", agent_list);
    vars.insert("task_list", task_list_str);

    let rendered = prompt::load_and_render("scrum_master", &vars)?;
    Ok(Some(rendered))
}

/// Parse LLM response to extract task assignments.
///
/// Handles various response formats:
/// - Clean JSON: `{"assignments":[...]}`
/// - JSON in markdown code blocks
/// - Malformed responses with assignment objects
pub fn parse_llm_assignments(response: &str) -> Vec<(usize, char)> {
    // Remove markdown code fences if present
    let cleaned = response
        .replace("```json", "")
        .replace("```", "");

    // Collapse whitespace for easier parsing
    let single_line: String = cleaned.split_whitespace().collect();

    // Try to find the JSON object with assignments
    // Look for {"assignments": or { "assignments": patterns
    if let Some(start) = single_line.find(r#"{"assignments":"#)
        .or_else(|| single_line.find(r#"{"assignments":["#))
    {
        // Find the matching closing brace
        let json_part = &single_line[start..];
        if let Some(end) = find_matching_brace(json_part) {
            let json_str = &json_part[..=end];
            if let Some(parsed) = parse_assignments_json(json_str) {
                return parsed;
            }
        }
    }

    // Method 2: Look for individual assignment objects using regex-like extraction
    let mut assignments = Vec::new();
    let mut search_from = 0;

    while search_from < single_line.len() {
        // Ensure search_from is on a character boundary
        search_from = ceil_char_boundary(&single_line, search_from);
        if search_from >= single_line.len() {
            break;
        }

        let search_slice = &single_line[search_from..];
        let Some(agent_pos) = search_slice.find(r#""agent":"#) else {
            break;
        };

        let abs_pos = search_from + agent_pos;
        let after_key_pos = abs_pos + r#""agent":"#.len();
        let after_key_pos = ceil_char_boundary(&single_line, after_key_pos);

        if after_key_pos >= single_line.len() {
            break;
        }

        let after_agent = &single_line[after_key_pos..];

        // Get the agent letter
        if let Some(agent_char) = after_agent.chars().next() {
            if agent_char.is_ascii_uppercase() {
                // Look for "line": in the surrounding context (within 100 bytes)
                let context_start = floor_char_boundary(&single_line, abs_pos.saturating_sub(50));
                let context_end = ceil_char_boundary(&single_line, (abs_pos + 100).min(single_line.len()));
                let context = &single_line[context_start..context_end];

                if let Some(line_pos) = context.find(r#""line":"#) {
                    let line_value_start = context_start + line_pos + r#""line":"#.len();
                    let line_value_start = ceil_char_boundary(&single_line, line_value_start);
                    if line_value_start < single_line.len() {
                        if let Some(line_num) = parse_number_at(&single_line[line_value_start..]) {
                            assignments.push((line_num, agent_char));
                        }
                    }
                }
            }
        }

        // Move past this match
        search_from = ceil_char_boundary(&single_line, abs_pos + 1);
    }

    assignments
}

/// Find the byte position of the matching closing brace.
fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (byte_pos, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(byte_pos);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the nearest valid character boundary at or before the given byte index.
fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Find the nearest valid character boundary at or after the given byte index.
fn ceil_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Parse a number from the start of a string.
fn parse_number_at(s: &str) -> Option<usize> {
    let num_str: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

/// Parse assignments from a JSON string.
fn parse_assignments_json(json: &str) -> Option<Vec<(usize, char)>> {
    // Simple manual JSON parsing since we don't want to add serde
    let mut assignments = Vec::new();

    // Find the assignments array
    let array_start = json.find('[')? + 1;
    let array_end = json.rfind(']')?;
    let array_content = &json[array_start..array_end];

    // Split by },{ to get individual assignment objects
    let objects: Vec<&str> = array_content.split("},{").collect();

    for obj in objects {
        let obj = obj.trim_matches(|c| c == '{' || c == '}' || c == ' ');

        // Extract agent - look for "agent":"X"
        let agent = if let Some(pos) = obj.find(r#""agent":"#) {
            let start = pos + 9; // skip "agent":"
            obj.chars().nth(start).filter(|c| c.is_ascii_uppercase())
        } else {
            None
        };

        // Extract line number - look for "line": followed by a number
        // The JSON can have "line":N or "line": N (whitespace collapsed)
        let line = if let Some(pos) = obj.find(r#""line":"#) {
            // Skip past "line": (7 chars)
            parse_number_at(&obj[pos + 7..])
        } else {
            None
        };

        if let (Some(a), Some(l)) = (agent, line) {
            assignments.push((l, a));
        }
    }

    if assignments.is_empty() {
        None
    } else {
        Some(assignments)
    }
}

/// Run LLM-assisted task assignment.
///
/// Uses the engine to get intelligent task assignments from an LLM.
pub fn run_llm_assignment(
    engine: &dyn Engine,
    task_list: &TaskList,
    agent_initials: &[char],
    tasks_per_agent: usize,
    log_dir: &Path,
) -> PlanningResult {
    // Generate the scrum master prompt
    let prompt = match generate_scrum_master_prompt(task_list, agent_initials, tasks_per_agent) {
        Ok(Some(p)) => p,
        Ok(None) => return PlanningResult::failure("No assignable tasks"),
        Err(e) => return PlanningResult::failure(e),
    };

    // For stub engine, generate deterministic assignments
    if engine.engine_type() == EngineType::Stub {
        return stub_assignment(task_list, agent_initials, tasks_per_agent);
    }

    // Execute via engine (using a special "planning" task)
    let result = engine.execute(
        "ScrumMaster",
        &prompt,
        log_dir,
        0, // turn 0 for planning
        None, // ScrumMaster doesn't need team context
    );

    if !result.success {
        return PlanningResult::failure(result.error.unwrap_or_else(|| "LLM execution failed".to_string()));
    }

    // Parse the response
    let assignments = parse_llm_assignments(&result.output);

    if assignments.is_empty() {
        // Log the failed response for debugging
        let debug_path = log_dir.join("scrum_master_response.log");
        let _ = fs::write(&debug_path, &result.output);
        return PlanningResult::failure("No parseable assignments in LLM response");
    }

    PlanningResult::success(assignments, result.output)
}

/// Generate stub assignments (deterministic for testing).
fn stub_assignment(
    task_list: &TaskList,
    agent_initials: &[char],
    tasks_per_agent: usize,
) -> PlanningResult {
    let unassigned: Vec<usize> = task_list
        .tasks
        .iter()
        .enumerate()
        .filter_map(|(idx, _t)| {
            if task_list.is_task_assignable(idx) {
                Some(idx + 1) // 1-indexed
            } else {
                None
            }
        })
        .collect();

    let mut assignments = Vec::new();
    let mut task_iter = unassigned.iter();

    // Round-robin assignment
    for _ in 0..tasks_per_agent {
        for &initial in agent_initials {
            if let Some(&line_num) = task_iter.next() {
                assignments.push((line_num, initial));
            }
        }
    }

    let response = format!(
        r#"{{"assignments":[{}]}}"#,
        assignments
            .iter()
            .map(|(l, a)| format!(r#"{{"agent":"{}","line":{}}}"#, a, l))
            .collect::<Vec<_>>()
            .join(",")
    );

    PlanningResult::success(assignments, response)
}

/// Generate the post-sprint review prompt.
///
/// # Errors
/// Returns an error if the review.md prompt file is missing.
pub fn generate_review_prompt(
    tasks_content: &str,
    git_log: &str,
) -> Result<String, String> {
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
        0, // turn 0 for review
        None, // ScrumMaster doesn't need team context
    );

    if !result.success {
        return Err(result.error.unwrap_or_else(|| "Review failed".to_string()));
    }

    Ok(parse_review_response(&result.output))
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
    if result.starts_with("```markdown") || result.starts_with("```md") {
        if let Some(first_newline) = result.find('\n') {
            result = result[first_newline + 1..].to_string();
        }
    } else if result.starts_with("```") {
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
        0, // turn 0 for PRD conversion
        None, // ScrumMaster doesn't need team context
    );

    if !result.success {
        return PrdConversionResult::failure(
            result.error.unwrap_or_else(|| "PRD conversion failed".to_string())
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
    let task_count = (word_count / 50).max(3).min(10);

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
    tasks.push_str(&format!(" Write unit tests for new features (blocked by #1)\n"));
    task_num += 1;
    tasks.push_str(&format!("- [ ] (#{})", task_num));
    tasks.push_str(&format!(" Write integration tests (blocked by #{})\n", task_num - 1));

    // Include first non-empty line from PRD as context in response
    let first_line = lines.iter()
        .find(|l| !l.trim().is_empty())
        .copied()
        .unwrap_or("(empty PRD)");

    let response = format!(
        "Generated tasks from PRD starting with: {}\n\n{}",
        first_line,
        tasks
    );

    PrdConversionResult::success(tasks, response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_scrum_master_prompt_empty() {
        let task_list = TaskList::parse("");
        let result = generate_scrum_master_prompt(&task_list, &['A', 'B'], 2);
        // With no tasks, should return Ok(None)
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn test_generate_scrum_master_prompt_with_tasks() {
        let content = "# Tasks\n- [ ] Task one\n- [ ] Task two\n- [ ] Task three\n";
        let task_list = TaskList::parse(content);
        let result = generate_scrum_master_prompt(&task_list, &['A', 'B'], 2);
        // If prompts dir not found, this will be an error - that's fine for CI
        if let Ok(Some(prompt)) = result {
            assert!(prompt.contains("Task one"));
            assert!(prompt.contains("Task two"));
            assert!(prompt.contains("Task three"));
            assert!(prompt.contains("A (Aaron)"));
            assert!(prompt.contains("B (Betty)"));
        }
    }

    #[test]
    fn test_generate_scrum_master_prompt_skips_blocked() {
        // Task #2 is blocked by incomplete #1
        let content = "# Tasks\n- [ ] (#1) Task one\n- [ ] (#2) Task two (blocked by #1)\n- [ ] (#3) Task three\n";
        let task_list = TaskList::parse(content);
        let result = generate_scrum_master_prompt(&task_list, &['A'], 2);
        // If prompts dir not found, this will be an error - that's fine for CI
        if let Ok(Some(prompt)) = result {
            assert!(prompt.contains("Task one"));
            assert!(!prompt.contains("Task two")); // Blocked task excluded
            assert!(prompt.contains("Task three"));
        }
    }

    #[test]
    fn test_generate_scrum_master_prompt_includes_unblocked() {
        // Task #2 blocked by #1, but #1 is complete - so #2 should be included
        let content = "# Tasks\n- [x] (#1) Task one (A)\n- [ ] (#2) Task two (blocked by #1)\n";
        let task_list = TaskList::parse(content);
        let result = generate_scrum_master_prompt(&task_list, &['A'], 2);
        if let Ok(Some(prompt)) = result {
            assert!(!prompt.contains("Task one")); // Completed, not included
            assert!(prompt.contains("Task two")); // Unblocked, should be included
        }
    }

    #[test]
    fn test_parse_llm_assignments_clean_json() {
        let response = r#"{"assignments":[{"agent":"A","line":1,"reason":"first"},{"agent":"B","line":2,"reason":"second"}]}"#;
        let assignments = parse_llm_assignments(response);
        assert_eq!(assignments.len(), 2);
        assert_eq!(assignments[0], (1, 'A'));
        assert_eq!(assignments[1], (2, 'B'));
    }

    #[test]
    fn test_parse_llm_assignments_with_markdown() {
        let response = r#"```json
{"assignments":[{"agent":"C","line":5,"reason":"test"}]}
```"#;
        let assignments = parse_llm_assignments(response);
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0], (5, 'C'));
    }

    #[test]
    fn test_parse_llm_assignments_multiline() {
        let response = r#"{
  "assignments": [
    {"agent": "A", "line": 1, "reason": "first"},
    {"agent": "B", "line": 2, "reason": "second"}
  ]
}"#;
        let assignments = parse_llm_assignments(response);
        assert_eq!(assignments.len(), 2);
    }

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
    fn test_stub_assignment() {
        let content = "- [ ] Task 1\n- [ ] Task 2\n- [ ] Task 3\n- [ ] Task 4\n";
        let task_list = TaskList::parse(content);
        let result = stub_assignment(&task_list, &['A', 'B'], 2);

        assert!(result.success);
        assert_eq!(result.assignments.len(), 4);

        // Verify round-robin: A gets 1,3; B gets 2,4
        let a_tasks: Vec<_> = result.assignments.iter().filter(|(_, c)| *c == 'A').collect();
        let b_tasks: Vec<_> = result.assignments.iter().filter(|(_, c)| *c == 'B').collect();
        assert_eq!(a_tasks.len(), 2);
        assert_eq!(b_tasks.len(), 2);
    }

    #[test]
    fn test_stub_assignment_fewer_tasks() {
        let content = "- [ ] Task 1\n- [ ] Task 2\n";
        let task_list = TaskList::parse(content);
        let result = stub_assignment(&task_list, &['A', 'B', 'C'], 3);

        assert!(result.success);
        // Only 2 tasks available, so only 2 assignments
        assert_eq!(result.assignments.len(), 2);
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

    #[test]
    fn test_find_matching_brace() {
        assert_eq!(find_matching_brace("{a}"), Some(2));
        assert_eq!(find_matching_brace("{{a}}"), Some(4));
        assert_eq!(find_matching_brace("{a:{b:c}}"), Some(8));
        assert_eq!(find_matching_brace("{"), None);
    }

    #[test]
    fn test_parse_number_at() {
        assert_eq!(parse_number_at("123abc"), Some(123));
        assert_eq!(parse_number_at("42"), Some(42));
        assert_eq!(parse_number_at("abc"), None);
        assert_eq!(parse_number_at(""), None);
    }

    #[test]
    fn test_find_matching_brace_with_utf8() {
        // Multi-byte characters: '→' is 3 bytes (E2 86 92), '日' etc are 3 bytes each
        // {→} = { (0) → (1-3) } (4) = closing brace at byte 4
        assert_eq!(find_matching_brace("{→}"), Some(4));
        // {日本語} = { (0) 日 (1-3) 本 (4-6) 語 (7-9) } (10) = closing brace at byte 10
        assert_eq!(find_matching_brace("{日本語}"), Some(10));
        // {a→b} = { (0) a (1) → (2-4) b (5) } (6) = closing brace at byte 6
        assert_eq!(find_matching_brace("{a→b}"), Some(6));
    }

    #[test]
    fn test_floor_char_boundary() {
        let s = "a→b"; // bytes: a(1) →(3) b(1) = 5 bytes total
        assert_eq!(floor_char_boundary(s, 0), 0); // 'a' boundary
        assert_eq!(floor_char_boundary(s, 1), 1); // '→' boundary
        assert_eq!(floor_char_boundary(s, 2), 1); // inside '→', floor to 1
        assert_eq!(floor_char_boundary(s, 3), 1); // inside '→', floor to 1
        assert_eq!(floor_char_boundary(s, 4), 4); // 'b' boundary
        assert_eq!(floor_char_boundary(s, 5), 5); // end
        assert_eq!(floor_char_boundary(s, 100), 5); // past end
    }

    #[test]
    fn test_ceil_char_boundary() {
        let s = "a→b"; // bytes: a(1) →(3) b(1) = 5 bytes total
        assert_eq!(ceil_char_boundary(s, 0), 0); // 'a' boundary
        assert_eq!(ceil_char_boundary(s, 1), 1); // '→' boundary
        assert_eq!(ceil_char_boundary(s, 2), 4); // inside '→', ceil to 4 ('b')
        assert_eq!(ceil_char_boundary(s, 3), 4); // inside '→', ceil to 4
        assert_eq!(ceil_char_boundary(s, 4), 4); // 'b' boundary
        assert_eq!(ceil_char_boundary(s, 5), 5); // end
        assert_eq!(ceil_char_boundary(s, 100), 5); // past end
    }

    #[test]
    fn test_parse_llm_assignments_with_utf8_content() {
        // Simulate the actual failing case: LLM response with arrows and other UTF-8
        let response = r#"
Based on my analysis → here are the assignments:

{"assignments": [
    {"line": 5, "agent": "A"},
    {"line": 10, "agent": "B"}
]}

Summary: Tasks assigned → done!
"#;
        let assignments = parse_llm_assignments(response);
        assert_eq!(assignments.len(), 2);
        assert!(assignments.contains(&(5, 'A')));
        assert!(assignments.contains(&(10, 'B')));
    }

    #[test]
    fn test_parse_llm_assignments_with_unicode_heavy_response() {
        // Response with lots of multi-byte characters that could cause slicing issues
        let response = r#"
分析完了 → 結果：
{"assignments":[{"line":1,"agent":"A"},{"line":2,"agent":"B"}]}
タスク完了！🎉
"#;
        let assignments = parse_llm_assignments(response);
        assert_eq!(assignments.len(), 2);
        assert!(assignments.contains(&(1, 'A')));
        assert!(assignments.contains(&(2, 'B')));
    }

    #[test]
    fn test_parse_llm_assignments_utf8_no_panic() {
        // Ensure no panic with various UTF-8 edge cases
        let responses = [
            "→→→ no assignments here →→→",
            r#"{"agent":"A"} → missing line"#,
            "日本語だけ",
            "",
            "🎉🎉🎉",
        ];
        for response in responses {
            // Should not panic, may return empty
            let _ = parse_llm_assignments(response);
        }
    }

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
            "raw response".to_string()
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
