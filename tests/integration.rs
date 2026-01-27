use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

use swarm::chat;
use swarm::task::{TaskList, TaskStatus};

/// Strip ANSI escape codes from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until we hit a letter (which ends the escape sequence)
            while let Some(&next) = chars.peek() {
                chars.next();
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn chat_contains_message(chat_content: &str, agent: &str, message: &str) -> bool {
    chat_content.lines().any(|line| {
        chat::parse_line(line)
            .map(|(_, line_agent, line_message)| line_agent == agent && line_message == message)
            .unwrap_or(false)
    })
}

fn run_success(cmd: &mut Command) -> Output {
    let output = cmd.output().expect("failed to run command");
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn init_git_repo(path: &Path) {
    let mut cmd = Command::new("git");
    cmd.arg("init").current_dir(path);
    run_success(&mut cmd);

    let mut name_cmd = Command::new("git");
    name_cmd
        .args(["config", "user.name", "Swarm Test"])
        .current_dir(path);
    run_success(&mut name_cmd);

    let mut email_cmd = Command::new("git");
    email_cmd
        .args(["config", "user.email", "swarm-test@example.com"])
        .current_dir(path);
    run_success(&mut email_cmd);
}

fn commit_all(path: &Path, message: &str) {
    let mut add_cmd = Command::new("git");
    add_cmd.args(["add", "."]).current_dir(path);
    run_success(&mut add_cmd);

    let mut commit_cmd = Command::new("git");
    commit_cmd.args(["commit", "-m", message]).current_dir(path);
    run_success(&mut commit_cmd);
}

fn write_team_tasks(team_root: &Path) -> PathBuf {
    let content = "# Tasks\n\n- [ ] Task one\n- [ ] Task two\n";
    let tasks_path = team_root.join("tasks.md");
    fs::write(&tasks_path, content).expect("write TASKS.md");
    tasks_path
}

fn write_team_tasks_multi_sprint(team_root: &Path) -> PathBuf {
    // 6 tasks: 2 per sprint for 3 sprints (with --tasks-per-agent 1 and 2 agents)
    let content = "# Tasks\n\n- [ ] Task 1\n- [ ] Task 2\n- [ ] Task 3\n- [ ] Task 4\n- [ ] Task 5\n- [ ] Task 6\n";
    let tasks_path = team_root.join("tasks.md");
    fs::write(&tasks_path, content).expect("write TASKS.md");
    tasks_path
}

#[test]
fn test_swarm_run_stub_integration() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    let team_root = repo_path.join(".swarm-hug").join(team_name);
    let tasks_path = write_team_tasks(&team_root);
    let chat_path = team_root.join("chat.md");
    commit_all(repo_path, "init");

    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--project",
            team_name,
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "1",
            "--no-tui",
            "--no-tail",
            "run",
        ])
        .current_dir(repo_path);
    run_success(&mut run_cmd);

    let tasks_content = fs::read_to_string(&tasks_path).expect("read TASKS.md");
    let task_list = TaskList::parse(&tasks_content);

    assert_eq!(task_list.completed_count(), 2);
    assert_eq!(task_list.assigned_count(), 0);
    assert_eq!(task_list.unassigned_count(), 0);

    let mut completed_initials = Vec::new();
    for task in &task_list.tasks {
        if let TaskStatus::Completed(initial) = task.status {
            completed_initials.push(initial);
        }
    }
    completed_initials.sort();
    assert_eq!(completed_initials, vec!['A', 'B']);

    let chat_content = fs::read_to_string(&chat_path).expect("read CHAT.md");
    assert!(chat_contains_message(
        &chat_content,
        "ScrumMaster",
        "Sprint planning started"
    ));
    assert!(chat_contains_message(
        &chat_content,
        "ScrumMaster",
        "Post-mortem started"
    ));
    assert!(chat_content.contains("Sprint 1 plan: 2 task(s) assigned"));
    assert!(chat_content.contains("SPRINT STATUS: Alpha Sprint 1 complete"));
    assert!(chat_content.contains("SPRINT STATUS: Completed this sprint: 2"));
    assert!(chat_content.contains("Completed: Task one"));
    assert!(chat_content.contains("Completed: Task two"));
    let chat_lines: Vec<&str> = chat_content.lines().collect();
    assert!(chat_lines.len() >= 5, "expected at least 5 chat lines");
    let mut tail: Vec<&str> = chat_lines.iter().rev().take(5).copied().collect();
    tail.reverse();
    assert!(tail[0].contains("SPRINT STATUS: Alpha Sprint 1 complete"));
    assert!(tail[1].contains("SPRINT STATUS: Completed this sprint: 2"));
    assert!(tail[2].contains("SPRINT STATUS: Failed this sprint: 0"));
    assert!(tail[3].contains("SPRINT STATUS: Remaining tasks: 0"));
    assert!(tail[4].contains("SPRINT STATUS: Total tasks: 2"));

    // Agents are unassigned after each sprint completes so they are available for next sprint
    let assignments_path = repo_path.join(".swarm-hug").join("assignments.toml");
    let assignments_content = fs::read_to_string(&assignments_path).expect("read assignments.toml");
    assert!(
        !assignments_content.contains("A = \"alpha\""),
        "agent A should be unassigned after sprint"
    );
    assert!(
        !assignments_content.contains("B = \"alpha\""),
        "agent B should be unassigned after sprint"
    );

    let output_dir = team_root.join("loop");
    assert!(output_dir.join("turn1-agentA.md").exists());
    assert!(output_dir.join("turn1-agentB.md").exists());

    // Worktrees are cleaned up after sprint completes for reliable recreation from master
    let worktrees_dir = team_root.join("worktrees");
    assert!(
        !worktrees_dir.join("agent-A-Aaron").exists(),
        "worktrees should be cleaned up after sprint"
    );
    assert!(
        !worktrees_dir.join("agent-B-Betty").exists(),
        "worktrees should be cleaned up after sprint"
    );

    // Branches are also cleaned up after sprint
    let mut branches_cmd = Command::new("git");
    branches_cmd
        .args(["branch", "--list", "agent/*"])
        .current_dir(repo_path);
    let branches_output = run_success(&mut branches_cmd);
    let branches_stdout = String::from_utf8_lossy(&branches_output.stdout);
    assert!(
        branches_stdout.trim().is_empty(),
        "agent branches should be cleaned up after sprint"
    );

    // Sprint completion should be committed (tasks marked complete, assignments released)
    let mut status_cmd = Command::new("git");
    status_cmd
        .args(["status", "--porcelain"])
        .current_dir(repo_path);
    let status_output = run_success(&mut status_cmd);
    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    // Filter out gitignored files (loop/, worktrees/, chat.md)
    let uncommitted: Vec<&str> = status_stdout
        .lines()
        .filter(|line| {
            !line.contains("/loop/")
                && !line.contains("loop/")
                && !line.contains("/worktrees/")
                && !line.contains("worktrees/")
                && !line.contains("chat.md")
        })
        .collect();
    assert!(
        uncommitted.is_empty(),
        "tasks and assignments should be committed after sprint, found uncommitted: {:?}",
        uncommitted
    );
}

// test_swarm_plan_writes_chat_summary removed: plan command was deprecated

// test_swarm_status_shows_counts_and_recent_chat removed: status command was deprecated

/// Test that multiple consecutive sprints correctly reassign agents.
/// This verifies agents are released after each sprint and picked up again for the next.
#[test]
fn test_swarm_run_multiple_sprints_reassigns_agents() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Initialize team
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    // Write 6 tasks - should complete in 3 sprints with 2 agents, 1 task per agent per sprint
    let team_root = repo_path.join(".swarm-hug").join(team_name);
    let tasks_path = write_team_tasks_multi_sprint(&team_root);
    let chat_path = team_root.join("chat.md");
    commit_all(repo_path, "init");

    // Run 3 sprints
    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--project",
            team_name,
            "--stub",
            "--max-sprints",
            "3",
            "--tasks-per-agent",
            "1",
            "--max-agents",
            "2",
            "--no-tui",
            "--no-tail",
            "run",
        ])
        .current_dir(repo_path);
    let output = run_success(&mut run_cmd);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    // Verify all 3 sprints were executed
    assert!(
        stdout.contains("Sprint 1: assigned 2 task(s)"),
        "Sprint 1 should assign 2 tasks. Output: {}",
        stdout
    );
    assert!(
        stdout.contains("Sprint 2: assigned 2 task(s)"),
        "Sprint 2 should assign 2 tasks. Output: {}",
        stdout
    );
    assert!(
        stdout.contains("Sprint 3: assigned 2 task(s)"),
        "Sprint 3 should assign 2 tasks. Output: {}",
        stdout
    );

    // Verify all tasks are completed
    let tasks_content = fs::read_to_string(&tasks_path).expect("read TASKS.md");
    let task_list = TaskList::parse(&tasks_content);
    assert_eq!(
        task_list.completed_count(),
        6,
        "All 6 tasks should be completed"
    );
    assert_eq!(task_list.assigned_count(), 0, "No tasks should be assigned");
    assert_eq!(
        task_list.unassigned_count(),
        0,
        "No tasks should be unassigned"
    );

    // Verify chat contains sprint plans for all 3 sprints
    let chat_content = fs::read_to_string(&chat_path).expect("read CHAT.md");
    assert!(
        chat_content.contains("Sprint 1 plan:"),
        "Chat should contain Sprint 1 plan"
    );
    assert!(
        chat_content.contains("Sprint 2 plan:"),
        "Chat should contain Sprint 2 plan"
    );
    assert!(
        chat_content.contains("Sprint 3 plan:"),
        "Chat should contain Sprint 3 plan"
    );

    // Verify agents were released after all sprints (assignments should be empty)
    let assignments_path = repo_path.join(".swarm-hug").join("assignments.toml");
    let assignments_content =
        fs::read_to_string(&assignments_path).expect("read assignments.toml");
    assert!(
        !assignments_content.contains("A = \"alpha\""),
        "Agent A should be unassigned after all sprints"
    );
    assert!(
        !assignments_content.contains("B = \"alpha\""),
        "Agent B should be unassigned after all sprints"
    );

    // Verify release messages were logged for each sprint
    assert!(
        stdout.contains("Released 2 agent assignment(s)"),
        "Should release agents after each sprint. Output: {}",
        stdout
    );

    // Verify post-sprint review was attempted (stub engine makes no git changes, so review is skipped)
    assert!(
        stdout.contains("Post-sprint review:"),
        "Should attempt post-sprint review after each sprint. Output: {}",
        stdout
    );
}

/// Test that per-task engine selection mechanism works correctly.
/// Verifies that when an agent has multiple tasks, the engine selection/creation
/// happens for each task individually (not once per agent).
///
/// This test uses stub mode for reliable CI testing. The key verification is that
/// the agent log shows "Executing with engine:" for each task, confirming the
/// per-task engine selection code path is exercised.
#[test]
fn test_per_task_engine_selection_mechanism() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Initialize team
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    // Write 3 tasks - with 1 agent and 3 tasks-per-agent, single agent gets all tasks
    let team_root = repo_path.join(".swarm-hug").join(team_name);
    let tasks_path = team_root.join("tasks.md");
    let tasks_content = "# Tasks\n\n- [ ] Task one\n- [ ] Task two\n- [ ] Task three\n";
    fs::write(&tasks_path, tasks_content).expect("write TASKS.md");
    commit_all(repo_path, "init");

    // Run with stub mode and single agent with 3 tasks
    // This ensures one agent handles multiple tasks sequentially
    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--project",
            team_name,
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "3",
            "--max-agents",
            "1",
            "--no-tui",
            "--no-tail",
            "run",
        ])
        .current_dir(repo_path);
    run_success(&mut run_cmd);

    // Verify all tasks completed
    let tasks_content = fs::read_to_string(&tasks_path).expect("read TASKS.md");
    let task_list = TaskList::parse(&tasks_content);
    assert_eq!(
        task_list.completed_count(),
        3,
        "All 3 tasks should be completed by single agent"
    );

    // Verify agent log shows engine execution for each task
    // The log format is: YYYY-MM-DD HH:MM:SS | AgentName | message
    let log_dir = team_root.join("loop");
    let agent_log = log_dir.join("agent-A.log");
    assert!(
        agent_log.exists(),
        "Agent A log file should exist at {:?}",
        agent_log
    );

    let log_content = fs::read_to_string(&agent_log).expect("read agent log");

    // Count occurrences of "Executing with engine:" in the log
    // Should appear once per task (3 times total)
    let engine_exec_count = log_content.matches("Executing with engine:").count();
    assert_eq!(
        engine_exec_count, 3,
        "Should have 3 'Executing with engine:' entries (one per task), found {}. Log content:\n{}",
        engine_exec_count, log_content
    );

    // Verify each task was assigned and logged
    assert!(
        log_content.contains("Assigned task: Task one"),
        "Log should contain 'Assigned task: Task one'"
    );
    assert!(
        log_content.contains("Assigned task: Task two"),
        "Log should contain 'Assigned task: Task two'"
    );
    assert!(
        log_content.contains("Assigned task: Task three"),
        "Log should contain 'Assigned task: Task three'"
    );

    // Verify the engine type is logged (stub in this case)
    assert!(
        log_content.contains("Executing with engine: stub"),
        "Log should show 'Executing with engine: stub'"
    );
}

/// Test multi-engine configuration parsing and selection.
/// Verifies that --engine flag with multiple engines is parsed correctly
/// and that the engine selection mechanism handles multiple engine types.
///
/// Note: In stub mode, the actual engine used is always 'stub', but this test
/// verifies the configuration parsing and per-task selection mechanism work together.
#[test]
fn test_multi_engine_configuration() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Initialize team
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    // Write tasks for a single agent with multiple tasks
    let team_root = repo_path.join(".swarm-hug").join(team_name);
    let tasks_path = team_root.join("tasks.md");
    let tasks_content = "# Tasks\n\n- [ ] Task A\n- [ ] Task B\n";
    fs::write(&tasks_path, tasks_content).expect("write TASKS.md");
    commit_all(repo_path, "init");

    // Run with --engine claude,codex AND --stub
    // The --stub flag overrides engine selection to use stub engine,
    // but this verifies the multi-engine flag is accepted
    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--project",
            team_name,
            "--engine",
            "claude,codex",
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "2",
            "--max-agents",
            "1",
            "--no-tui",
            "--no-tail",
            "run",
        ])
        .current_dir(repo_path);
    let output = run_success(&mut run_cmd);

    // Verify command succeeded (validates that --engine claude,codex is accepted)
    assert!(
        output.status.success(),
        "Command with --engine claude,codex should succeed"
    );

    // Verify tasks completed
    let tasks_content = fs::read_to_string(&tasks_path).expect("read TASKS.md");
    let task_list = TaskList::parse(&tasks_content);
    assert_eq!(
        task_list.completed_count(),
        2,
        "Both tasks should be completed"
    );

    // Verify agent log shows per-task engine execution
    let log_dir = team_root.join("loop");
    let agent_log = log_dir.join("agent-A.log");
    let log_content = fs::read_to_string(&agent_log).expect("read agent log");

    // Should have 2 engine execution entries (one per task)
    let engine_exec_count = log_content.matches("Executing with engine:").count();
    assert_eq!(
        engine_exec_count, 2,
        "Should have 2 'Executing with engine:' entries (one per task)"
    );
}

/// Test that stub mode continues using stub engine exclusively.
/// When --stub flag is used, the engine should always be 'stub' regardless of
/// what engines are configured via --engine flag.
///
/// This test verifies:
/// 1. With --stub and --engine claude,codex, all tasks use stub engine
/// 2. Agent logs show "Executing with engine: stub" for all tasks
/// 3. Chat messages show "[engine: stub]" for all tasks
/// 4. No task uses claude or codex engines
#[test]
fn test_stub_mode_uses_stub_engine_exclusively() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Initialize team
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    // Write multiple tasks for a single agent to ensure multiple per-task engine selections
    let team_root = repo_path.join(".swarm-hug").join(team_name);
    let tasks_path = team_root.join("tasks.md");
    let tasks_content = "# Tasks\n\n- [ ] Task Alpha\n- [ ] Task Beta\n- [ ] Task Gamma\n";
    fs::write(&tasks_path, tasks_content).expect("write TASKS.md");
    commit_all(repo_path, "init");

    // Run with --engine claude,codex AND --stub
    // The --stub flag should OVERRIDE the engine selection to always use stub
    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--project",
            team_name,
            "--engine",
            "claude,codex",
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "3",
            "--max-agents",
            "1",
            "--no-tui",
            "--no-tail",
            "run",
        ])
        .current_dir(repo_path);
    let output = run_success(&mut run_cmd);
    assert!(
        output.status.success(),
        "Command with --stub and --engine should succeed"
    );

    // Verify all tasks completed
    let tasks_content = fs::read_to_string(&tasks_path).expect("read TASKS.md");
    let task_list = TaskList::parse(&tasks_content);
    assert_eq!(
        task_list.completed_count(),
        3,
        "All 3 tasks should be completed"
    );

    // Verify agent log shows stub engine for ALL tasks
    let log_dir = team_root.join("loop");
    let agent_log = log_dir.join("agent-A.log");
    let log_content = fs::read_to_string(&agent_log).expect("read agent log");

    // Count stub engine executions - should be 3 (one per task)
    let stub_engine_count = log_content.matches("Executing with engine: stub").count();
    assert_eq!(
        stub_engine_count, 3,
        "Should have 3 'Executing with engine: stub' entries (one per task). Log:\n{}",
        log_content
    );

    // Verify NO claude or codex engine executions
    assert!(
        !log_content.contains("Executing with engine: claude"),
        "Should not have any claude engine executions in stub mode. Log:\n{}",
        log_content
    );
    assert!(
        !log_content.contains("Executing with engine: codex"),
        "Should not have any codex engine executions in stub mode. Log:\n{}",
        log_content
    );

    // Verify chat messages also show stub engine
    let chat_path = team_root.join("chat.md");
    let chat_content = fs::read_to_string(&chat_path).expect("read chat.md");

    // All "Starting:" messages should have [engine: stub]
    let starting_stub_count = chat_content.matches("[engine: stub]").count();
    assert_eq!(
        starting_stub_count, 3,
        "All 3 'Starting:' messages should have [engine: stub]. Chat:\n{}",
        chat_content
    );

    // Verify NO claude or codex in chat engine tags
    assert!(
        !chat_content.contains("[engine: claude]"),
        "Should not have any [engine: claude] in chat. Chat:\n{}",
        chat_content
    );
    assert!(
        !chat_content.contains("[engine: codex]"),
        "Should not have any [engine: codex] in chat. Chat:\n{}",
        chat_content
    );
}

/// Test that single-engine configuration works unchanged.
/// When only one engine is configured (e.g., `--engine claude`), that engine should be used
/// for all tasks without any changes to behavior from the per-task engine selection mechanism.
///
/// This test verifies:
/// 1. Single engine configuration is accepted and works correctly
/// 2. All tasks use the same engine (the one configured)
/// 3. Agent logs show consistent engine usage across all tasks
/// 4. Chat messages show the same engine for all tasks
///
/// Note: We use stub mode here, so the "single engine" is actually stub,
/// but this verifies the per-task selection with a single-element engine list works.
#[test]
fn test_single_engine_configuration_works_unchanged() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Initialize team
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    // Write multiple tasks for a single agent to exercise per-task engine selection
    let team_root = repo_path.join(".swarm-hug").join(team_name);
    let tasks_path = team_root.join("tasks.md");
    let tasks_content = "# Tasks\n\n- [ ] First task\n- [ ] Second task\n- [ ] Third task\n";
    fs::write(&tasks_path, tasks_content).expect("write TASKS.md");
    commit_all(repo_path, "init");

    // Run with --stub (single engine configuration)
    // This simulates a single-engine config where all tasks should use the same engine
    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--project",
            team_name,
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "3",
            "--max-agents",
            "1",
            "--no-tui",
            "--no-tail",
            "run",
        ])
        .current_dir(repo_path);
    let output = run_success(&mut run_cmd);
    assert!(
        output.status.success(),
        "Command with single engine configuration should succeed"
    );

    // Verify all tasks completed
    let tasks_content = fs::read_to_string(&tasks_path).expect("read TASKS.md");
    let task_list = TaskList::parse(&tasks_content);
    assert_eq!(
        task_list.completed_count(),
        3,
        "All 3 tasks should be completed"
    );

    // Verify agent log shows consistent engine usage for all tasks
    let log_dir = team_root.join("loop");
    let agent_log = log_dir.join("agent-A.log");
    let log_content = fs::read_to_string(&agent_log).expect("read agent log");

    // All executions should use stub engine
    let stub_engine_count = log_content.matches("Executing with engine: stub").count();
    assert_eq!(
        stub_engine_count, 3,
        "All 3 tasks should use stub engine. Log:\n{}",
        log_content
    );

    // Verify chat messages show consistent engine for all tasks
    let chat_path = team_root.join("chat.md");
    let chat_content = fs::read_to_string(&chat_path).expect("read chat.md");

    // All "Starting:" messages should have [engine: stub]
    let engine_stub_count = chat_content.matches("[engine: stub]").count();
    assert_eq!(
        engine_stub_count, 3,
        "All 3 tasks should show [engine: stub] in chat. Chat:\n{}",
        chat_content
    );

    // Verify no other engine types appear
    assert!(
        !log_content.contains("Executing with engine: claude"),
        "Should not have claude engine in single-engine config"
    );
    assert!(
        !log_content.contains("Executing with engine: codex"),
        "Should not have codex engine in single-engine config"
    );
}

/// Regression test: chat history should persist across consecutive sprints in a single run.
#[test]
fn test_chat_history_persists_across_sprints_in_single_run() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    let team_root = repo_path.join(".swarm-hug").join(team_name);
    let tasks_path = team_root.join("tasks.md");
    let tasks_content = "# Tasks\n\n- [ ] Task 1\n- [ ] Task 2\n- [ ] Task 3\n- [ ] Task 4\n";
    fs::write(&tasks_path, tasks_content).expect("write TASKS.md");
    let chat_path = team_root.join("chat.md");
    commit_all(repo_path, "init");

    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--project",
            team_name,
            "--stub",
            "--max-sprints",
            "2",
            "--tasks-per-agent",
            "1",
            "--max-agents",
            "2",
            "--no-tui",
            "--no-tail",
            "run",
        ])
        .current_dir(repo_path);
    run_success(&mut run_cmd);

    let chat_content = fs::read_to_string(&chat_path).expect("read CHAT.md");
    let planning_count = chat_content.matches("Sprint planning started").count();
    assert_eq!(
        planning_count, 2,
        "chat should retain planning logs from both sprints"
    );

    let sprint1_pos = chat_content
        .find("Sprint 1 plan:")
        .expect("Sprint 1 plan should be present");
    let sprint2_pos = chat_content
        .find("Sprint 2 plan:")
        .expect("Sprint 2 plan should be present");
    assert!(
        sprint1_pos < sprint2_pos,
        "Sprint 1 plan should appear before Sprint 2 plan in chat history"
    );
}
