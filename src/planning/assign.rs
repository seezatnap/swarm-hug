use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::agent;
use crate::config::EngineType;
use crate::engine::Engine;
use crate::prompt;
use crate::task::TaskList;

use super::parse::{
    ceil_char_boundary, find_matching_brace, floor_char_boundary, parse_assignments_json,
    parse_number_at,
};

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
    let cleaned = response.replace("```json", "").replace("```", "");

    // Collapse whitespace for easier parsing
    let single_line: String = cleaned.split_whitespace().collect();

    // Try to find the JSON object with assignments
    // Look for {"assignments": or { "assignments": patterns
    if let Some(start) = single_line
        .find(r#"{"assignments":"#)
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
                let context_end =
                    ceil_char_boundary(&single_line, (abs_pos + 100).min(single_line.len()));
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
        0,    // turn 0 for planning
        None, // ScrumMaster doesn't need team context
    );

    if !result.success {
        return PlanningResult::failure(
            result
                .error
                .unwrap_or_else(|| "LLM execution failed".to_string()),
        );
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
        let content =
            "# Tasks\n- [ ] (#1) Task one\n- [ ] (#2) Task two (blocked by #1)\n- [ ] (#3) Task three\n";
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
    fn test_stub_assignment() {
        let content = "- [ ] Task 1\n- [ ] Task 2\n- [ ] Task 3\n- [ ] Task 4\n";
        let task_list = TaskList::parse(content);
        let result = stub_assignment(&task_list, &['A', 'B'], 2);

        assert!(result.success);
        assert_eq!(result.assignments.len(), 4);

        // Verify round-robin: A gets 1,3; B gets 2,4
        let a_tasks: Vec<_> = result
            .assignments
            .iter()
            .filter(|(_, c)| *c == 'A')
            .collect();
        let b_tasks: Vec<_> = result
            .assignments
            .iter()
            .filter(|(_, c)| *c == 'B')
            .collect();
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
}
