use super::parse::parse_task_line;
use super::*;

#[test]
fn test_parse_unassigned() {
    let task = parse_task_line("- [ ] Write tests", 1).unwrap();
    assert_eq!(task.description, "Write tests");
    assert_eq!(task.status, TaskStatus::Unassigned);
}

#[test]
fn test_parse_assigned() {
    let task = parse_task_line("- [A] Write tests", 1).unwrap();
    assert_eq!(task.description, "Write tests");
    assert_eq!(task.status, TaskStatus::Assigned('A'));
}

#[test]
fn test_parse_assigned_lowercase() {
    let task = parse_task_line("- [a] Write tests", 1).unwrap();
    assert_eq!(task.description, "Write tests");
    assert_eq!(task.status, TaskStatus::Assigned('A'));
}

#[test]
fn test_parse_completed() {
    let task = parse_task_line("- [x] Write tests (A)", 1).unwrap();
    assert_eq!(task.description, "Write tests");
    assert_eq!(task.status, TaskStatus::Completed('A'));
}

#[test]
fn test_parse_completed_uppercase_x() {
    let task = parse_task_line("- [X] Write tests (B)", 1).unwrap();
    assert_eq!(task.description, "Write tests");
    assert_eq!(task.status, TaskStatus::Completed('B'));
}

#[test]
fn test_parse_not_a_task() {
    assert!(parse_task_line("# Header", 1).is_none());
    assert!(parse_task_line("Some text", 1).is_none());
    assert!(parse_task_line("", 1).is_none());
}

#[test]
fn test_task_has_blockers() {
    let task = Task::new("(#2) Task (blocked by #1)");
    assert!(task.has_blockers());

    let task2 = Task::new("(#1) Normal task");
    assert!(!task2.has_blockers());

    let task3 = Task::new("(#3) Complex (blocked by #1, #2)");
    assert!(task3.has_blockers());
}

#[test]
fn test_task_number_parsing() {
    let task = Task::new("(#12) Implement feature");
    assert_eq!(task.task_number(), Some(12));

    let task2 = Task::new("No number here");
    assert_eq!(task2.task_number(), None);

    let task3 = Task::new("   (#7) Leading space");
    assert_eq!(task3.task_number(), Some(7));

    let task4 = Task::new("(#abc) Invalid");
    assert_eq!(task4.task_number(), None);
}

#[test]
fn test_tasklist_max_task_number() {
    let content = "- [ ] (#2) Task 2\n- [ ] Task no number\n- [ ] (#10) Task 10\n";
    let list = TaskList::parse(content);
    assert_eq!(list.max_task_number(), 10);
}

#[test]
fn test_blocking_task_numbers_single() {
    let task = Task::new("(#2) My task (blocked by #1)");
    let blockers = task.blocking_task_numbers();
    assert_eq!(blockers, vec![1]);
}

#[test]
fn test_blocking_task_numbers_multiple() {
    let task = Task::new("(#5) Complex task (blocked by #1, #2, #3)");
    let blockers = task.blocking_task_numbers();
    assert_eq!(blockers, vec![1, 2, 3]);
}

#[test]
fn test_blocking_task_numbers_none() {
    let task = Task::new("(#1) Simple task with no blockers");
    let blockers = task.blocking_task_numbers();
    assert!(blockers.is_empty());
}

#[test]
fn test_blocking_task_numbers_with_spaces() {
    let task = Task::new("(#4) Task (blocked by #1, #2)");
    let blockers = task.blocking_task_numbers();
    assert_eq!(blockers, vec![1, 2]);
}

#[test]
fn test_task_assign() {
    let mut task = Task::new("Write tests");
    assert!(task.is_assignable());

    task.assign('a');
    assert_eq!(task.status, TaskStatus::Assigned('A'));
    assert!(!task.is_assignable());
}

#[test]
fn test_task_complete() {
    let mut task = Task::new("Write tests");
    task.assign('B');
    task.complete('B');
    assert_eq!(task.status, TaskStatus::Completed('B'));
}

#[test]
fn test_task_to_line() {
    let mut task = Task::new("Write tests");
    assert_eq!(task.to_line(), "- [ ] Write tests");

    task.assign('A');
    assert_eq!(task.to_line(), "- [A] Write tests");

    task.complete('A');
    assert_eq!(task.to_line(), "- [x] Write tests (A)");
}

#[test]
fn test_tasklist_parse() {
    let content = "# Tasks\n\n- [ ] Task 1\n- [A] Task 2\n- [x] Task 3 (B)\n";
    let list = TaskList::parse(content);

    assert_eq!(list.header.len(), 2); // "# Tasks" and empty line
    assert_eq!(list.tasks.len(), 3);
    assert_eq!(list.tasks[0].status, TaskStatus::Unassigned);
    assert_eq!(list.tasks[1].status, TaskStatus::Assigned('A'));
    assert_eq!(list.tasks[2].status, TaskStatus::Completed('B'));
}

#[test]
fn test_tasklist_counts() {
    let content = "- [ ] Task 1\n- [ ] Task 2\n- [A] Task 3\n- [x] Task 4 (B)\n";
    let list = TaskList::parse(content);

    assert_eq!(list.unassigned_count(), 2);
    assert_eq!(list.assigned_count(), 1);
    assert_eq!(list.completed_count(), 1);
}

#[test]
fn test_tasklist_assignable_count() {
    let content = "- [ ] (#1) Task 1\n- [ ] (#2) Task 2 (blocked by #1)\n- [A] (#3) Task 3\n";
    let list = TaskList::parse(content);

    // Only #1 is assignable: #2 is blocked, #3 is already assigned
    assert_eq!(list.assignable_count(), 1);
}

#[test]
fn test_tasklist_tasks_for_agent() {
    let content = "- [A] Task 1\n- [B] Task 2\n- [A] Task 3\n";
    let list = TaskList::parse(content);

    let a_tasks = list.tasks_for_agent('A');
    assert_eq!(a_tasks.len(), 2);
    assert_eq!(a_tasks[0].description, "Task 1");
    assert_eq!(a_tasks[1].description, "Task 3");
}

#[test]
fn test_tasklist_assign_sprint() {
    let content = "- [ ] Task 1\n- [ ] Task 2\n- [ ] Task 3\n- [ ] Task 4\n- [ ] Task 5\n";
    let mut list = TaskList::parse(content);

    let assigned = list.assign_sprint(&['A', 'B'], 2);
    assert_eq!(assigned, 4);

    // A gets tasks 1, 2; B gets tasks 3, 4
    assert_eq!(list.tasks[0].status, TaskStatus::Assigned('A'));
    assert_eq!(list.tasks[1].status, TaskStatus::Assigned('A'));
    assert_eq!(list.tasks[2].status, TaskStatus::Assigned('B'));
    assert_eq!(list.tasks[3].status, TaskStatus::Assigned('B'));
    assert_eq!(list.tasks[4].status, TaskStatus::Unassigned);
}

#[test]
fn test_tasklist_assign_sprint_skips_blocked() {
    // Task 1 is blocked by incomplete task 3
    let content = "- [ ] (#1) Task 1 (blocked by #3)\n- [ ] (#2) Task 2\n- [ ] (#3) Task 3\n";
    let mut list = TaskList::parse(content);

    let assigned = list.assign_sprint(&['A'], 2);
    assert_eq!(assigned, 2);

    assert_eq!(list.tasks[0].status, TaskStatus::Unassigned); // still blocked by #3
    assert_eq!(list.tasks[1].status, TaskStatus::Assigned('A'));
    assert_eq!(list.tasks[2].status, TaskStatus::Assigned('A'));
}

#[test]
fn test_tasklist_is_task_blocked_dynamic() {
    // Task #2 is blocked by #1, which is not completed
    let content = "- [ ] (#1) First task\n- [ ] (#2) Second task (blocked by #1)\n";
    let list = TaskList::parse(content);

    assert!(!list.is_task_blocked(0)); // #1 is not blocked
    assert!(list.is_task_blocked(1)); // #2 is blocked by incomplete #1
}

#[test]
fn test_tasklist_is_task_blocked_dynamic_completed() {
    // Task #2 is blocked by #1, which IS completed
    let content = "- [x] (#1) First task (A)\n- [ ] (#2) Second task (blocked by #1)\n";
    let list = TaskList::parse(content);

    assert!(!list.is_task_blocked(0)); // #1 is completed, not blocked
    assert!(!list.is_task_blocked(1)); // #2 is now unblocked because #1 is done
}

#[test]
fn test_tasklist_is_task_blocked_multiple_blockers() {
    // Task #3 is blocked by #1 and #2
    let content =
        "- [x] (#1) First task (A)\n- [ ] (#2) Second task\n- [ ] (#3) Third task (blocked by #1, #2)\n";
    let list = TaskList::parse(content);

    assert!(list.is_task_blocked(2)); // #3 is blocked because #2 is not complete

    // Now with both completed
    let content2 = "- [x] (#1) First task (A)\n- [x] (#2) Second task (B)\n- [ ] (#3) Third task (blocked by #1, #2)\n";
    let list2 = TaskList::parse(content2);

    assert!(!list2.is_task_blocked(2)); // #3 is unblocked because both #1 and #2 are done
}

#[test]
fn test_tasklist_is_task_assignable_with_blockers() {
    let content = "- [x] (#1) First task (A)\n- [ ] (#2) Second task (blocked by #1)\n";
    let list = TaskList::parse(content);

    assert!(!list.is_task_assignable(0)); // #1 is completed, not assignable
    assert!(list.is_task_assignable(1)); // #2 is unblocked and unassigned, so assignable
}

#[test]
fn test_tasklist_assignable_count_with_dynamic_blocking() {
    // #1 incomplete, #2 blocked by #1
    let content = "- [ ] (#1) First task\n- [ ] (#2) Second task (blocked by #1)\n";
    let list = TaskList::parse(content);

    assert_eq!(list.assignable_count(), 1); // Only #1 is assignable

    // #1 complete, #2 now unblocked
    let content2 = "- [x] (#1) First task (A)\n- [ ] (#2) Second task (blocked by #1)\n";
    let list2 = TaskList::parse(content2);

    assert_eq!(list2.assignable_count(), 1); // Only #2 is assignable now (#1 is done)
}

#[test]
fn test_tasklist_assign_sprint_respects_dynamic_blocking() {
    // Initially #2 is blocked by incomplete #1
    let content = "- [ ] (#1) First task\n- [ ] (#2) Second task (blocked by #1)\n";
    let mut list = TaskList::parse(content);

    let assigned = list.assign_sprint(&['A'], 2);
    assert_eq!(assigned, 1); // Only #1 can be assigned

    assert_eq!(list.tasks[0].status, TaskStatus::Assigned('A'));
    assert_eq!(list.tasks[1].status, TaskStatus::Unassigned); // Still blocked
}

#[test]
fn test_real_world_blocked_task_scenario() {
    // This is the exact scenario from the user's bug report
    let content = r#"## Frontend - Location Array Input

- [x] (#8) Create reusable array input component with add/remove buttons for individual items (C)
- [ ] (#9) Replace location text input with array input component in job form/edit (blocked by #8)
"#;
    let list = TaskList::parse(content);

    // #8 is completed, so #9 should be unblocked and assignable
    assert!(!list.is_task_blocked(1)); // #9 is NOT blocked anymore
    assert!(list.is_task_assignable(1)); // #9 should be assignable
    assert_eq!(list.assignable_count(), 1); // Only #9 is assignable
}

#[test]
fn test_tasklist_to_string() {
    let content = "# Tasks\n\n- [ ] Task 1\n- [A] Task 2\n";
    let list = TaskList::parse(content);
    let output = list.to_string();

    assert!(output.contains("# Tasks"));
    assert!(output.contains("- [ ] Task 1"));
    assert!(output.contains("- [A] Task 2"));
}

#[test]
fn test_tasklist_roundtrip() {
    let content = "# Tasks\n\n- [ ] Task 1\n- [A] Task 2\n- [x] Task 3 (B)\n";
    let list = TaskList::parse(content);
    let output = list.to_string();

    // Parse again and verify
    let list2 = TaskList::parse(&output);
    assert_eq!(list2.tasks.len(), 3);
    assert_eq!(list2.tasks[0].description, "Task 1");
    assert_eq!(list2.tasks[1].description, "Task 2");
    assert_eq!(list2.tasks[2].description, "Task 3");
}

#[test]
fn test_task_unassign() {
    let mut task = Task::new("Write tests");
    task.assign('A');
    assert_eq!(task.status, TaskStatus::Assigned('A'));

    task.unassign();
    assert_eq!(task.status, TaskStatus::Unassigned);
    assert!(task.is_assignable());
}

#[test]
fn test_task_unassign_completed_no_effect() {
    let mut task = Task::new("Write tests");
    task.assign('A');
    task.complete('A');
    assert_eq!(task.status, TaskStatus::Completed('A'));

    task.unassign(); // Should have no effect on completed tasks
    assert_eq!(task.status, TaskStatus::Completed('A'));
}

#[test]
fn test_tasklist_unassign_all() {
    let content = "- [ ] Task 1\n- [A] Task 2\n- [B] Task 3\n- [x] Task 4 (C)\n";
    let mut list = TaskList::parse(content);

    assert_eq!(list.assigned_count(), 2);

    let unassigned = list.unassign_all();
    assert_eq!(unassigned, 2);
    assert_eq!(list.assigned_count(), 0);
    assert_eq!(list.unassigned_count(), 3); // Task 1, 2, 3 now unassigned
    assert_eq!(list.completed_count(), 1); // Task 4 still completed
}

#[test]
fn test_tasklist_preserves_section_headings() {
    // Test that section headings between tasks are preserved
    let content = "# Tasks\n\n### Section 1\n- [ ] Task 1\n- [ ] Task 2\n\n### Section 2\n- [ ] Task 3\n";
    let list = TaskList::parse(content);

    // Header includes everything before the first task
    assert_eq!(list.header.len(), 3); // "# Tasks", "", "### Section 1"
    assert_eq!(list.header, vec!["# Tasks", "", "### Section 1"]);
    assert_eq!(list.tasks.len(), 3);

    // First task has no prefix (section heading is in header since it's before first task)
    assert!(list.tasks[0].prefix.is_empty());
    assert_eq!(list.tasks[0].description, "Task 1");

    // Second task has no prefix (follows directly after first)
    assert!(list.tasks[1].prefix.is_empty());
    assert_eq!(list.tasks[1].description, "Task 2");

    // Third task should have blank line and section heading as prefix
    assert_eq!(list.tasks[2].prefix, vec!["", "### Section 2"]);
    assert_eq!(list.tasks[2].description, "Task 3");
}

#[test]
fn test_tasklist_section_roundtrip() {
    // Test that parsing and writing back preserves document structure
    let content = "# Phase 0 Tasks\n\n## M0.1 — Setup\n\n### Directory Structure\n- [ ] Task 1\n- [A] Task 2\n\n### Tooling\n- [ ] Task 3\n- [x] Task 4 (B)\n\n## M0.2 — Database\n- [ ] Task 5\n";
    let list = TaskList::parse(content);
    let output = list.to_string();

    // The output should preserve the section structure
    assert!(output.contains("# Phase 0 Tasks"));
    assert!(output.contains("## M0.1 — Setup"));
    assert!(output.contains("### Directory Structure"));
    assert!(output.contains("### Tooling"));
    assert!(output.contains("## M0.2 — Database"));

    // Verify order is correct by checking substring positions
    let pos_setup = output.find("## M0.1 — Setup").unwrap();
    let pos_dir = output.find("### Directory Structure").unwrap();
    let pos_task1 = output.find("Task 1").unwrap();
    let pos_tooling = output.find("### Tooling").unwrap();
    let pos_task3 = output.find("Task 3").unwrap();
    let pos_database = output.find("## M0.2 — Database").unwrap();
    let pos_task5 = output.find("Task 5").unwrap();

    assert!(pos_setup < pos_dir, "Setup should come before Directory Structure");
    assert!(pos_dir < pos_task1, "Directory Structure should come before Task 1");
    assert!(pos_task1 < pos_tooling, "Task 1 should come before Tooling");
    assert!(pos_tooling < pos_task3, "Tooling should come before Task 3");
    assert!(pos_task3 < pos_database, "Task 3 should come before Database");
    assert!(pos_database < pos_task5, "Database should come before Task 5");
}

#[test]
fn test_tasklist_section_roundtrip_exact() {
    // Test exact roundtrip fidelity
    let content = "# Tasks\n\n### Section A\n- [ ] Task 1\n\n### Section B\n- [ ] Task 2\n";
    let list = TaskList::parse(content);
    let output = list.to_string();

    assert_eq!(output, content);
}

#[test]
fn test_tasklist_preserves_blank_lines_between_sections() {
    let content = "# Header\n\n- [ ] Task 1\n\n\n### New Section\n- [ ] Task 2\n";
    let list = TaskList::parse(content);

    assert_eq!(list.tasks.len(), 2);
    // Task 2 should have two blank lines and section heading as prefix
    assert_eq!(list.tasks[1].prefix, vec!["", "", "### New Section"]);

    let output = list.to_string();
    assert_eq!(output, content);
}

#[test]
fn test_tasklist_complex_structure_roundtrip() {
    // Real-world example similar to user's issue
    let content = r#"# Phase 0 Tasks

## M0.1 — Repository Structure

### Directory Setup
- [x] Create /apps/web directory (A)
- [A] Configure ESLint
- [ ] Configure Prettier

### Build Scripts
- [B] Add pnpm build script
- [ ] Add pnpm test script

## M0.2 — Database

### Schema
- [ ] Create jobs table migration
- [ ] Create candidates table migration
"#;
    let list = TaskList::parse(content);
    let output = list.to_string();

    // Verify all sections are preserved in correct order
    let lines: Vec<&str> = output.lines().collect();

    // Find key lines and verify order
    let phase_idx = lines.iter().position(|l| l.contains("# Phase 0")).unwrap();
    let m01_idx = lines.iter().position(|l| l.contains("## M0.1")).unwrap();
    let dir_idx = lines.iter().position(|l| l.contains("### Directory")).unwrap();
    let build_idx = lines.iter().position(|l| l.contains("### Build")).unwrap();
    let m02_idx = lines.iter().position(|l| l.contains("## M0.2")).unwrap();
    let schema_idx = lines.iter().position(|l| l.contains("### Schema")).unwrap();

    assert!(phase_idx < m01_idx);
    assert!(m01_idx < dir_idx);
    assert!(dir_idx < build_idx);
    assert!(build_idx < m02_idx);
    assert!(m02_idx < schema_idx);

    // Verify tasks are under correct sections
    let create_web_idx = lines
        .iter()
        .position(|l| l.contains("Create /apps/web"))
        .unwrap();
    let eslint_idx = lines
        .iter()
        .position(|l| l.contains("Configure ESLint"))
        .unwrap();
    let build_script_idx = lines
        .iter()
        .position(|l| l.contains("pnpm build"))
        .unwrap();
    let jobs_table_idx = lines.iter().position(|l| l.contains("jobs table")).unwrap();

    assert!(
        dir_idx < create_web_idx && create_web_idx < build_idx,
        "Create /apps/web should be under Directory Setup"
    );
    assert!(
        dir_idx < eslint_idx && eslint_idx < build_idx,
        "Configure ESLint should be under Directory Setup"
    );
    assert!(
        build_idx < build_script_idx && build_script_idx < m02_idx,
        "pnpm build should be under Build Scripts"
    );
    assert!(schema_idx < jobs_table_idx, "jobs table should be under Schema");
}
