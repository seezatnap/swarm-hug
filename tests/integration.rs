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
}

fn write_config(path: &Path) {
    let content = r#"# Swarm test config

[agents]
max_count = 2
tasks_per_agent = 1

[files]
tasks = "TASKS.md"
chat = "CHAT.md"
log_dir = "loop"

[engine]
type = "stub"
stub_mode = true

[sprints]
max = 0
"#;
    fs::write(path.join("swarm.toml"), content).expect("write swarm.toml");
}

fn write_tasks(path: &Path) -> PathBuf {
    let content = "# Tasks\n\n- [ ] Task one\n- [ ] Task two\n";
    let tasks_path = path.join("TASKS.md");
    fs::write(&tasks_path, content).expect("write TASKS.md");
    tasks_path
}

fn write_chat(path: &Path) -> PathBuf {
    let chat_path = path.join("CHAT.md");
    fs::write(&chat_path, "").expect("write CHAT.md");
    chat_path
}

#[test]
fn test_swarm_run_stub_integration() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    write_config(repo_path);
    let tasks_path = write_tasks(repo_path);
    let chat_path = write_chat(repo_path);

    let swarm_bin = env!("CARGO_BIN_EXE_swarm");
    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args(["--stub", "--max-sprints", "1", "--no-tail", "run"])
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

    let output_dir = repo_path.join("loop");
    assert!(output_dir.join("turn1-agentA.md").exists());
    assert!(output_dir.join("turn1-agentB.md").exists());

    let worktrees_dir = repo_path.join("worktrees");
    assert!(worktrees_dir.join("agent-A-Aaron").exists());
    assert!(worktrees_dir.join("agent-B-Betty").exists());

    let mut cleanup_cmd = Command::new(swarm_bin);
    cleanup_cmd.args(["cleanup"]).current_dir(repo_path);
    run_success(&mut cleanup_cmd);
    assert!(!worktrees_dir.exists());
}
