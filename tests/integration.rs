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

    let output_dir = team_root.join("loop");
    assert!(output_dir.join("turn1-agentA.md").exists());
    assert!(output_dir.join("turn1-agentB.md").exists());

    let worktrees_dir = team_root.join("worktrees");
    assert!(worktrees_dir.join("agent-A-Aaron").exists());
    assert!(worktrees_dir.join("agent-B-Betty").exists());

    let mut branches_cmd = Command::new("git");
    branches_cmd
        .args(["branch", "--list", "agent/*"])
        .current_dir(repo_path);
    let branches_output = run_success(&mut branches_cmd);
    let branches_stdout = String::from_utf8_lossy(&branches_output.stdout);
    assert!(branches_stdout.contains("agent/aaron"));
    assert!(branches_stdout.contains("agent/betty"));

    let mut cleanup_cmd = Command::new(swarm_bin);
    cleanup_cmd
        .args(["--team", team_name, "cleanup"])
        .current_dir(repo_path);
    run_success(&mut cleanup_cmd);
    assert!(!worktrees_dir.exists());

    let mut branches_after_cmd = Command::new("git");
    branches_after_cmd
        .args(["branch", "--list", "agent/*"])
        .current_dir(repo_path);
    let branches_after = run_success(&mut branches_after_cmd);
    let branches_after_stdout = String::from_utf8_lossy(&branches_after.stdout);
    assert!(branches_after_stdout.trim().is_empty());
}
