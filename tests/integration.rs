use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

use swarm::task::{TaskList, TaskStatus};

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

fn run_failure(cmd: &mut Command) -> Output {
    let output = cmd.output().expect("failed to run command");
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
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

fn current_branch(path: &Path) -> String {
    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path);
    let output = run_success(&mut cmd);
    String::from_utf8_lossy(&output.stdout).trim().to_string()
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

    let assignments_path = repo_path.join(".swarm-hug").join("assignments.toml");
    let assignments_content = fs::read_to_string(&assignments_path).expect("read assignments.toml");
    assert!(assignments_content.contains("A = \"alpha\""));
    assert!(assignments_content.contains("B = \"alpha\""));

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
    let tasks_content = "# Tasks\n\n- [ ] Task one\n- [A] Task two\n- [x] Task three (A)\n- [ ] BLOCKED: Task four\n";
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
    let stdout = String::from_utf8_lossy(&output.stdout);

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

#[test]
fn test_swarm_merge_conflict_writes_chat_and_exits_nonzero() {
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
    let chat_path = team_root.join("chat.md");

    let file_path = repo_path.join("greeting.txt");
    fs::write(&file_path, "hello\n").expect("write base file");
    commit_all(repo_path, "base");

    let base_branch = current_branch(repo_path);

    let mut checkout_a = Command::new("git");
    checkout_a
        .args(["checkout", "-b", "agent/aaron"])
        .current_dir(repo_path);
    run_success(&mut checkout_a);
    fs::write(&file_path, "hello from aaron\n").expect("write aaron change");
    commit_all(repo_path, "aaron change");

    let mut checkout_base = Command::new("git");
    checkout_base
        .args(["checkout", &base_branch])
        .current_dir(repo_path);
    run_success(&mut checkout_base);

    let mut checkout_b = Command::new("git");
    checkout_b
        .args(["checkout", "-b", "agent/betty"])
        .current_dir(repo_path);
    run_success(&mut checkout_b);
    fs::write(&file_path, "hello from betty\n").expect("write betty change");
    commit_all(repo_path, "betty change");

    let mut checkout_base_again = Command::new("git");
    checkout_base_again
        .args(["checkout", &base_branch])
        .current_dir(repo_path);
    run_success(&mut checkout_base_again);

    let mut merge_cmd = Command::new(swarm_bin);
    merge_cmd
        .args(["--team", team_name, "merge"])
        .current_dir(repo_path);
    let merge_output = run_failure(&mut merge_cmd);
    let stderr = String::from_utf8_lossy(&merge_output.stderr);
    assert!(stderr.contains("Some merges had conflicts"));

    let chat_content = fs::read_to_string(&chat_path).expect("read CHAT.md");
    assert!(chat_content.contains("Merge conflict for Betty"));
    assert!(chat_content.contains("Conflicts in: greeting.txt"));

    let mut status_cmd = Command::new("git");
    status_cmd
        .args(["status", "--porcelain"])
        .current_dir(repo_path);
    let status_output = run_success(&mut status_cmd);
    let status_text = String::from_utf8_lossy(&status_output.stdout);
    assert!(!status_text.contains("UU"));
}
