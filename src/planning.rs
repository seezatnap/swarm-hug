//! LLM-assisted sprint planning module.
//!
//! Provides intelligent task assignment and post-sprint review capabilities
//! using the engine abstraction. Can use any engine (claude, codex, stub).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::agent;
use crate::config::EngineType;
use crate::engine::Engine;
use crate::prompt;
use crate::task::TaskList;

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

/// Default scrum master prompt (fallback if prompts/scrum_master.md not found).
const DEFAULT_SCRUM_MASTER_PROMPT: &str = r#"You are the scrum master for a team of AI coding agents. Your job is to assign tasks for the next sprint.

## CRITICAL REQUIREMENT
You MUST assign exactly {{to_assign}} tasks total across {{num_agents}} agents.
- Each agent should get approximately {{tasks_per_agent}} tasks
- You have {{num_unassigned}} unassigned tasks available
- DO NOT assign fewer than {{to_assign}} tasks unless there's a critical blocking dependency

## Available Agents ({{num_agents}} agents)
{{agent_list}}
## Unassigned Tasks ({{num_unassigned}} available)
{{task_list}}
## Assignment Strategy

1. **ASSIGN {{to_assign}} TASKS** - This is mandatory. Distribute across all agents.
2. **Group related tasks** - Give each agent tasks in the same area of the codebase
3. **Respect dependencies** - If Task B depends on Task A:
   - BEST: Assign both to the SAME agent (A runs first, then B)
   - OK: Only assign Task A this sprint, leave B for next sprint
   - NEVER: Assign A to one agent and B to a different agent
4. **Avoid file conflicts** - Don't give different agents tasks that edit the same files
5. **Priority = line order** - Lower line numbers are higher priority

## Output Format

Output ONLY valid JSON (no markdown code blocks, no explanation before or after):
{"assignments":[{"agent":"A","line":5,"reason":"..."},{"agent":"A","line":6,"reason":"..."},{"agent":"B","line":8,"reason":"..."}]}

You must include exactly {{to_assign}} assignment objects. Assign now:"#;

/// Generate the scrum master prompt for task assignment.
///
/// This prompt asks the LLM to assign tasks to agents intelligently,
/// considering dependencies, file conflicts, and priority order.
pub fn generate_scrum_master_prompt(
    task_list: &TaskList,
    agent_initials: &[char],
    tasks_per_agent: usize,
) -> Option<String> {
    let unassigned: Vec<(usize, &str)> = task_list
        .tasks
        .iter()
        .enumerate()
        .filter_map(|(idx, t)| {
            if matches!(t.status, crate::task::TaskStatus::Unassigned) && !t.is_blocked() {
                Some((idx + 1, t.description.as_str())) // 1-indexed line numbers
            } else {
                None
            }
        })
        .collect();

    if unassigned.is_empty() {
        return None;
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

    let rendered = prompt::load_and_render("scrum_master", &vars, DEFAULT_SCRUM_MASTER_PROMPT);
    Some(rendered)
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

    while let Some(agent_pos) = single_line[search_from..].find(r#""agent":"#) {
        let abs_pos = search_from + agent_pos;
        let after_agent = &single_line[abs_pos + 9..]; // skip "agent":"

        // Get the agent letter
        if let Some(agent_char) = after_agent.chars().next() {
            if agent_char.is_ascii_uppercase() {
                // Look for "line": in the surrounding context (within 100 chars)
                let context_start = abs_pos.saturating_sub(50);
                let context_end = (abs_pos + 100).min(single_line.len());
                let context = &single_line[context_start..context_end];

                if let Some(line_pos) = context.find(r#""line":"#) {
                    let line_value_start = context_start + line_pos + 8;
                    if line_value_start < single_line.len() {
                        if let Some(line_num) = parse_number_at(&single_line[line_value_start..]) {
                            assignments.push((line_num, agent_char));
                        }
                    }
                }
            }
        }

        search_from = abs_pos + 10;
        if search_from >= single_line.len() {
            break;
        }
    }

    assignments
}

/// Find the position of the matching closing brace.
fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.chars().enumerate() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
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
        Some(p) => p,
        None => return PlanningResult::failure("No assignable tasks"),
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
        .filter_map(|(idx, t)| {
            if matches!(t.status, crate::task::TaskStatus::Unassigned) && !t.is_blocked() {
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

/// Default review prompt (fallback if prompts/review.md not found).
const DEFAULT_REVIEW_PROMPT: &str = r#"You are the scrum master reviewing the work completed during a sprint. Your job is to identify any follow-up tasks needed.

## Your Responsibilities

1. **Check for incomplete work**: Look for TODOs, FIXMEs, partial implementations, or work that was started but not finished
2. **Check for regressions**: Look for changes that might have broken something or need testing
3. **Check for missing pieces**: If a feature was added, are there missing tests, docs, or edge cases?
4. **Check task accuracy**: Were tasks marked complete that weren't fully done?

## Rules

- Only add follow-up tasks for REAL issues found in the code changes
- Don't add tasks for things already in TASKS.md
- Be specific about what needs to be done
- Keep task descriptions concise
- Use the existing checkbox format: `- [ ] Task description`
- If no follow-ups needed, output "NO_FOLLOWUPS_NEEDED"

## Git Log (commits and changes from this sprint)

```
{{git_log}}
```

## Current TASKS.md

```
{{tasks_content}}
```

## Output Format

If follow-up tasks are needed, output ONLY the new tasks to add (one per line, with `- [ ]` prefix).
If no follow-ups needed, output exactly: NO_FOLLOWUPS_NEEDED

Output now:"#;

/// Generate the post-sprint review prompt.
pub fn generate_review_prompt(
    tasks_content: &str,
    git_log: &str,
) -> String {
    let mut vars = HashMap::new();
    vars.insert("git_log", git_log.to_string());
    vars.insert("tasks_content", tasks_content.to_string());

    prompt::load_and_render("review", &vars, DEFAULT_REVIEW_PROMPT)
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

    let prompt = generate_review_prompt(tasks_content, git_log);

    let result = engine.execute(
        "ScrumMaster",
        &prompt,
        log_dir,
        0, // turn 0 for review
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
    fn test_generate_scrum_master_prompt_empty() {
        let task_list = TaskList::parse("");
        let result = generate_scrum_master_prompt(&task_list, &['A', 'B'], 2);
        assert!(result.is_none());
    }

    #[test]
    fn test_generate_scrum_master_prompt_with_tasks() {
        let content = "# Tasks\n- [ ] Task one\n- [ ] Task two\n- [ ] Task three\n";
        let task_list = TaskList::parse(content);
        let result = generate_scrum_master_prompt(&task_list, &['A', 'B'], 2);
        assert!(result.is_some());
        let prompt = result.unwrap();
        assert!(prompt.contains("Task one"));
        assert!(prompt.contains("Task two"));
        assert!(prompt.contains("Task three"));
        assert!(prompt.contains("A (Aaron)"));
        assert!(prompt.contains("B (Betty)"));
    }

    #[test]
    fn test_generate_scrum_master_prompt_skips_blocked() {
        let content = "# Tasks\n- [ ] Task one\n- [ ] BLOCKED: Task two\n- [ ] Task three\n";
        let task_list = TaskList::parse(content);
        let result = generate_scrum_master_prompt(&task_list, &['A'], 2);
        assert!(result.is_some());
        let prompt = result.unwrap();
        assert!(prompt.contains("Task one"));
        assert!(!prompt.contains("Task two")); // Blocked task excluded
        assert!(prompt.contains("Task three"));
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
        let prompt = generate_review_prompt(tasks, git_log);

        assert!(prompt.contains("Done task"));
        assert!(prompt.contains("commit abc123"));
        assert!(prompt.contains("NO_FOLLOWUPS_NEEDED"));
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
}
