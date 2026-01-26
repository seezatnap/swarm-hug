use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

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
        .args(["team", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    let team_root = repo_path.join(".swarm-hug").join(team_name);
    let tasks_path = write_team_tasks(&team_root);
    let chat_path = team_root.join("chat.md");
    commit_all(repo_path, "init");

    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--team",
            team_name,
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "1",
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
    assert!(chat_content.contains("Sprint 1 plan: 2 task(s) assigned"));
    assert!(chat_content.contains("Completed: Task one"));
    assert!(chat_content.contains("Completed: Task two"));

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

#[test]
fn test_swarm_plan_writes_chat_summary() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["team", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    let team_root = repo_path.join(".swarm-hug").join(team_name);
    write_team_tasks(&team_root);
    let chat_path = team_root.join("chat.md");
    commit_all(repo_path, "init");

    let mut plan_cmd = Command::new(swarm_bin);
    plan_cmd
        .args(["--team", team_name, "--tasks-per-agent", "1", "plan"])
        .current_dir(repo_path);
    run_success(&mut plan_cmd);

    let chat_content = fs::read_to_string(&chat_path).expect("read CHAT.md");
    assert!(chat_content.contains("Sprint 1 plan: 2 task(s) assigned"));
    assert!(chat_content.contains("Aaron assigned: Task one"));
    assert!(chat_content.contains("Betty assigned: Task two"));
}

#[test]
fn test_swarm_status_shows_counts_and_recent_chat() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["team", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    let team_root = repo_path.join(".swarm-hug").join(team_name);
    let tasks_path = team_root.join("tasks.md");
    let tasks_content = "# Tasks\n\n- [ ] (#1) Task one\n- [A] (#2) Task two\n- [x] (#3) Task three (A)\n- [ ] (#4) Task four (blocked by #1)\n";
    fs::write(&tasks_path, tasks_content).expect("write TASKS.md");

    let chat_path = team_root.join("chat.md");
    let mut chat_lines = Vec::new();
    for i in 1..=7 {
        chat_lines.push(format!(
            "2026-01-21 10:00:0{} | Aaron | AGENT_THINK: Message {}",
            i, i
        ));
    }
    fs::write(&chat_path, format!("{}\n", chat_lines.join("\n"))).expect("write CHAT.md");

    let mut status_cmd = Command::new(swarm_bin);
    status_cmd
        .args(["--team", team_name, "status"])
        .current_dir(repo_path);
    let output = run_success(&mut status_cmd);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    assert!(stdout.contains("Unassigned: 2"));
    assert!(stdout.contains("Assigned:   1"));
    assert!(stdout.contains("Completed:  1"));
    assert!(stdout.contains("Assignable: 1"));
    assert!(stdout.contains("Total:      4"));

    for i in 3..=7 {
        assert!(stdout.contains(&format!("Message {}", i)));
    }
    assert!(!stdout.contains("Message 1"));
    assert!(!stdout.contains("Message 2"));
}

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
        .args(["team", "init", team_name])
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
            "--team",
            team_name,
            "--stub",
            "--max-sprints",
            "3",
            "--tasks-per-agent",
            "1",
            "--max-agents",
            "2",
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

