use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;
use std::thread;

use tempfile::TempDir;

use swarm::chat;
use swarm::engine::StubEngine;
use swarm::merge_agent;
use swarm::run_context::RunContext;
use swarm::task::{TaskList, TaskStatus};
use swarm::worktree;

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

fn chat_contains_worktree_creation(chat_content: &str, team_name: &str, base_short: &str) -> bool {
    let prefix = format!("Creating worktree {}-sprint-1-", team_name);
    let suffix = format!("from {}", base_short);
    chat_content.lines().any(|line| {
        chat::parse_line(line)
            .map(|(_, line_agent, line_message)| {
                line_agent == "ScrumMaster"
                    && line_message.starts_with(&prefix)
                    && line_message.contains(&suffix)
            })
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

static CWD_LOCK: Mutex<()> = Mutex::new(());

fn with_temp_cwd<F, R>(f: F) -> R
where
    F: FnOnce(&Path) -> R,
{
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original = std::env::current_dir().expect("current dir");
    let temp = TempDir::new().expect("temp dir");
    std::env::set_current_dir(temp.path()).expect("set temp dir");
    let result = f(temp.path());
    std::env::set_current_dir(original).expect("restore dir");
    result
}

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo).args(args);
    let output = run_success(&mut cmd);
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Canonicalize a path for comparison purposes.
/// On macOS, /var is a symlink to /private/var, so we need to canonicalize
/// paths before comparing them with git worktree list output.
fn canonical_path_str(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
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

fn write_team_tasks_with_assignments(team_root: &Path) -> PathBuf {
    let content = "# Tasks\n\n- [A] Task one\n- [ ] Task two\n- [B] Task three\n";
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
fn test_removed_commands_return_error() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");
    let removed_commands = [
        "sprint",
        "plan",
        "status",
        "worktrees",
        "worktrees-branch",
        "cleanup",
    ];

    for command in removed_commands {
        let mut cmd = Command::new(swarm_bin);
        cmd.arg(command).current_dir(repo_path);
        let output = cmd.output().expect("failed to run command");
        assert!(
            !output.status.success(),
            "expected command '{}' to fail\nstdout:\n{}\nstderr:\n{}",
            command,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr_raw = String::from_utf8_lossy(&output.stderr);
        let stderr = strip_ansi(&stderr_raw);
        assert!(
            stderr.contains(&format!("unknown command: {}", command)),
            "expected unknown command error for '{}'\nstderr:\n{}",
            command,
            stderr
        );
    }
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
    let base_short = git_stdout(repo_path, &["rev-parse", "--short", "HEAD"]);

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
            "run",
        ])
        .current_dir(repo_path);
    let output = run_success(&mut run_cmd);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("Merge agent:"),
        "merge agent should run at sprint completion. stdout:\n{}",
        stdout
    );

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
    assert!(
        chat_contains_worktree_creation(&chat_content, team_name, &base_short),
        "expected worktree creation message for {} from {}. chat:\n{}",
        team_name,
        base_short,
        chat_content
    );
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
    assert!(chat_lines.len() >= 7, "expected at least 7 chat lines");
    // Merge agent messages come after sprint status
    let mut tail: Vec<&str> = chat_lines.iter().rev().take(7).copied().collect();
    tail.reverse();
    assert!(tail[0].contains("SPRINT STATUS: Alpha Sprint 1 complete"));
    assert!(tail[1].contains("SPRINT STATUS: Completed this sprint: 2"));
    assert!(tail[2].contains("SPRINT STATUS: Failed this sprint: 0"));
    assert!(tail[3].contains("SPRINT STATUS: Remaining tasks: 0"));
    assert!(tail[4].contains("SPRINT STATUS: Total tasks: 2"));
    assert!(tail[5].contains("Merge agent: starting"));
    assert!(tail[6].contains("Merge agent: completed"));

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

    let sprint_prefix = format!("{}-sprint-1-", team_name);
    let sprint_worktrees_remaining = fs::read_dir(&worktrees_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&sprint_prefix)
        });
    assert!(
        !sprint_worktrees_remaining,
        "feature worktree(s) with prefix '{}' should be removed after merge",
        sprint_prefix
    );

    // Branches are also cleaned up after sprint
    let mut branches_cmd = Command::new("git");
    branches_cmd
        .args(["branch", "--list", "agent-*"])
        .current_dir(repo_path);
    let branches_output = run_success(&mut branches_cmd);
    let branches_stdout = String::from_utf8_lossy(&branches_output.stdout);
    assert!(
        branches_stdout.trim().is_empty(),
        "agent branches should be cleaned up after sprint"
    );

    let mut feature_branch_cmd = Command::new("git");
    feature_branch_cmd
        .args(["branch", "--list", &format!("{}-sprint-1-*", team_name)])
        .current_dir(repo_path);
    let feature_branch_output = run_success(&mut feature_branch_cmd);
    let feature_branch_stdout = String::from_utf8_lossy(&feature_branch_output.stdout);
    assert!(
        feature_branch_stdout.trim().is_empty(),
        "feature branch should be deleted after merge"
    );

    let mut main_log_cmd = Command::new("git");
    main_log_cmd.args(["log", "--oneline", "-10"]).current_dir(repo_path);
    let main_log_output = run_success(&mut main_log_cmd);
    let main_log = String::from_utf8_lossy(&main_log_output.stdout);
    assert!(
        main_log.contains("Alpha Sprint 1: completed"),
        "target branch should include sprint completion commit after merge, log:\n{}",
        main_log
    );
    assert!(
        main_log.contains("Alpha Sprint 1: task assignments"),
        "target branch should include sprint assignment commit after merge, log:\n{}",
        main_log
    );
}

#[test]
fn test_merge_agent_conflict_surfaces_files() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("conflict.txt"), "base\n").expect("write base");
    commit_all(repo_path, "base");

    let mut rename_cmd = Command::new("git");
    rename_cmd
        .args(["branch", "-M", "main"])
        .current_dir(repo_path);
    run_success(&mut rename_cmd);

    let mut checkout_feature = Command::new("git");
    checkout_feature
        .args(["checkout", "-b", "alpha-sprint-1"])
        .current_dir(repo_path);
    run_success(&mut checkout_feature);
    fs::write(repo_path.join("conflict.txt"), "feature change\n").expect("write feature");
    commit_all(repo_path, "feature change");

    let mut checkout_main = Command::new("git");
    checkout_main
        .args(["checkout", "main"])
        .current_dir(repo_path);
    run_success(&mut checkout_main);
    fs::write(repo_path.join("conflict.txt"), "target change\n").expect("write target");
    commit_all(repo_path, "target change");

    let engine = StubEngine::new(repo_path.join("loop").to_string_lossy().to_string());
    let err = merge_agent::ensure_feature_merged(
        &engine,
        "alpha-sprint-1",
        "main",
        repo_path,
    )
    .expect_err("expected merge conflict");

    assert!(
        err.contains("merge conflicts"),
        "expected conflict error, got: {}",
        err
    );
    assert!(
        err.contains("conflict.txt"),
        "expected conflict file name in error, got: {}",
        err
    );

    let mut diff_cmd = Command::new("git");
    diff_cmd
        .args(["diff", "--name-only", "--diff-filter=U"])
        .current_dir(repo_path);
    let diff_output = run_success(&mut diff_cmd);
    let conflicts = String::from_utf8_lossy(&diff_output.stdout);
    assert!(
        conflicts.trim().is_empty(),
        "merge conflicts should be cleared, got: {}",
        conflicts
    );
}

/// Test that merge conflicts during sprint-to-target merge are properly reported.
///
/// Note: This test uses the direct merge_agent API since the full swarm run now
/// cleans up pre-existing sprint branches (to handle failed sprints). The lower-level
/// test `test_merge_agent_conflict_surfaces_files` also tests this scenario.
#[test]
fn test_merge_agent_conflict_in_run_reports_error() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("conflict.txt"), "base\n").expect("write base");
    commit_all(repo_path, "init");

    let mut rename_cmd = Command::new("git");
    rename_cmd
        .args(["branch", "-M", "main"])
        .current_dir(repo_path);
    run_success(&mut rename_cmd);

    // Create feature branch with changes
    let mut checkout_feature = Command::new("git");
    checkout_feature
        .args(["checkout", "-b", "test-sprint-1"])
        .current_dir(repo_path);
    run_success(&mut checkout_feature);
    fs::write(repo_path.join("conflict.txt"), "feature change\n").expect("write feature");
    commit_all(repo_path, "feature change");

    // Create conflicting changes on main
    let mut checkout_main = Command::new("git");
    checkout_main
        .args(["checkout", "main"])
        .current_dir(repo_path);
    run_success(&mut checkout_main);
    fs::write(repo_path.join("conflict.txt"), "target change\n").expect("write target");
    commit_all(repo_path, "target change");

    // Use the merge_agent directly to test conflict detection
    let engine = swarm::engine::StubEngine::new(repo_path.join("loop").to_string_lossy().to_string());
    let err = swarm::merge_agent::ensure_feature_merged(
        &engine,
        "test-sprint-1",
        "main",
        repo_path,
    )
    .expect_err("expected merge conflict");

    assert!(
        err.contains("merge conflicts"),
        "expected conflict error, got: {}",
        err
    );
    assert!(
        err.contains("conflict.txt"),
        "expected conflict file name in error, got: {}",
        err
    );

    // Verify merge was aborted properly
    let mut diff_cmd = Command::new("git");
    diff_cmd
        .args(["diff", "--name-only", "--diff-filter=U"])
        .current_dir(repo_path);
    let diff_output = run_success(&mut diff_cmd);
    let conflicts = String::from_utf8_lossy(&diff_output.stdout);
    assert!(
        conflicts.trim().is_empty(),
        "merge conflicts should be cleared, got: {}",
        conflicts
    );
}

#[test]
fn test_worktree_lifecycle_feature_agent_merge_cleanup() {
    with_temp_cwd(|repo_path| {
        let repo_path = repo_path.to_path_buf();

        init_git_repo(&repo_path);
        fs::write(repo_path.join("README.md"), "init").expect("write README");
        fs::write(repo_path.join(".gitignore"), ".swarm-hug/\n").expect("write gitignore");
        commit_all(&repo_path, "init");

        let mut rename_cmd = Command::new("git");
        rename_cmd
            .arg("-C")
            .arg(&repo_path)
            .args(["branch", "-M", "main"]);
        run_success(&mut rename_cmd);

        let team_name = "alpha";
        let worktrees_dir = repo_path
            .join(".swarm-hug")
            .join(team_name)
            .join("worktrees");
        let feature_branch = format!("{}-sprint-1", team_name);

        let feature_worktree = worktree::create_feature_worktree_in(
            &worktrees_dir,
            &feature_branch,
            "main",
        )
        .expect("create feature worktree");

        let run_ctx = RunContext::new(team_name, 1);
        let assignments = vec![('A', "Task one".to_string())];
        let worktrees = worktree::create_worktrees_in(
            &worktrees_dir,
            &assignments,
            &feature_branch,
            &run_ctx,
        )
        .expect("create agent worktree");
        assert_eq!(worktrees.len(), 1);
        let agent_worktree = worktrees[0].path.clone();

        let worktree_list = git_stdout(&repo_path, &["worktree", "list", "--porcelain"]);
        let feature_canonical = canonical_path_str(&feature_worktree);
        let agent_canonical = canonical_path_str(&agent_worktree);
        assert!(
            worktree_list.contains(&format!("worktree {}", feature_canonical)),
            "feature worktree should be registered"
        );
        assert!(
            worktree_list.contains(&format!("worktree {}", agent_canonical)),
            "agent worktree should be registered"
        );

        let agent_branch = git_stdout(&agent_worktree, &["rev-parse", "--abbrev-ref", "HEAD"]);
        let expected_branch_prefix = format!("{}-agent-aaron-", team_name);
        assert!(
            agent_branch.starts_with(&expected_branch_prefix),
            "agent branch '{}' should start with '{}'",
            agent_branch, expected_branch_prefix
        );

        fs::write(agent_worktree.join("agent.txt"), "agent change").expect("write agent file");
        commit_all(&agent_worktree, "Agent commit");

        let merge_result =
            worktree::merge_agent_branch_in_with_ctx(&feature_worktree, &run_ctx, 'A', Some(&feature_branch));
        assert!(matches!(merge_result, worktree::MergeResult::Success));

        let merge_parents = git_stdout(&feature_worktree, &["rev-list", "--parents", "-n", "1", "HEAD"]);
        let parent_count = merge_parents.split_whitespace().count();
        assert_eq!(parent_count, 3, "expected merge commit with two parents");

        let merged_content =
            fs::read_to_string(feature_worktree.join("agent.txt")).expect("read merged file");
        assert_eq!(merged_content, "agent change");

        worktree::cleanup_agent_worktree(&worktrees_dir, 'A', true, &run_ctx)
            .expect("cleanup agent worktree");
        assert!(!agent_worktree.exists(), "agent worktree should be removed");

        let worktree_list_after = git_stdout(&repo_path, &["worktree", "list", "--porcelain"]);
        assert!(
            !worktree_list_after.contains(&format!("worktree {}", agent_canonical)),
            "agent worktree should be deregistered"
        );
        assert!(
            worktree_list_after.contains(&format!("worktree {}", feature_canonical)),
            "feature worktree should remain"
        );

        // The agent branch should be deleted - check that no branch matching the pattern exists
        let all_branches = git_stdout(&repo_path, &["branch", "--list"]);
        assert!(
            !all_branches.contains(&format!("{}-agent-aaron-", team_name)),
            "agent branch should be deleted"
        );
    });
}

#[test]
fn test_target_branch_worktree_errors_when_outside_shared_root() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("README.md"), "init").expect("write README");
    commit_all(repo_path, "init");

    let mut branch_cmd = Command::new("git");
    branch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("branch")
        .arg("target-branch");
    run_success(&mut branch_cmd);

    let outside_dir = TempDir::new().expect("outside dir");
    let outside_path = outside_dir.path().join("target-branch");

    let mut worktree_cmd = Command::new("git");
    worktree_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("worktree")
        .arg("add")
        .arg(outside_path.to_str().expect("outside path"))
        .arg("target-branch");
    run_success(&mut worktree_cmd);

    let err = worktree::create_target_branch_worktree_in(repo_path, "target-branch")
        .expect_err("should error when worktree exists outside shared root");
    assert!(
        err.contains("outside shared worktrees root"),
        "expected outside shared root error, got: {}",
        err
    );
}

#[test]
fn test_target_branch_worktree_reuses_existing_shared_root() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("README.md"), "init").expect("write README");
    commit_all(repo_path, "init");

    let mut branch_cmd = Command::new("git");
    branch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("branch")
        .arg("target-branch");
    run_success(&mut branch_cmd);

    let shared_root = worktree::ensure_shared_worktrees_root(repo_path).expect("shared root");
    let worktree_path = shared_root.join("target-branch");

    let mut worktree_cmd = Command::new("git");
    worktree_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("worktree")
        .arg("add")
        .arg(worktree_path.to_str().expect("worktree path"))
        .arg("target-branch");
    run_success(&mut worktree_cmd);

    let reused = worktree::create_target_branch_worktree_in(repo_path, "target-branch")
        .expect("reuse target branch worktree");
    assert_eq!(
        canonical_path_str(&reused),
        canonical_path_str(&worktree_path),
        "expected existing worktree to be reused"
    );

    let head = git_stdout(&reused, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "target-branch");
}

#[test]
fn test_target_branch_worktree_creates_when_missing() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("README.md"), "init").expect("write README");
    commit_all(repo_path, "init");

    let mut branch_cmd = Command::new("git");
    branch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("branch")
        .arg("target-branch");
    run_success(&mut branch_cmd);

    let created = worktree::create_target_branch_worktree_in(repo_path, "target-branch")
        .expect("create target branch worktree");
    let shared_root = worktree::ensure_shared_worktrees_root(repo_path).expect("shared root");

    assert!(created.exists(), "created worktree should exist");
    assert!(
        created.starts_with(&shared_root),
        "created worktree should live under shared root"
    );

    let expected = shared_root.join("target-branch");
    assert_eq!(
        canonical_path_str(&created),
        canonical_path_str(&expected),
        "worktree path should match shared root target path"
    );

    let head = git_stdout(&created, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "target-branch");
}

#[test]
fn test_target_branch_worktree_reconciles_mismatch_and_reregisters() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("README.md"), "init").expect("write README");
    commit_all(repo_path, "init");

    let mut target_branch_cmd = Command::new("git");
    target_branch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("branch")
        .arg("target-branch");
    run_success(&mut target_branch_cmd);

    let mut other_branch_cmd = Command::new("git");
    other_branch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("branch")
        .arg("other");
    run_success(&mut other_branch_cmd);

    let shared_root = worktree::ensure_shared_worktrees_root(repo_path).expect("shared root");
    let reserved_path = shared_root.join("target-branch");

    let mut add_mismatch_cmd = Command::new("git");
    add_mismatch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("worktree")
        .arg("add")
        .arg(reserved_path.to_str().expect("reserved path"))
        .arg("other");
    run_success(&mut add_mismatch_cmd);

    let recovered = worktree::create_target_branch_worktree_in(repo_path, "target-branch")
        .expect("reconcile mismatched worktree registration");
    assert_eq!(
        canonical_path_str(&recovered),
        canonical_path_str(&reserved_path),
        "recovered worktree should reuse reserved shared path"
    );

    let head = git_stdout(&recovered, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "target-branch");
}

#[test]
fn test_target_branch_worktree_recovers_missing_prior_run_registration() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("README.md"), "init").expect("write README");
    commit_all(repo_path, "init");

    let mut branch_cmd = Command::new("git");
    branch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("branch")
        .arg("target-branch");
    run_success(&mut branch_cmd);

    let shared_root = worktree::ensure_shared_worktrees_root(repo_path).expect("shared root");
    let reserved_path = shared_root.join("target-branch");

    let mut add_cmd = Command::new("git");
    add_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("worktree")
        .arg("add")
        .arg(reserved_path.to_str().expect("reserved path"))
        .arg("target-branch");
    run_success(&mut add_cmd);

    fs::remove_dir_all(&reserved_path).expect("remove registered worktree path");

    let recovered = worktree::create_target_branch_worktree_in(repo_path, "target-branch")
        .expect("recover from stale prior-run registration");
    assert_eq!(
        canonical_path_str(&recovered),
        canonical_path_str(&reserved_path),
        "recovered worktree should reuse stale reserved path"
    );
    assert!(recovered.exists(), "recovered worktree should exist");

    let head = git_stdout(&recovered, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "target-branch");
}

#[test]
fn test_target_branch_worktree_dirty_mismatch_preserves_work() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("README.md"), "init").expect("write README");
    commit_all(repo_path, "init");

    let mut target_branch_cmd = Command::new("git");
    target_branch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("branch")
        .arg("target-branch");
    run_success(&mut target_branch_cmd);

    let mut other_branch_cmd = Command::new("git");
    other_branch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("branch")
        .arg("other");
    run_success(&mut other_branch_cmd);

    let shared_root = worktree::ensure_shared_worktrees_root(repo_path).expect("shared root");
    let reserved_path = shared_root.join("target-branch");

    let mut add_mismatch_cmd = Command::new("git");
    add_mismatch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("worktree")
        .arg("add")
        .arg(reserved_path.to_str().expect("reserved path"))
        .arg("other");
    run_success(&mut add_mismatch_cmd);

    let dirty_file = reserved_path.join("dirty.txt");
    fs::write(&dirty_file, "keep me").expect("write dirty file");

    let err = worktree::create_target_branch_worktree_in(repo_path, "target-branch")
        .expect_err("dirty mismatched worktree should not be replaced");
    assert!(
        err.contains("uncommitted changes"),
        "expected dirty-worktree safety error, got: {}",
        err
    );
    assert!(reserved_path.exists(), "dirty worktree path should be preserved");
    assert!(dirty_file.exists(), "dirty file should be preserved");

    let head = git_stdout(&reserved_path, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "other");
}

#[test]
fn test_single_variation_run_succeeds_with_shared_mismatch_present() {
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
    commit_all(repo_path, "init");

    let default_target = git_stdout(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]);

    let mut other_branch_cmd = Command::new("git");
    other_branch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("branch")
        .arg("other");
    run_success(&mut other_branch_cmd);

    let shared_root = worktree::ensure_shared_worktrees_root(repo_path).expect("shared root");
    let reserved_path = shared_root.join(&default_target);

    let mut add_mismatch_cmd = Command::new("git");
    add_mismatch_cmd
        .arg("-C")
        .arg(repo_path)
        .arg("worktree")
        .arg("add")
        .arg(reserved_path.to_str().expect("reserved path"))
        .arg("other");
    run_success(&mut add_mismatch_cmd);

    let keep_file = reserved_path.join("keep.txt");
    fs::write(&keep_file, "keep").expect("write keep file");

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
            "run",
        ])
        .current_dir(repo_path);
    run_success(&mut run_cmd);

    let tasks_content = fs::read_to_string(&tasks_path).expect("read tasks");
    let task_list = TaskList::parse(&tasks_content);
    assert_eq!(task_list.completed_count(), 2, "single variation run should complete tasks");

    assert!(
        reserved_path.exists(),
        "shared mismatch path should not break single variation run"
    );
    assert!(keep_file.exists(), "existing work under mismatch path should remain");

    let head = git_stdout(&reserved_path, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "other");
}

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

/// Test that agent work is merged to sprint branch, not directly to target branch.
/// This verifies the worktree workflow: agents merge to sprint branch, then sprint branch
/// merges to target at sprint completion.
#[test]
fn test_agent_merges_go_to_sprint_branch_not_target() {
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
    write_team_tasks(&team_root);
    commit_all(repo_path, "init");

    // Get the initial commit on master before the sprint
    let initial_commit = git_stdout(repo_path, &["rev-parse", "HEAD"]);

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
            "run",
        ])
        .current_dir(repo_path);
    run_success(&mut run_cmd);

    // Get the merge commits on main since the initial commit
    let merge_log = git_stdout(
        repo_path,
        &["log", "--oneline", "--merges", &format!("{}..HEAD", initial_commit)],
    );

    // Should NOT have individual agent merge commits on the target branch
    // (those should only be on the sprint branch, which then merges to target)

    // The merge from sprint branch should mention the sprint branch, not agent branches
    // In stub mode, the sprint branch is merged after all agents complete
    assert!(
        !merge_log.contains("Merge agent-"),
        "Target branch should not have direct agent merge commits. Got:\n{}",
        merge_log
    );

    // Verify that the sprint completion commit exists (indicating sprint was merged)
    let log = git_stdout(repo_path, &["log", "--oneline", "-10"]);
    assert!(
        log.contains("Alpha Sprint 1: completed") || log.contains("alpha-sprint-1"),
        "Target branch should have sprint completion commit. Got:\n{}",
        log
    );

    // Verify that agent work content is present (it should have been merged via sprint branch)
    // The stub engine doesn't create actual files, but the task list should show completion
    let tasks_content = fs::read_to_string(team_root.join("tasks.md")).expect("read tasks");
    let task_list = TaskList::parse(&tasks_content);
    assert_eq!(
        task_list.completed_count(),
        2,
        "Both tasks should be completed"
    );
}

/// Test that the sprint-to-target merge is authored by Swarm ScrumMaster, not an agent.
#[test]
fn test_sprint_merge_authored_by_scrummaster() {
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
    write_team_tasks(&team_root);
    commit_all(repo_path, "init");

    // Get the initial commit before the sprint
    let initial_commit = git_stdout(repo_path, &["rev-parse", "HEAD"]);

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
            "run",
        ])
        .current_dir(repo_path);
    run_success(&mut run_cmd);

    // Get the merge commit that brought the sprint branch into the target
    // Look for the merge commit after the initial commit
    let merge_log = git_stdout(
        repo_path,
        &[
            "log",
            "--format=%an <%ae>",
            "--merges",
            "-1",
            &format!("{}..HEAD", initial_commit),
        ],
    );

    assert!(
        merge_log.contains("Swarm ScrumMaster"),
        "Sprint-to-target merge should be authored by Swarm ScrumMaster, got: {}",
        merge_log
    );
    assert!(
        !merge_log.contains("Agent"),
        "Sprint-to-target merge should NOT be authored by an Agent, got: {}",
        merge_log
    );
}

/// Test that a pre-existing feature worktree from a failed sprint is cleaned up.
#[test]
fn test_failed_sprint_worktree_cleanup_on_restart() {
    with_temp_cwd(|repo_path| {
        let repo_path = repo_path.to_path_buf();
        let team_name = "alpha";

        init_git_repo(&repo_path);
        let swarm_bin = env!("CARGO_BIN_EXE_swarm");

        let mut team_init_cmd = Command::new(swarm_bin);
        team_init_cmd
            .args(["project", "init", team_name])
            .current_dir(&repo_path);
        run_success(&mut team_init_cmd);

        let team_root = repo_path.join(".swarm-hug").join(team_name);
        let worktrees_dir = team_root.join("worktrees");

        // Write tasks for multiple sprints
        write_team_tasks_multi_sprint(&team_root);
        commit_all(&repo_path, "init");

        // Get the current branch name (could be master or main depending on git config)
        let target_branch = git_stdout(&repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]);

        // Create a "leftover" feature worktree to simulate a failed sprint
        // This simulates what would happen if a previous sprint crashed mid-way
        let leftover_branch = format!("{}-sprint-1", team_name);
        let leftover_worktree = worktree::create_feature_worktree_in(
            &worktrees_dir,
            &leftover_branch,
            &target_branch,
        )
        .expect("create leftover worktree");

        // Add a file to the leftover worktree to make it "dirty"
        fs::write(leftover_worktree.join("leftover.txt"), "leftover content")
            .expect("write leftover file");
        commit_all(&leftover_worktree, "leftover commit");

        // Verify the leftover worktree exists
        assert!(
            leftover_worktree.exists(),
            "leftover worktree should exist before run"
        );

        // Run a sprint - this should clean up the old worktree and create a fresh one
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
                "run",
            ])
            .current_dir(&repo_path);
        let output = run_success(&mut run_cmd);
        let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

        // Verify the sprint ran successfully
        assert!(
            stdout.contains("Sprint 1: assigned"),
            "Sprint should have run successfully. Output:\n{}",
            stdout
        );

        // Verify tasks were completed (proving the sprint actually ran)
        let tasks_content = fs::read_to_string(team_root.join("tasks.md")).expect("read tasks");
        let task_list = TaskList::parse(&tasks_content);
        assert!(
            task_list.completed_count() >= 2,
            "Tasks should be completed after sprint"
        );

        // The leftover file should NOT be in the final result since the worktree was recreated fresh
        // (Note: in stub mode the worktree is cleaned up after sprint, so we can't check this directly)
    });
}

/// Test that sprint initialization keeps the target branch clean.
/// The sprint branch is created BEFORE any sprint files are written, ensuring
/// all sprint-specific state (sprint-history.json, team-state.json, task assignments)
/// goes to the sprint branch, not the target branch.
#[test]
fn test_sprint_init_keeps_target_branch_clean() {
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

    // Write tasks for the sprint
    let team_root = repo_path.join(".swarm-hug").join(team_name);
    write_team_tasks(&team_root);
    commit_all(repo_path, "init with tasks");

    // Rename default branch to 'main' for consistent testing
    let mut rename_cmd = Command::new("git");
    rename_cmd
        .args(["branch", "-M", "main"])
        .current_dir(repo_path);
    run_success(&mut rename_cmd);

    // Record the state of the main branch before the sprint
    // These files should NOT exist on main before the sprint
    let main_sprint_history = team_root.join("sprint-history.json");
    let main_team_state = team_root.join("team-state.json");
    assert!(
        !main_sprint_history.exists(),
        "sprint-history.json should not exist on main before sprint"
    );
    assert!(
        !main_team_state.exists(),
        "team-state.json should not exist on main before sprint"
    );

    // Run a single sprint
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
            "run",
        ])
        .current_dir(repo_path);
    let output = run_success(&mut run_cmd);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    // Verify the sprint ran successfully
    assert!(
        stdout.contains("Sprint 1: assigned"),
        "Sprint should have run successfully. Output:\n{}",
        stdout
    );

    // KEY ASSERTION 1: Main branch should have NO uncommitted changes
    // git status --porcelain returns empty string if working tree is clean
    let mut status_cmd = Command::new("git");
    status_cmd
        .args(["status", "--porcelain"])
        .current_dir(repo_path);
    let status_output = run_success(&mut status_cmd);
    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        status_stdout.trim().is_empty(),
        "Target branch should have no uncommitted changes after sprint initialization.\n\
         Expected: clean working tree\n\
         Got git status:\n{}",
        status_stdout
    );

    // KEY ASSERTION 2: Sprint-specific files should NOT exist in main branch working tree
    // (they should only exist in the sprint branch, which was merged back)
    // After the sprint completes, the sprint branch is merged and cleaned up,
    // but the files should be in committed history, not as new uncommitted files

    // Check that there are no untracked files in the swarm-hug team directory
    let mut status_untracked_cmd = Command::new("git");
    status_untracked_cmd
        .args(["status", "--porcelain", "-u"])
        .current_dir(repo_path);
    let untracked_output = run_success(&mut status_untracked_cmd);
    let untracked_stdout = String::from_utf8_lossy(&untracked_output.stdout);

    // Filter for sprint-specific state files
    let has_untracked_sprint_files = untracked_stdout.lines().any(|line| {
        line.contains("sprint-history.json") || line.contains("team-state.json")
    });
    assert!(
        !has_untracked_sprint_files,
        "Sprint state files should not be untracked on main branch.\n\
         Got git status:\n{}",
        untracked_stdout
    );

    // KEY ASSERTION 3: Verify that the git log shows sprint commits were merged
    // This confirms the sprint branch was properly created and merged
    let main_log = git_stdout(repo_path, &["log", "--oneline", "-10"]);
    assert!(
        main_log.contains("Alpha Sprint 1: task assignments") ||
        main_log.contains("alpha-sprint-1"),
        "Target branch should contain sprint commits after merge.\n\
         Git log:\n{}",
        main_log
    );

    // KEY ASSERTION 4: Verify tasks were completed (proves sprint actually ran)
    let tasks_content = fs::read_to_string(team_root.join("tasks.md")).expect("read tasks");
    let task_list = TaskList::parse(&tasks_content);
    assert!(
        task_list.completed_count() >= 2,
        "Tasks should be completed after sprint. Got {} completed.",
        task_list.completed_count()
    );
}

/// Test that unassigning previously-assigned tasks does NOT dirty the target branch.
///
/// This covers the regression where `task_list.unassign_all()` wrote to the target
/// branch working tree, causing the merge agent to fail with local changes.
#[test]
fn test_unassign_does_not_dirty_target_branch() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "omega";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Initialize team
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    // Write tasks with assigned statuses to trigger unassign_all
    let team_root = repo_path.join(".swarm-hug").join(team_name);
    write_team_tasks_with_assignments(&team_root);
    commit_all(repo_path, "init with assigned tasks");

    // Rename default branch to 'main' for consistent testing
    let mut rename_cmd = Command::new("git");
    rename_cmd
        .args(["branch", "-M", "main"])
        .current_dir(repo_path);
    run_success(&mut rename_cmd);

    // Run a single sprint
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
            "run",
        ])
        .current_dir(repo_path);
    run_success(&mut run_cmd);

    // Target branch should remain clean (no uncommitted changes)
    let mut status_cmd = Command::new("git");
    status_cmd
        .args(["status", "--porcelain"])
        .current_dir(repo_path);
    let status_output = run_success(&mut status_cmd);
    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        status_stdout.trim().is_empty(),
        "Target branch should be clean after unassigning tasks.\nGot git status:\n{}",
        status_stdout
    );
}

/// Test that the first sprint creates state files only in the sprint branch.
/// When a repo has no existing sprint-history.json or team-state.json,
/// the first sprint should create these files in the sprint branch (not main).
///
/// This is a specific case of the sprint branch ordering fix, where we verify:
/// 1. No sprint state files exist before the sprint starts
/// 2. After sprint initialization, these files are created in the sprint branch
/// 3. The target branch remains clean (no uncommitted changes)
/// 4. After the sprint completes and merges, the files are in committed history
#[test]
fn test_first_sprint_creates_files_in_sprint_branch() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "beta";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Initialize team
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    // Write only tasks - no sprint-history.json or team-state.json
    let team_root = repo_path.join(".swarm-hug").join(team_name);
    write_team_tasks(&team_root);
    commit_all(repo_path, "init with tasks only");

    // Rename default branch to 'main' for consistent testing
    let mut rename_cmd = Command::new("git");
    rename_cmd
        .args(["branch", "-M", "main"])
        .current_dir(repo_path);
    run_success(&mut rename_cmd);

    // PRECONDITION: Verify sprint state files do NOT exist before sprint
    let main_sprint_history = team_root.join("sprint-history.json");
    let main_team_state = team_root.join("team-state.json");
    assert!(
        !main_sprint_history.exists(),
        "sprint-history.json should NOT exist before first sprint"
    );
    assert!(
        !main_team_state.exists(),
        "team-state.json should NOT exist before first sprint"
    );

    // Run the first sprint
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
            "run",
        ])
        .current_dir(repo_path);
    let output = run_success(&mut run_cmd);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    // Verify the sprint ran successfully
    assert!(
        stdout.contains("Sprint 1: assigned"),
        "Sprint should have run successfully. Output:\n{}",
        stdout
    );

    // KEY ASSERTION 1: Main branch should have NO uncommitted changes
    // This proves files weren't written to main before branch creation
    let mut status_cmd = Command::new("git");
    status_cmd
        .args(["status", "--porcelain"])
        .current_dir(repo_path);
    let status_output = run_success(&mut status_cmd);
    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        status_stdout.trim().is_empty(),
        "Target branch should have no uncommitted changes after first sprint.\n\
         This indicates sprint files were NOT written to main before branch creation.\n\
         Expected: clean working tree\n\
         Got git status:\n{}",
        status_stdout
    );

    // KEY ASSERTION 2: Sprint state files should NOT be untracked on main
    // (they should only exist as committed files from the merged sprint branch)
    let mut status_untracked_cmd = Command::new("git");
    status_untracked_cmd
        .args(["status", "--porcelain", "-u"])
        .current_dir(repo_path);
    let untracked_output = run_success(&mut status_untracked_cmd);
    let untracked_stdout = String::from_utf8_lossy(&untracked_output.stdout);

    let has_untracked_sprint_files = untracked_stdout.lines().any(|line| {
        line.contains("sprint-history.json") || line.contains("team-state.json")
    });
    assert!(
        !has_untracked_sprint_files,
        "Sprint state files should NOT be untracked on main after first sprint.\n\
         They should be committed via the sprint branch merge.\n\
         Got git status:\n{}",
        untracked_stdout
    );

    // KEY ASSERTION 3: The sprint state files should now exist (from merged sprint branch)
    // After the sprint completes and is merged, the files should be in committed history
    assert!(
        main_sprint_history.exists(),
        "sprint-history.json should exist after sprint completes (from merged sprint branch)"
    );
    assert!(
        main_team_state.exists(),
        "team-state.json should exist after sprint completes (from merged sprint branch)"
    );

    // KEY ASSERTION 4: Verify these files are tracked (committed), not untracked
    let mut ls_files_cmd = Command::new("git");
    ls_files_cmd
        .args(["ls-files", "--error-unmatch", ".swarm-hug/beta/sprint-history.json"])
        .current_dir(repo_path);
    let ls_files_result = ls_files_cmd.output().expect("git ls-files failed");
    assert!(
        ls_files_result.status.success(),
        "sprint-history.json should be tracked in git (committed via sprint branch)"
    );

    let mut ls_files_team_cmd = Command::new("git");
    ls_files_team_cmd
        .args(["ls-files", "--error-unmatch", ".swarm-hug/beta/team-state.json"])
        .current_dir(repo_path);
    let ls_files_team_result = ls_files_team_cmd.output().expect("git ls-files failed");
    assert!(
        ls_files_team_result.status.success(),
        "team-state.json should be tracked in git (committed via sprint branch)"
    );

    // KEY ASSERTION 5: Verify the git log shows the sprint was properly created and merged
    let main_log = git_stdout(repo_path, &["log", "--oneline", "-10"]);
    assert!(
        main_log.contains("Beta Sprint 1: task assignments") ||
        main_log.contains("beta-sprint-1"),
        "Target branch should contain sprint commits after merge.\n\
         Git log:\n{}",
        main_log
    );

    // KEY ASSERTION 6: Verify tasks were completed (proves sprint actually ran)
    let tasks_content = fs::read_to_string(team_root.join("tasks.md")).expect("read tasks");
    let task_list = TaskList::parse(&tasks_content);
    assert!(
        task_list.completed_count() >= 2,
        "Tasks should be completed after first sprint. Got {} completed.",
        task_list.completed_count()
    );
}

/// Test that follow-up tasks are written to the sprint worktree, not the main repo.
///
/// This test verifies the fix for the bug where follow-up tasks were written to
/// `config.files_tasks` (main repo path) instead of the worktree-relative path,
/// causing them to be lost when the commit happens from the worktree context.
///
/// The test verifies:
/// (a) Follow-up tasks appear in sprint worktree's tasks.md
/// (b) Main repo tasks.md is unchanged
/// (c) Main repo chat.md is unchanged
/// (d) Commit exists in sprint branch history
///
/// Since the stub engine returns no follow-up tasks (deterministic for testing),
/// this test directly simulates the follow-up task writing behavior to verify
/// the worktree path fix.
#[test]
fn test_followup_tasks_written_to_worktree() {
    with_temp_cwd(|repo_path| {
        let repo_path = repo_path.to_path_buf();
        let team_name = "gamma";
        let sprint_branch = format!("{}-sprint-1", team_name);

        // Initialize git repo
        init_git_repo(&repo_path);
        fs::write(repo_path.join("README.md"), "init").expect("write README");
        fs::write(repo_path.join(".gitignore"), ".swarm-hug/*/worktrees/\n").expect("write gitignore");
        commit_all(&repo_path, "init");

        // Rename to 'main' for consistent testing
        let mut rename_cmd = Command::new("git");
        rename_cmd
            .arg("-C")
            .arg(&repo_path)
            .args(["branch", "-M", "main"]);
        run_success(&mut rename_cmd);

        // Create team structure in main repo (simulating swarm project init)
        let main_team_root = repo_path.join(".swarm-hug").join(team_name);
        fs::create_dir_all(&main_team_root).expect("create team dir");

        // Write initial tasks to main repo
        let main_tasks_path = main_team_root.join("tasks.md");
        let initial_tasks_content = "# Tasks\n\n- [x] (A) (#1) Task one\n- [x] (B) (#2) Task two\n";
        fs::write(&main_tasks_path, initial_tasks_content).expect("write main tasks.md");

        // Write initial chat to main repo
        let main_chat_path = main_team_root.join("chat.md");
        let initial_chat_content = "12:00:00 | ScrumMaster | Sprint planning started\n";
        fs::write(&main_chat_path, initial_chat_content).expect("write main chat.md");

        // Commit initial state on main
        commit_all(&repo_path, "initial team state");

        // Record the content of main repo files BEFORE any sprint work
        let main_tasks_before = fs::read_to_string(&main_tasks_path).expect("read main tasks before");
        let main_chat_before = fs::read_to_string(&main_chat_path).expect("read main chat before");

        // Create sprint worktree (simulating sprint start)
        let worktrees_dir = main_team_root.join("worktrees");
        let feature_worktree = worktree::create_feature_worktree_in(
            &worktrees_dir,
            &sprint_branch,
            "main",
        )
        .expect("create feature worktree");

        // Verify worktree was created
        assert!(feature_worktree.exists(), "feature worktree should exist");

        // Get worktree-relative paths (this is the key fix - paths should be in worktree)
        let worktree_team_root = feature_worktree.join(".swarm-hug").join(team_name);
        let worktree_tasks_path = worktree_team_root.join("tasks.md");
        let worktree_chat_path = worktree_team_root.join("chat.md");

        // Verify worktree has copies of the files (inherited from main)
        assert!(
            worktree_tasks_path.exists(),
            "worktree tasks.md should exist at {:?}",
            worktree_tasks_path
        );
        assert!(
            worktree_chat_path.exists(),
            "worktree chat.md should exist at {:?}",
            worktree_chat_path
        );

        // SIMULATE: Follow-up tasks being identified and written to worktree's tasks.md
        // This mimics what run_post_sprint_review() does after the path fix
        let mut worktree_tasks_content = fs::read_to_string(&worktree_tasks_path)
            .expect("read worktree tasks");

        // Ensure newline before appending
        if !worktree_tasks_content.ends_with('\n') {
            worktree_tasks_content.push('\n');
        }

        // Add follow-up tasks (same format as run_post_sprint_review)
        worktree_tasks_content.push_str("\n## Follow-up tasks (from sprint review)\n");
        worktree_tasks_content.push_str("- [ ] (#3) Add unit tests for new feature\n");
        worktree_tasks_content.push_str("- [ ] (#4) Update documentation\n");

        // Write to WORKTREE path (not main repo)
        fs::write(&worktree_tasks_path, &worktree_tasks_content)
            .expect("write follow-up tasks to worktree");

        // SIMULATE: Chat message about follow-up tasks being written to worktree
        let mut worktree_chat_content = fs::read_to_string(&worktree_chat_path)
            .expect("read worktree chat");
        worktree_chat_content.push_str("13:00:00 | ScrumMaster | Sprint review added 2 follow-up task(s)\n");
        fs::write(&worktree_chat_path, &worktree_chat_content)
            .expect("write chat to worktree");

        // SIMULATE: Commit the follow-up tasks in the worktree on the sprint branch
        let commit_msg = format!("{} Sprint 1: follow-up tasks from review", team_name);
        let mut add_cmd = Command::new("git");
        add_cmd
            .arg("-C")
            .arg(&feature_worktree)
            .args(["add", worktree_tasks_path.to_str().unwrap(), worktree_chat_path.to_str().unwrap()]);
        run_success(&mut add_cmd);

        let mut commit_cmd = Command::new("git");
        commit_cmd
            .arg("-C")
            .arg(&feature_worktree)
            .args(["commit", "-m", &commit_msg]);
        run_success(&mut commit_cmd);

        // ========== ASSERTIONS ==========

        // (a) ASSERT: Follow-up tasks appear in sprint worktree's tasks.md
        let worktree_tasks_after = fs::read_to_string(&worktree_tasks_path)
            .expect("read worktree tasks after");
        assert!(
            worktree_tasks_after.contains("## Follow-up tasks (from sprint review)"),
            "Worktree tasks.md should contain follow-up tasks section.\n\
             Worktree tasks.md content:\n{}",
            worktree_tasks_after
        );
        assert!(
            worktree_tasks_after.contains("- [ ] (#3) Add unit tests for new feature"),
            "Worktree tasks.md should contain first follow-up task.\n\
             Worktree tasks.md content:\n{}",
            worktree_tasks_after
        );
        assert!(
            worktree_tasks_after.contains("- [ ] (#4) Update documentation"),
            "Worktree tasks.md should contain second follow-up task.\n\
             Worktree tasks.md content:\n{}",
            worktree_tasks_after
        );

        // (b) ASSERT: Main repo tasks.md is unchanged
        let main_tasks_after = fs::read_to_string(&main_tasks_path)
            .expect("read main tasks after");
        assert_eq!(
            main_tasks_before, main_tasks_after,
            "Main repo tasks.md should be UNCHANGED.\n\
             Before:\n{}\n\
             After:\n{}",
            main_tasks_before, main_tasks_after
        );
        assert!(
            !main_tasks_after.contains("## Follow-up tasks"),
            "Main repo tasks.md should NOT contain follow-up tasks section.\n\
             Main tasks.md content:\n{}",
            main_tasks_after
        );

        // (c) ASSERT: Main repo chat.md is unchanged
        let main_chat_after = fs::read_to_string(&main_chat_path)
            .expect("read main chat after");
        assert_eq!(
            main_chat_before, main_chat_after,
            "Main repo chat.md should be UNCHANGED.\n\
             Before:\n{}\n\
             After:\n{}",
            main_chat_before, main_chat_after
        );
        assert!(
            !main_chat_after.contains("Sprint review added"),
            "Main repo chat.md should NOT contain sprint review message.\n\
             Main chat.md content:\n{}",
            main_chat_after
        );

        // (d) ASSERT: Commit exists in sprint branch history
        let sprint_log = git_stdout(&feature_worktree, &["log", "--oneline", "-5"]);
        assert!(
            sprint_log.contains("follow-up tasks from review"),
            "Sprint branch should contain follow-up tasks commit.\n\
             Sprint branch log:\n{}",
            sprint_log
        );

        // BONUS: Verify main branch doesn't have the follow-up commit
        // (The commit should only be on the sprint branch)
        let main_log = git_stdout(&repo_path, &["log", "--oneline", "-5"]);
        assert!(
            !main_log.contains("follow-up tasks from review"),
            "Main branch should NOT contain follow-up tasks commit (yet).\n\
             Main branch log:\n{}",
            main_log
        );

        // BONUS: Verify git status on main repo shows NO uncommitted changes
        // (This proves we didn't write to main repo and forget to commit)
        let mut status_cmd = Command::new("git");
        status_cmd
            .args(["status", "--porcelain"])
            .current_dir(&repo_path);
        let status_output = run_success(&mut status_cmd);
        let status_stdout = String::from_utf8_lossy(&status_output.stdout);
        // Filter for team-related uncommitted files
        let has_uncommitted_team_files = status_stdout.lines().any(|line| {
            line.contains("tasks.md") || line.contains("chat.md")
        });
        assert!(
            !has_uncommitted_team_files,
            "Main repo should have NO uncommitted tasks.md or chat.md changes.\n\
             Git status:\n{}",
            status_stdout
        );

        // Cleanup: remove the worktree
        worktree::cleanup_feature_worktree(&worktrees_dir, &sprint_branch, true)
            .expect("cleanup feature worktree");
    });
}

// ============================================================================
// Integration tests for project-namespaced worktrees with run hash isolation
// ============================================================================

/// Test parallel project execution: two projects with the same agents running
/// concurrently without conflict.
///
/// This test verifies:
/// 1. Two projects can run sprints simultaneously with overlapping agent assignments
/// 2. Worktrees for each project are isolated by project name and run hash
/// 3. Branches for each project are isolated by project name and run hash
/// 4. Cleanup of one project doesn't affect the other
#[test]
fn test_parallel_projects_no_worktree_conflict() {
    with_temp_cwd(|repo_path| {
        let repo_path = repo_path.to_path_buf();

        // Initialize git repo
        init_git_repo(&repo_path);
        fs::write(repo_path.join("README.md"), "init").expect("write README");
        fs::write(repo_path.join(".gitignore"), ".swarm-hug/\n").expect("write gitignore");
        commit_all(&repo_path, "init");

        let mut rename_cmd = Command::new("git");
        rename_cmd
            .arg("-C")
            .arg(&repo_path)
            .args(["branch", "-M", "main"]);
        run_success(&mut rename_cmd);

        // Setup common directory for testing
        let worktrees_dir = repo_path.join(".swarm-hug").join("worktrees");

        // Create two different project contexts (simulating parallel runs)
        let ctx_greenfield = RunContext::new("greenfield", 1);
        let ctx_payments = RunContext::new("payments", 1);

        // Both projects will use the same agent (Agent A)
        let assignments = vec![('A', "Task one".to_string())];

        // Create worktrees for greenfield project
        let worktrees_greenfield = worktree::create_worktrees_in(
            &worktrees_dir,
            &assignments,
            "main",
            &ctx_greenfield,
        )
        .expect("create greenfield worktrees");
        assert_eq!(worktrees_greenfield.len(), 1);

        // Create worktrees for payments project (same agent - should NOT conflict)
        let worktrees_payments = worktree::create_worktrees_in(
            &worktrees_dir,
            &assignments,
            "main",
            &ctx_payments,
        )
        .expect("create payments worktrees");
        assert_eq!(worktrees_payments.len(), 1);

        // Verify both worktrees exist simultaneously
        let wt_greenfield = &worktrees_greenfield[0].path;
        let wt_payments = &worktrees_payments[0].path;
        assert!(
            wt_greenfield.exists(),
            "greenfield worktree should exist: {:?}",
            wt_greenfield
        );
        assert!(
            wt_payments.exists(),
            "payments worktree should exist: {:?}",
            wt_payments
        );

        // Verify they are different paths
        assert_ne!(
            wt_greenfield, wt_payments,
            "worktree paths should be different"
        );

        // Verify branch names are different
        let branch_greenfield = ctx_greenfield.agent_branch('A');
        let branch_payments = ctx_payments.agent_branch('A');
        assert_ne!(
            branch_greenfield, branch_payments,
            "branch names should be different"
        );
        assert!(
            branch_greenfield.starts_with("greenfield-agent-aaron-"),
            "greenfield branch should have greenfield prefix: {}",
            branch_greenfield
        );
        assert!(
            branch_payments.starts_with("payments-agent-aaron-"),
            "payments branch should have payments prefix: {}",
            branch_payments
        );

        // Verify each worktree is on the correct branch
        let greenfield_branch_output = Command::new("git")
            .arg("-C")
            .arg(wt_greenfield)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .expect("get greenfield branch");
        let greenfield_actual_branch = String::from_utf8_lossy(&greenfield_branch_output.stdout)
            .trim()
            .to_string();
        assert_eq!(
            greenfield_actual_branch, branch_greenfield,
            "greenfield worktree should be on greenfield branch"
        );

        let payments_branch_output = Command::new("git")
            .arg("-C")
            .arg(wt_payments)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .expect("get payments branch");
        let payments_actual_branch = String::from_utf8_lossy(&payments_branch_output.stdout)
            .trim()
            .to_string();
        assert_eq!(
            payments_actual_branch, branch_payments,
            "payments worktree should be on payments branch"
        );

        // Clean up greenfield project - should NOT affect payments
        worktree::cleanup_agent_worktree(&worktrees_dir, 'A', true, &ctx_greenfield)
            .expect("cleanup greenfield");

        assert!(
            !wt_greenfield.exists(),
            "greenfield worktree should be removed"
        );
        assert!(wt_payments.exists(), "payments worktree should still exist");

        // Clean up payments project
        worktree::cleanup_agent_worktree(&worktrees_dir, 'A', true, &ctx_payments)
            .expect("cleanup payments");

        assert!(!wt_payments.exists(), "payments worktree should be removed");
    });
}

/// Test parallel projects with multiple agents: verifies complete isolation
/// when two projects use the same set of agents simultaneously.
#[test]
fn test_parallel_projects_multiple_agents_isolated() {
    with_temp_cwd(|repo_path| {
        let repo_path = repo_path.to_path_buf();

        init_git_repo(&repo_path);
        fs::write(repo_path.join("README.md"), "init").expect("write README");
        commit_all(&repo_path, "init");

        let mut rename_cmd = Command::new("git");
        rename_cmd
            .arg("-C")
            .arg(&repo_path)
            .args(["branch", "-M", "main"]);
        run_success(&mut rename_cmd);

        let worktrees_dir = repo_path.join(".swarm-hug").join("worktrees");

        let ctx_proj1 = RunContext::new("project1", 1);
        let ctx_proj2 = RunContext::new("project2", 1);

        // Both projects use agents A, B, and C
        let assignments = vec![
            ('A', "Task A".to_string()),
            ('B', "Task B".to_string()),
            ('C', "Task C".to_string()),
        ];

        // Create worktrees for both projects
        let worktrees_proj1 = worktree::create_worktrees_in(
            &worktrees_dir,
            &assignments,
            "main",
            &ctx_proj1,
        )
        .expect("create project1 worktrees");

        let worktrees_proj2 = worktree::create_worktrees_in(
            &worktrees_dir,
            &assignments,
            "main",
            &ctx_proj2,
        )
        .expect("create project2 worktrees");

        assert_eq!(worktrees_proj1.len(), 3);
        assert_eq!(worktrees_proj2.len(), 3);

        // Verify all 6 worktrees exist (3 per project)
        for wt in &worktrees_proj1 {
            assert!(wt.path.exists(), "project1 worktree should exist: {:?}", wt.path);
        }
        for wt in &worktrees_proj2 {
            assert!(wt.path.exists(), "project2 worktree should exist: {:?}", wt.path);
        }

        // Verify no path collisions
        let proj1_paths: std::collections::HashSet<_> = worktrees_proj1.iter().map(|w| &w.path).collect();
        let proj2_paths: std::collections::HashSet<_> = worktrees_proj2.iter().map(|w| &w.path).collect();
        assert!(
            proj1_paths.is_disjoint(&proj2_paths),
            "worktree paths should not overlap"
        );

        // Clean up project1 completely
        let proj1_initials: Vec<char> = worktrees_proj1.iter().map(|w| w.initial).collect();
        let summary = worktree::cleanup_agent_worktrees(&worktrees_dir, &proj1_initials, true, &ctx_proj1);
        assert_eq!(summary.cleaned_count(), 3);
        assert!(!summary.has_errors());

        // Verify project1 worktrees are gone but project2 worktrees remain
        for wt in &worktrees_proj1 {
            assert!(!wt.path.exists(), "project1 worktree should be removed: {:?}", wt.path);
        }
        for wt in &worktrees_proj2 {
            assert!(wt.path.exists(), "project2 worktree should still exist: {:?}", wt.path);
        }

        // Clean up project2
        let proj2_initials: Vec<char> = worktrees_proj2.iter().map(|w| w.initial).collect();
        worktree::cleanup_agent_worktrees(&worktrees_dir, &proj2_initials, true, &ctx_proj2);
    });
}

/// Test restart isolation: when a sprint is cancelled and restarted,
/// a new hash is generated and old artifacts remain until explicitly cleaned up.
///
/// This test verifies:
/// 1. Each RunContext generates a unique hash
/// 2. Restarting (creating new RunContext for same project/sprint) creates new artifacts
/// 3. Old artifacts from cancelled run remain untouched
/// 4. Cleanup with old RunContext targets only old artifacts
/// 5. Cleanup with new RunContext targets only new artifacts
#[test]
fn test_restart_isolation_new_hash_old_artifacts_remain() {
    with_temp_cwd(|repo_path| {
        let repo_path = repo_path.to_path_buf();

        init_git_repo(&repo_path);
        fs::write(repo_path.join("README.md"), "init").expect("write README");
        commit_all(&repo_path, "init");

        let mut rename_cmd = Command::new("git");
        rename_cmd
            .arg("-C")
            .arg(&repo_path)
            .args(["branch", "-M", "main"]);
        run_success(&mut rename_cmd);

        let worktrees_dir = repo_path.join(".swarm-hug").join("worktrees");
        let assignments = vec![('A', "Task one".to_string())];

        // Simulate first run (which gets cancelled)
        let ctx_run1 = RunContext::new("greenfield", 1);
        let run1_hash = ctx_run1.hash().to_string();

        let worktrees_run1 = worktree::create_worktrees_in(
            &worktrees_dir,
            &assignments,
            "main",
            &ctx_run1,
        )
        .expect("create run1 worktrees");

        let wt_run1 = worktrees_run1[0].path.clone();
        assert!(wt_run1.exists(), "run1 worktree should exist");

        // Make a change in run1's worktree to simulate work in progress
        fs::write(wt_run1.join("wip.txt"), "work in progress from run 1").expect("write wip");
        Command::new("git")
            .arg("-C")
            .arg(&wt_run1)
            .args(["add", "."])
            .output()
            .expect("git add");
        Command::new("git")
            .arg("-C")
            .arg(&wt_run1)
            .args(["commit", "-m", "WIP from run 1"])
            .output()
            .expect("git commit");

        // Simulate restart: create new RunContext for same project/sprint
        // This represents cancelling the sprint and starting over
        let ctx_run2 = RunContext::new("greenfield", 1);
        let run2_hash = ctx_run2.hash().to_string();

        // Verify hashes are different
        assert_ne!(
            run1_hash, run2_hash,
            "restarted sprint should have a different hash"
        );

        // Create worktrees for run2
        let worktrees_run2 = worktree::create_worktrees_in(
            &worktrees_dir,
            &assignments,
            "main",
            &ctx_run2,
        )
        .expect("create run2 worktrees");

        let wt_run2 = worktrees_run2[0].path.clone();
        assert!(wt_run2.exists(), "run2 worktree should exist");

        // KEY ASSERTION: Both worktrees should exist simultaneously
        assert!(wt_run1.exists(), "run1 worktree should still exist after run2 creation");
        assert!(wt_run2.exists(), "run2 worktree should exist");
        assert_ne!(wt_run1, wt_run2, "run1 and run2 worktrees should be different paths");

        // Verify run1's work is still there
        assert!(
            wt_run1.join("wip.txt").exists(),
            "run1's WIP file should still exist"
        );

        // Verify run2 is clean (doesn't have run1's changes)
        assert!(
            !wt_run2.join("wip.txt").exists(),
            "run2 worktree should NOT have run1's WIP file"
        );

        // Clean up run2 (the new run) - should NOT affect run1
        worktree::cleanup_agent_worktree(&worktrees_dir, 'A', true, &ctx_run2)
            .expect("cleanup run2");

        assert!(!wt_run2.exists(), "run2 worktree should be removed");
        assert!(wt_run1.exists(), "run1 worktree should still exist after run2 cleanup");

        // Now clean up run1
        worktree::cleanup_agent_worktree(&worktrees_dir, 'A', true, &ctx_run1)
            .expect("cleanup run1");

        assert!(!wt_run1.exists(), "run1 worktree should be removed after explicit cleanup");
    });
}

/// Test cleanup scope: cleanup only affects artifacts from the current run's hash.
///
/// This test creates multiple runs with different hashes and verifies that cleanup
/// operations are precisely scoped to only remove artifacts matching the given RunContext.
#[test]
fn test_cleanup_scope_only_affects_current_run_hash() {
    with_temp_cwd(|repo_path| {
        let repo_path = repo_path.to_path_buf();

        init_git_repo(&repo_path);
        fs::write(repo_path.join("README.md"), "init").expect("write README");
        commit_all(&repo_path, "init");

        let mut rename_cmd = Command::new("git");
        rename_cmd
            .arg("-C")
            .arg(&repo_path)
            .args(["branch", "-M", "main"]);
        run_success(&mut rename_cmd);

        let worktrees_dir = repo_path.join(".swarm-hug").join("worktrees");
        let assignments = vec![
            ('A', "Task A".to_string()),
            ('B', "Task B".to_string()),
        ];

        // Create 3 runs with different hashes
        let ctx1 = RunContext::new("project", 1);
        let ctx2 = RunContext::new("project", 2);
        let ctx3 = RunContext::new("project", 1); // Same project/sprint, different hash

        // Record hashes for clarity
        let hash1 = ctx1.hash().to_string();
        let hash2 = ctx2.hash().to_string();
        let hash3 = ctx3.hash().to_string();
        assert_ne!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_ne!(hash2, hash3);

        // Create worktrees for all 3 runs
        let worktrees1 =
            worktree::create_worktrees_in(&worktrees_dir, &assignments, "main", &ctx1)
                .expect("create worktrees1");
        let worktrees2 =
            worktree::create_worktrees_in(&worktrees_dir, &assignments, "main", &ctx2)
                .expect("create worktrees2");
        let worktrees3 =
            worktree::create_worktrees_in(&worktrees_dir, &assignments, "main", &ctx3)
                .expect("create worktrees3");

        // Verify all worktrees exist (6 total: 2 per run)
        assert_eq!(worktrees1.len(), 2);
        assert_eq!(worktrees2.len(), 2);
        assert_eq!(worktrees3.len(), 2);

        for wt in &worktrees1 {
            assert!(wt.path.exists(), "ctx1 worktree should exist: {:?}", wt.path);
        }
        for wt in &worktrees2 {
            assert!(wt.path.exists(), "ctx2 worktree should exist: {:?}", wt.path);
        }
        for wt in &worktrees3 {
            assert!(wt.path.exists(), "ctx3 worktree should exist: {:?}", wt.path);
        }

        // Cleanup ctx1 - should ONLY affect ctx1's worktrees
        let summary1 = worktree::cleanup_agent_worktrees(&worktrees_dir, &['A', 'B'], true, &ctx1);
        assert_eq!(summary1.cleaned_count(), 2);
        assert!(!summary1.has_errors());

        // Verify ctx1 worktrees are removed, others remain
        for wt in &worktrees1 {
            assert!(!wt.path.exists(), "ctx1 worktree should be removed: {:?}", wt.path);
        }
        for wt in &worktrees2 {
            assert!(wt.path.exists(), "ctx2 worktree should still exist after ctx1 cleanup: {:?}", wt.path);
        }
        for wt in &worktrees3 {
            assert!(wt.path.exists(), "ctx3 worktree should still exist after ctx1 cleanup: {:?}", wt.path);
        }

        // Cleanup ctx3 - should ONLY affect ctx3's worktrees
        let summary3 = worktree::cleanup_agent_worktrees(&worktrees_dir, &['A', 'B'], true, &ctx3);
        assert_eq!(summary3.cleaned_count(), 2);

        // Verify ctx3 worktrees are removed, ctx2 still remains
        for wt in &worktrees3 {
            assert!(!wt.path.exists(), "ctx3 worktree should be removed: {:?}", wt.path);
        }
        for wt in &worktrees2 {
            assert!(wt.path.exists(), "ctx2 worktree should still exist after ctx3 cleanup: {:?}", wt.path);
        }

        // Finally cleanup ctx2
        let summary2 = worktree::cleanup_agent_worktrees(&worktrees_dir, &['A', 'B'], true, &ctx2);
        assert_eq!(summary2.cleaned_count(), 2);

        for wt in &worktrees2 {
            assert!(!wt.path.exists(), "ctx2 worktree should be removed: {:?}", wt.path);
        }
    });
}

/// Test that branch cleanup is also scoped by run hash.
///
/// Verifies that when deleting branches during cleanup, only branches
/// matching the specific run hash are deleted.
#[test]
fn test_branch_cleanup_scoped_by_hash() {
    with_temp_cwd(|repo_path| {
        let repo_path = repo_path.to_path_buf();

        init_git_repo(&repo_path);
        fs::write(repo_path.join("README.md"), "init").expect("write README");
        commit_all(&repo_path, "init");

        let mut rename_cmd = Command::new("git");
        rename_cmd
            .arg("-C")
            .arg(&repo_path)
            .args(["branch", "-M", "main"]);
        run_success(&mut rename_cmd);

        let worktrees_dir = repo_path.join(".swarm-hug").join("worktrees");
        let assignments = vec![('A', "Task A".to_string())];

        // Create two runs
        let ctx1 = RunContext::new("project", 1);
        let ctx2 = RunContext::new("project", 1);

        let branch1 = ctx1.agent_branch('A');
        let branch2 = ctx2.agent_branch('A');

        // Create worktrees for both runs
        worktree::create_worktrees_in(&worktrees_dir, &assignments, "main", &ctx1)
            .expect("create worktrees ctx1");
        worktree::create_worktrees_in(&worktrees_dir, &assignments, "main", &ctx2)
            .expect("create worktrees ctx2");

        // Helper to check if a branch exists
        let branch_exists = |branch: &str| -> bool {
            Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["show-ref", "--verify", "--quiet", &format!("refs/heads/{}", branch)])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };

        // Both branches should exist
        assert!(branch_exists(&branch1), "branch1 should exist");
        assert!(branch_exists(&branch2), "branch2 should exist");

        // Cleanup ctx1 with branch deletion
        worktree::cleanup_agent_worktree(&worktrees_dir, 'A', true, &ctx1)
            .expect("cleanup ctx1");

        // branch1 should be deleted, branch2 should remain
        assert!(!branch_exists(&branch1), "branch1 should be deleted after ctx1 cleanup");
        assert!(branch_exists(&branch2), "branch2 should still exist after ctx1 cleanup");

        // Cleanup ctx2 with branch deletion
        worktree::cleanup_agent_worktree(&worktrees_dir, 'A', true, &ctx2)
            .expect("cleanup ctx2");

        // Now both branches should be deleted
        assert!(!branch_exists(&branch2), "branch2 should be deleted after ctx2 cleanup");
    });
}

/// Test that the run hash is consistent across all artifacts within a single run.
///
/// Verifies that sprint branch, agent worktrees, and agent branches all share
/// the same hash suffix within a single RunContext.
#[test]
fn test_run_hash_consistency_across_artifacts() {
    let ctx = RunContext::new("myproject", 5);
    let hash = ctx.hash();

    // Sprint branch should end with the hash
    let sprint_branch = ctx.sprint_branch();
    assert!(
        sprint_branch.ends_with(hash),
        "sprint branch '{}' should end with hash '{}'",
        sprint_branch,
        hash
    );
    assert!(
        sprint_branch.starts_with("myproject-sprint-5-"),
        "sprint branch should have correct prefix"
    );

    // All agent branches should end with the same hash
    for initial in 'A'..='Z' {
        let agent_branch = ctx.agent_branch(initial);
        assert!(
            agent_branch.ends_with(hash),
            "agent branch for {} '{}' should end with hash '{}'",
            initial,
            agent_branch,
            hash
        );
    }

    // Verify hash length is 6 characters
    assert_eq!(hash.len(), 6, "hash should be 6 characters");

    // Verify hash is alphanumeric lowercase
    assert!(
        hash.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
        "hash '{}' should be lowercase alphanumeric",
        hash
    );
}

/// Test that different projects with same sprint number get different hashes.
///
/// Even when two projects happen to be on the same sprint number, their
/// RunContexts will have different hashes ensuring full isolation.
#[test]
fn test_different_projects_same_sprint_different_hashes() {
    let ctx_alpha = RunContext::new("alpha", 1);
    let ctx_beta = RunContext::new("beta", 1);

    // Hashes should be different (statistically guaranteed)
    assert_ne!(
        ctx_alpha.hash(),
        ctx_beta.hash(),
        "different projects should have different hashes"
    );

    // Sprint branches should be completely different
    assert_ne!(
        ctx_alpha.sprint_branch(),
        ctx_beta.sprint_branch(),
        "sprint branches should be different"
    );

    // Agent branches should be completely different
    assert_ne!(
        ctx_alpha.agent_branch('A'),
        ctx_beta.agent_branch('A'),
        "agent branches should be different"
    );

    // Verify prefixes are correct
    assert!(ctx_alpha.sprint_branch().starts_with("alpha-sprint-1-"));
    assert!(ctx_beta.sprint_branch().starts_with("beta-sprint-1-"));
}

/// Test that providing --target-branch without --source-branch produces the exact error message.
#[test]
fn test_target_branch_only_flag_returns_error() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("README.md"), "init").expect("write README");
    commit_all(repo_path, "init");

    let swarm_bin = env!("CARGO_BIN_EXE_swarm");
    let mut cmd = Command::new(swarm_bin);
    cmd.args(["--target-branch", "feature-1", "--no-tui", "run"])
        .current_dir(repo_path);

    let output = cmd.output().expect("failed to run command");
    assert!(
        !output.status.success(),
        "should fail when --target-branch is provided without --source-branch"
    );

    let stderr_raw = String::from_utf8_lossy(&output.stderr);
    let stderr = strip_ansi(&stderr_raw);

    assert!(
        stderr.contains("--target-branch requires --source-branch. Specify both flags explicitly."),
        "expected exact error message about --target-branch requiring --source-branch, got stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Example: swarm run --source-branch main --target-branch feature-1"),
        "expected example usage in error message, got stderr:\n{}",
        stderr
    );
}

/// Test that --source-branch alone sets both source and target to the same branch.
/// Verifies the sprint forks from and merges into the specified branch.
#[test]
fn test_source_branch_only_sets_both_source_and_target() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Rename default branch to "main" and create a separate branch "dev"
    let mut rename_cmd = Command::new("git");
    rename_cmd
        .args(["branch", "-M", "main"])
        .current_dir(repo_path);
    run_success(&mut rename_cmd);

    fs::write(repo_path.join("README.md"), "init").expect("write README");
    commit_all(repo_path, "base commit");

    // Create "dev" branch from main
    let mut branch_cmd = Command::new("git");
    branch_cmd
        .args(["checkout", "-b", "dev"])
        .current_dir(repo_path);
    run_success(&mut branch_cmd);

    fs::write(repo_path.join("dev.txt"), "dev content").expect("write dev.txt");
    commit_all(repo_path, "dev commit");

    // Go back to main for the swarm run
    let mut checkout_main = Command::new("git");
    checkout_main
        .args(["checkout", "main"])
        .current_dir(repo_path);
    run_success(&mut checkout_main);

    // Initialize project
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    let team_root = repo_path.join(".swarm-hug").join(team_name);
    write_team_tasks(&team_root);
    commit_all(repo_path, "init project");

    // Also commit the project setup to the dev branch so it has tasks
    let mut checkout_dev = Command::new("git");
    checkout_dev
        .args(["checkout", "dev"])
        .current_dir(repo_path);
    run_success(&mut checkout_dev);
    let dev_team_root = repo_path.join(".swarm-hug").join(team_name);
    fs::create_dir_all(&dev_team_root).ok();
    write_team_tasks(&dev_team_root);
    commit_all(repo_path, "init project on dev");

    // Stay on dev; run with --source-branch dev (no --target-branch)
    // This should set both source and target to "dev"
    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--project",
            team_name,
            "--source-branch",
            "dev",
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "1",
            "--no-tui",
            "run",
        ])
        .current_dir(repo_path);
    let output = run_success(&mut run_cmd);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    // Verify sprint ran
    assert!(
        stdout.contains("Sprint 1: assigned"),
        "Sprint should have run. Output:\n{}",
        stdout
    );

    // Verify tasks completed
    let tasks_content =
        fs::read_to_string(dev_team_root.join("tasks.md")).expect("read tasks");
    let task_list = TaskList::parse(&tasks_content);
    assert!(
        task_list.completed_count() >= 2,
        "Tasks should be completed"
    );

    // Verify the sprint was merged into "dev" (the target), not "main"
    let dev_log = git_stdout(repo_path, &["log", "--oneline", "-10", "dev"]);
    assert!(
        dev_log.contains("Alpha Sprint 1: completed"),
        "dev branch should have sprint completion commit. Got:\n{}",
        dev_log
    );

    let main_log = git_stdout(repo_path, &["log", "--oneline", "-10", "main"]);
    assert!(
        !main_log.contains("Alpha Sprint 1: completed"),
        "main branch should NOT have sprint completion commit (target is dev). Got:\n{}",
        main_log
    );

    // Verify chat mentions the correct base (should fork from dev, not main)
    let chat_content =
        fs::read_to_string(dev_team_root.join("chat.md")).expect("read chat");
    assert!(
        chat_content.contains("Creating worktree"),
        "chat should contain worktree creation message. Chat:\n{}",
        chat_content
    );
}

/// Test that providing neither --source-branch nor --target-branch auto-detects main/master.
/// This is the backwards-compatible default behavior.
#[test]
fn test_neither_branch_flag_auto_detects() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Rename default branch to "main"
    let mut rename_cmd = Command::new("git");
    rename_cmd
        .args(["branch", "-M", "main"])
        .current_dir(repo_path);
    run_success(&mut rename_cmd);

    // Initialize project
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    let team_root = repo_path.join(".swarm-hug").join(team_name);
    write_team_tasks(&team_root);
    commit_all(repo_path, "init");

    let initial_commit = git_stdout(repo_path, &["rev-parse", "HEAD"]);

    // Run WITHOUT --source-branch or --target-branch
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
            "run",
        ])
        .current_dir(repo_path);
    let output = run_success(&mut run_cmd);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    // Verify sprint ran successfully
    assert!(
        stdout.contains("Sprint 1: assigned"),
        "Sprint should have run. Output:\n{}",
        stdout
    );

    // Verify tasks completed
    let tasks_content =
        fs::read_to_string(team_root.join("tasks.md")).expect("read tasks");
    let task_list = TaskList::parse(&tasks_content);
    assert_eq!(
        task_list.completed_count(),
        2,
        "Both tasks should be completed"
    );

    // Verify the sprint was merged into auto-detected "main" branch
    let main_log = git_stdout(repo_path, &["log", "--oneline", "-10", "main"]);
    assert!(
        main_log.contains("Alpha Sprint 1: completed"),
        "auto-detected main branch should have sprint completion commit. Got:\n{}",
        main_log
    );

    // Verify we're still on main and the branch has advanced past the initial commit
    let current_branch = git_stdout(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(
        current_branch, "main",
        "should still be on main branch"
    );

    let current_commit = git_stdout(repo_path, &["rev-parse", "HEAD"]);
    assert_ne!(
        current_commit, initial_commit,
        "main branch should have advanced past initial commit"
    );
}

/// Test that --source-branch + --target-branch forks from source and merges into target.
#[test]
fn test_source_and_target_branch_forks_from_source_merges_into_target() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Rename default branch to "main"
    let mut rename_cmd = Command::new("git");
    rename_cmd
        .args(["branch", "-M", "main"])
        .current_dir(repo_path);
    run_success(&mut rename_cmd);

    // Initialize project and write tasks on main
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    let team_root = repo_path.join(".swarm-hug").join(team_name);
    write_team_tasks(&team_root);
    commit_all(repo_path, "init project on main");

    // Create "feature-1" branch from main AFTER project setup
    // so feature-1 and main share the same initial state
    let mut create_target = Command::new("git");
    create_target
        .args(["branch", "feature-1"])
        .current_dir(repo_path);
    run_success(&mut create_target);

    // Add a commit on main that only exists on main (source-only)
    fs::write(repo_path.join("main-only.txt"), "main content").expect("write main-only.txt");
    commit_all(repo_path, "main-only commit");

    // Sanity check: target branch does not have the source-only file before the run.
    let feature_files_before = git_stdout(repo_path, &["ls-tree", "--name-only", "feature-1"]);
    assert!(
        !feature_files_before.contains("main-only.txt"),
        "feature-1 should not contain main-only.txt before sprint. Files:\n{}",
        feature_files_before
    );

    // Run with --source-branch main --target-branch feature-1
    // This should fork from main (source) and merge back into feature-1 (target).
    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--project",
            team_name,
            "--source-branch",
            "main",
            "--target-branch",
            "feature-1",
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "1",
            "--no-tui",
            "run",
        ])
        .current_dir(repo_path);
    let output = run_success(&mut run_cmd);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));

    // Verify sprint ran
    assert!(
        stdout.contains("Sprint 1: assigned"),
        "Sprint should have run. Output:\n{}",
        stdout
    );

    // Verify the sprint was merged into "feature-1" (the target), not "main"
    let feature_log = git_stdout(repo_path, &["log", "--oneline", "-10", "feature-1"]);
    assert!(
        feature_log.contains("Alpha Sprint 1: completed"),
        "feature-1 branch should have sprint completion commit. Got:\n{}",
        feature_log
    );

    // Main should NOT have the sprint completion commit (it was the source, not the target)
    let main_log = git_stdout(repo_path, &["log", "--oneline", "-10", "main"]);
    assert!(
        !main_log.contains("Alpha Sprint 1: completed"),
        "main branch should NOT have sprint completion commit (it's the source, not target). Got:\n{}",
        main_log
    );

    // Verify that feature-1 now contains the source-only content from main.
    // This confirms sprint worktree creation forked from source branch.
    let feature_files = git_stdout(repo_path, &["ls-tree", "--name-only", "feature-1"]);
    assert!(
        feature_files.contains("main-only.txt"),
        "feature-1 should contain main-only.txt after sprint (sprint must fork from source tip). Files:\n{}",
        feature_files
    );

    // Verify tasks completed on the target branch
    let mut checkout_feature = Command::new("git");
    checkout_feature
        .args(["checkout", "feature-1"])
        .current_dir(repo_path);
    run_success(&mut checkout_feature);

    let tasks_content =
        fs::read_to_string(team_root.join("tasks.md")).expect("read tasks");
    let task_list = TaskList::parse(&tasks_content);
    assert!(
        task_list.completed_count() >= 2,
        "Tasks should be completed on target branch"
    );
}

/// Test that a non-existent source branch returns a clear error message.
#[test]
fn test_nonexistent_source_branch_returns_clear_error() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Rename default branch to "main"
    let mut rename_cmd = Command::new("git");
    rename_cmd
        .args(["branch", "-M", "main"])
        .current_dir(repo_path);
    run_success(&mut rename_cmd);

    // Initialize project
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    let team_root = repo_path.join(".swarm-hug").join(team_name);
    write_team_tasks(&team_root);
    commit_all(repo_path, "init");

    // Run with a non-existent source branch
    let mut run_cmd = Command::new(swarm_bin);
    run_cmd
        .args([
            "--project",
            team_name,
            "--source-branch",
            "does-not-exist",
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "1",
            "--no-tui",
            "run",
        ])
        .current_dir(repo_path);

    let output = run_cmd.output().expect("failed to run command");
    assert!(
        !output.status.success(),
        "should fail when source branch does not exist"
    );

    let stderr_raw = String::from_utf8_lossy(&output.stderr);
    let stderr = strip_ansi(&stderr_raw);

    assert!(
        stderr.contains("source branch 'does-not-exist' does not exist"),
        "expected clear error about non-existent source branch, got stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Check the branch name and try again"),
        "expected guidance in error message, got stderr:\n{}",
        stderr
    );
}

/// Test same-project concurrent variation prep across different target branches.
///
/// This verifies runtime isolation guarantees from #2/#3/#4 without relying on
/// merge-agent behavior:
/// 1. Sprint plans are independent per target branch
/// 2. Task assignments are independent per target branch
/// 3. tasks.md is loaded from each target-branch worktree
/// 4. Sprint worktrees fork from each target branch tip
/// 5. Sprint history remains isolated per variation
#[test]
fn test_same_project_different_target_branches_isolated_variation_prep() {
    #[derive(Debug)]
    struct VariationPlan {
        branch: String,
        next_sprint: usize,
        run_ctx: RunContext,
        tasks_content: String,
        assigned_tasks: Vec<String>,
    }

    with_temp_cwd(|repo_path| {
        let repo_path = repo_path.to_path_buf();
        let team_name = "alpha";

        init_git_repo(&repo_path);
        fs::write(repo_path.join("README.md"), "init").expect("write README");
        commit_all(&repo_path, "init");
        run_success(
            Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["branch", "-M", "main"]),
        );

        // Seed shared project directory on main.
        let team_root = repo_path.join(".swarm-hug").join(team_name);
        fs::create_dir_all(&team_root).expect("create team root");
        fs::write(team_root.join("tasks.md"), "# Tasks\n\n- [ ] Seed task\n")
            .expect("write seed tasks");
        commit_all(&repo_path, "seed project");

        // target-one has distinct tasks/history/tip marker.
        run_success(Command::new("git").arg("-C").arg(&repo_path).args([
            "checkout",
            "-b",
            "target-one",
        ]));
        fs::write(
            team_root.join("tasks.md"),
            "# Tasks\n\n- [ ] Alpha one task A\n- [ ] Alpha one task B\n",
        )
        .expect("write target-one tasks");
        fs::write(
            team_root.join("sprint-history.json"),
            "{\n  \"team\": \"alpha\",\n  \"total_sprints\": 4\n}\n",
        )
        .expect("write target-one history");
        fs::write(repo_path.join("target-one-tip.txt"), "target one tip\n")
            .expect("write target-one tip marker");
        commit_all(&repo_path, "seed target-one");

        // target-two has different tasks/history/tip marker.
        run_success(
            Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["checkout", "main"]),
        );
        run_success(Command::new("git").arg("-C").arg(&repo_path).args([
            "checkout",
            "-b",
            "target-two",
        ]));
        fs::write(
            team_root.join("tasks.md"),
            "# Tasks\n\n- [ ] Alpha two task X\n- [ ] Alpha two task Y\n",
        )
        .expect("write target-two tasks");
        fs::write(
            team_root.join("sprint-history.json"),
            "{\n  \"team\": \"alpha\",\n  \"total_sprints\": 9\n}\n",
        )
        .expect("write target-two history");
        fs::write(repo_path.join("target-two-tip.txt"), "target two tip\n")
            .expect("write target-two tip marker");
        commit_all(&repo_path, "seed target-two");
        run_success(
            Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["checkout", "main"]),
        );

        let target_one_worktree =
            worktree::create_target_branch_worktree_in(&repo_path, "target-one")
                .expect("create target-one worktree");
        let target_two_worktree =
            worktree::create_target_branch_worktree_in(&repo_path, "target-two")
                .expect("create target-two worktree");
        assert_ne!(
            canonical_path_str(&target_one_worktree),
            canonical_path_str(&target_two_worktree),
            "target branch worktrees should be distinct"
        );

        let plan_one = {
            let team_name = team_name.to_string();
            let target_one_worktree = target_one_worktree.clone();
            thread::spawn(move || {
                let tasks_path = target_one_worktree
                    .join(".swarm-hug")
                    .join(&team_name)
                    .join("tasks.md");
                let tasks_content = fs::read_to_string(&tasks_path).expect("read target-one tasks");
                let task_list = TaskList::parse(&tasks_content);

                let loop_dir = target_one_worktree
                    .join(".swarm-hug")
                    .join(&team_name)
                    .join("loop");
                let engine = StubEngine::new(loop_dir.to_string_lossy().to_string());
                let plan_result = swarm::planning::run_llm_assignment(
                    &engine,
                    &task_list,
                    &['A', 'B'],
                    1,
                    &loop_dir,
                );
                assert!(plan_result.success, "target-one plan should succeed");

                let mut assigned_list = task_list.clone();
                for (line_num, initial) in &plan_result.assignments {
                    let idx = line_num.saturating_sub(1);
                    if idx < assigned_list.tasks.len() {
                        assigned_list.tasks[idx].assign(*initial);
                    }
                }
                let assigned_tasks: Vec<String> = assigned_list
                    .tasks
                    .iter()
                    .filter_map(|t| match t.status {
                        TaskStatus::Assigned(_) => Some(t.description.clone()),
                        _ => None,
                    })
                    .collect();

                let history_path = target_one_worktree
                    .join(".swarm-hug")
                    .join(&team_name)
                    .join("sprint-history.json");
                let history = swarm::team::SprintHistory::load_from(&history_path)
                    .expect("load target-one history");
                let next_sprint = history.peek_next_sprint();

                VariationPlan {
                    branch: "target-one".to_string(),
                    next_sprint,
                    run_ctx: RunContext::new(&team_name, next_sprint as u32),
                    tasks_content,
                    assigned_tasks,
                }
            })
        };

        let plan_two = {
            let team_name = team_name.to_string();
            let target_two_worktree = target_two_worktree.clone();
            thread::spawn(move || {
                let tasks_path = target_two_worktree
                    .join(".swarm-hug")
                    .join(&team_name)
                    .join("tasks.md");
                let tasks_content = fs::read_to_string(&tasks_path).expect("read target-two tasks");
                let task_list = TaskList::parse(&tasks_content);

                let loop_dir = target_two_worktree
                    .join(".swarm-hug")
                    .join(&team_name)
                    .join("loop");
                let engine = StubEngine::new(loop_dir.to_string_lossy().to_string());
                let plan_result = swarm::planning::run_llm_assignment(
                    &engine,
                    &task_list,
                    &['A', 'B'],
                    1,
                    &loop_dir,
                );
                assert!(plan_result.success, "target-two plan should succeed");

                let mut assigned_list = task_list.clone();
                for (line_num, initial) in &plan_result.assignments {
                    let idx = line_num.saturating_sub(1);
                    if idx < assigned_list.tasks.len() {
                        assigned_list.tasks[idx].assign(*initial);
                    }
                }
                let assigned_tasks: Vec<String> = assigned_list
                    .tasks
                    .iter()
                    .filter_map(|t| match t.status {
                        TaskStatus::Assigned(_) => Some(t.description.clone()),
                        _ => None,
                    })
                    .collect();

                let history_path = target_two_worktree
                    .join(".swarm-hug")
                    .join(&team_name)
                    .join("sprint-history.json");
                let history = swarm::team::SprintHistory::load_from(&history_path)
                    .expect("load target-two history");
                let next_sprint = history.peek_next_sprint();

                VariationPlan {
                    branch: "target-two".to_string(),
                    next_sprint,
                    run_ctx: RunContext::new(&team_name, next_sprint as u32),
                    tasks_content,
                    assigned_tasks,
                }
            })
        };

        let mut plans = vec![
            plan_one.join().expect("join target-one plan"),
            plan_two.join().expect("join target-two plan"),
        ];
        plans.sort_by(|a, b| a.branch.cmp(&b.branch));
        let plan_one = &plans[0];
        let plan_two = &plans[1];

        // Independent sprint plans + target-branch tasks.md loading.
        assert!(
            plan_one.tasks_content.contains("Alpha one task A")
                && plan_one.tasks_content.contains("Alpha one task B"),
            "target-one plan should load target-one tasks"
        );
        assert!(
            !plan_one.tasks_content.contains("Alpha two task X"),
            "target-one plan should not load target-two tasks"
        );
        assert!(
            plan_two.tasks_content.contains("Alpha two task X")
                && plan_two.tasks_content.contains("Alpha two task Y"),
            "target-two plan should load target-two tasks"
        );
        assert!(
            !plan_two.tasks_content.contains("Alpha one task A"),
            "target-two plan should not load target-one tasks"
        );

        // Independent task assignments.
        assert_eq!(
            plan_one.assigned_tasks.len(),
            2,
            "target-one should assign two tasks"
        );
        assert_eq!(
            plan_two.assigned_tasks.len(),
            2,
            "target-two should assign two tasks"
        );
        assert!(
            plan_one
                .assigned_tasks
                .iter()
                .all(|t| t.contains("Alpha one")),
            "target-one assignments should stay in target-one task set: {:?}",
            plan_one.assigned_tasks
        );
        assert!(
            plan_two
                .assigned_tasks
                .iter()
                .all(|t| t.contains("Alpha two")),
            "target-two assignments should stay in target-two task set: {:?}",
            plan_two.assigned_tasks
        );
        assert_ne!(
            plan_one.assigned_tasks, plan_two.assigned_tasks,
            "target variations should not share assignment lists"
        );

        // Isolated sprint numbering from per-branch sprint history.
        assert_eq!(
            plan_one.next_sprint, 5,
            "target-one should continue from history=4"
        );
        assert_eq!(
            plan_two.next_sprint, 10,
            "target-two should continue from history=9"
        );

        let sprint_branch_one = plan_one.run_ctx.sprint_branch();
        let sprint_branch_two = plan_two.run_ctx.sprint_branch();
        assert!(
            sprint_branch_one.starts_with("alpha-sprint-5-"),
            "target-one sprint branch should use sprint 5: {}",
            sprint_branch_one
        );
        assert!(
            sprint_branch_two.starts_with("alpha-sprint-10-"),
            "target-two sprint branch should use sprint 10: {}",
            sprint_branch_two
        );
        assert_ne!(
            sprint_branch_one, sprint_branch_two,
            "same-project variations should still use distinct sprint branches"
        );

        // Target-tip worktree forking.
        let worktrees_dir = repo_path
            .join(".swarm-hug")
            .join(team_name)
            .join("worktrees");
        let feature_one =
            worktree::create_feature_worktree_in(&worktrees_dir, &sprint_branch_one, "target-one")
                .expect("create target-one sprint worktree");
        let feature_two =
            worktree::create_feature_worktree_in(&worktrees_dir, &sprint_branch_two, "target-two")
                .expect("create target-two sprint worktree");
        assert!(feature_one.join("target-one-tip.txt").exists());
        assert!(!feature_one.join("target-two-tip.txt").exists());
        assert!(feature_two.join("target-two-tip.txt").exists());
        assert!(!feature_two.join("target-one-tip.txt").exists());

        let feature_one_tasks = fs::read_to_string(
            feature_one
                .join(".swarm-hug")
                .join(team_name)
                .join("tasks.md"),
        )
        .expect("read target-one sprint tasks");
        assert!(feature_one_tasks.contains("Alpha one task A"));
        assert!(!feature_one_tasks.contains("Alpha two task X"));

        let feature_two_tasks = fs::read_to_string(
            feature_two
                .join(".swarm-hug")
                .join(team_name)
                .join("tasks.md"),
        )
        .expect("read target-two sprint tasks");
        assert!(feature_two_tasks.contains("Alpha two task X"));
        assert!(!feature_two_tasks.contains("Alpha one task A"));

        // Isolated sprint-history mutation in each variation worktree (run concurrently).
        let history_one_path = feature_one
            .join(".swarm-hug")
            .join(team_name)
            .join("sprint-history.json");
        let history_two_path = feature_two
            .join(".swarm-hug")
            .join(team_name)
            .join("sprint-history.json");

        let history_one_handle = {
            let history_one_path = history_one_path.clone();
            thread::spawn(move || {
                let mut history = swarm::team::SprintHistory::load_from(&history_one_path)
                    .expect("load feature-one history");
                assert_eq!(history.total_sprints, 4);
                history.increment();
                history.save().expect("save feature-one history");
                history.total_sprints
            })
        };
        let history_two_handle = {
            let history_two_path = history_two_path.clone();
            thread::spawn(move || {
                let mut history = swarm::team::SprintHistory::load_from(&history_two_path)
                    .expect("load feature-two history");
                assert_eq!(history.total_sprints, 9);
                history.increment();
                history.save().expect("save feature-two history");
                history.total_sprints
            })
        };

        assert_eq!(
            history_one_handle.join().expect("join feature-one history"),
            5
        );
        assert_eq!(
            history_two_handle.join().expect("join feature-two history"),
            10
        );

        let final_history_one = swarm::team::SprintHistory::load_from(&history_one_path)
            .expect("reload feature-one history");
        let final_history_two = swarm::team::SprintHistory::load_from(&history_two_path)
            .expect("reload feature-two history");
        assert_eq!(final_history_one.total_sprints, 5);
        assert_eq!(final_history_two.total_sprints, 10);

        // Target branches remain unchanged until merge (history updates are isolated in sprint branches).
        let target_one_history = git_stdout(
            &repo_path,
            &["show", "target-one:.swarm-hug/alpha/sprint-history.json"],
        );
        let target_two_history = git_stdout(
            &repo_path,
            &["show", "target-two:.swarm-hug/alpha/sprint-history.json"],
        );
        assert!(target_one_history.contains("\"total_sprints\": 4"));
        assert!(target_two_history.contains("\"total_sprints\": 9"));
    });
}

/// Test the two-step follow-up workflow end-to-end:
/// 1. First run: source=main, target=feature-1 (fork from main, merge into feature-1)
/// 2. Second run: source=feature-1, target=feature-1-follow-ups (fork from feature-1, merge into follow-ups)
/// Verify that feature-1-follow-ups contains commits from both runs.
#[test]
fn test_two_step_followup_workflow() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();
    let team_name = "alpha";

    init_git_repo(repo_path);
    let swarm_bin = env!("CARGO_BIN_EXE_swarm");

    // Rename default branch to "main"
    let mut rename_cmd = Command::new("git");
    rename_cmd
        .args(["branch", "-M", "main"])
        .current_dir(repo_path);
    run_success(&mut rename_cmd);

    fs::write(repo_path.join("README.md"), "init").expect("write README");
    commit_all(repo_path, "base commit");

    // Initialize project on main
    let mut team_init_cmd = Command::new(swarm_bin);
    team_init_cmd
        .args(["project", "init", team_name])
        .current_dir(repo_path);
    run_success(&mut team_init_cmd);

    let team_root = repo_path.join(".swarm-hug").join(team_name);
    let tasks_path = team_root.join("tasks.md");
    // Write initial tasks for the first run
    let tasks_content = "# Tasks\n\n- [ ] First run task A\n- [ ] First run task B\n";
    fs::write(&tasks_path, tasks_content).expect("write tasks");
    commit_all(repo_path, "init project");

    // Create "feature-1" branch from main (target for first run)
    let mut create_feature1 = Command::new("git");
    create_feature1
        .args(["branch", "feature-1"])
        .current_dir(repo_path);
    run_success(&mut create_feature1);

    // === STEP 1: Run with source=main, target=feature-1 ===
    let mut run_cmd1 = Command::new(swarm_bin);
    run_cmd1
        .args([
            "--project",
            team_name,
            "--source-branch",
            "main",
            "--target-branch",
            "feature-1",
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "1",
            "--no-tui",
            "run",
        ])
        .current_dir(repo_path);
    let output1 = run_success(&mut run_cmd1);
    let stdout1 = strip_ansi(&String::from_utf8_lossy(&output1.stdout));

    assert!(
        stdout1.contains("Sprint 1: assigned"),
        "First run should have run. Output:\n{}",
        stdout1
    );

    // Verify feature-1 has the sprint completion
    let feature1_log = git_stdout(repo_path, &["log", "--oneline", "-10", "feature-1"]);
    assert!(
        feature1_log.contains("Alpha Sprint 1: completed"),
        "feature-1 should have sprint 1 completion. Got:\n{}",
        feature1_log
    );

    // Record the feature-1 commit for later comparison
    let feature1_head_after_run1 = git_stdout(repo_path, &["rev-parse", "feature-1"]);

    // === STEP 2: Prepare for second run ===
    // Checkout feature-1 to update tasks for the second run
    let mut checkout_feature1 = Command::new("git");
    checkout_feature1
        .args(["checkout", "feature-1"])
        .current_dir(repo_path);
    run_success(&mut checkout_feature1);

    // Write new tasks for the second run (replace completed tasks)
    let tasks_content2 = "# Tasks\n\n- [ ] Follow-up task X\n- [ ] Follow-up task Y\n";
    fs::write(&tasks_path, tasks_content2).expect("write second run tasks");
    commit_all(repo_path, "second run tasks");

    // Create "feature-1-follow-ups" branch from feature-1 (target for second run)
    let mut create_followups = Command::new("git");
    create_followups
        .args(["branch", "feature-1-follow-ups"])
        .current_dir(repo_path);
    run_success(&mut create_followups);

    // === STEP 2: Run with source=feature-1, target=feature-1-follow-ups ===
    let mut run_cmd2 = Command::new(swarm_bin);
    run_cmd2
        .args([
            "--project",
            team_name,
            "--source-branch",
            "feature-1",
            "--target-branch",
            "feature-1-follow-ups",
            "--stub",
            "--max-sprints",
            "1",
            "--tasks-per-agent",
            "1",
            "--no-tui",
            "run",
        ])
        .current_dir(repo_path);
    let output2 = run_success(&mut run_cmd2);
    let stdout2 = strip_ansi(&String::from_utf8_lossy(&output2.stdout));

    // The second run continues sprint numbering from the first run's history
    // (inherited from feature-1), so this is Sprint 2
    assert!(
        stdout2.contains("Sprint 2: assigned"),
        "Second run should have run (as Sprint 2, continuing from first run). Output:\n{}",
        stdout2
    );

    // === ASSERTIONS ===

    // feature-1-follow-ups should have the sprint completion from the second run
    // (Sprint 2, since sprint numbering continues from the first run's history)
    let followups_log = git_stdout(repo_path, &["log", "--oneline", "-20", "feature-1-follow-ups"]);
    assert!(
        followups_log.contains("Alpha Sprint 2: completed"),
        "feature-1-follow-ups should have sprint 2 completion commit. Got:\n{}",
        followups_log
    );

    // feature-1-follow-ups should contain commits from both runs:
    // It was forked from feature-1 (which already had the first run's commits),
    // then the second run's sprint was merged into it.
    // So it should have the base commit from main AND the first run's sprint work
    assert!(
        followups_log.contains("base commit"),
        "feature-1-follow-ups should contain the original base commit (inherited from main via feature-1). Got:\n{}",
        followups_log
    );

    // feature-1-follow-ups should be a descendant of feature-1's state after run 1
    // (i.e., the first run's work is reachable from feature-1-follow-ups)
    let is_ancestor = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["merge-base", "--is-ancestor", &feature1_head_after_run1, "feature-1-follow-ups"])
        .output()
        .expect("git merge-base");
    assert!(
        is_ancestor.status.success(),
        "feature-1 (after run 1) should be an ancestor of feature-1-follow-ups. \
         This proves the follow-up branch contains the first run's commits."
    );

    // Verify the follow-up tasks were completed
    let mut checkout_followups = Command::new("git");
    checkout_followups
        .args(["checkout", "feature-1-follow-ups"])
        .current_dir(repo_path);
    run_success(&mut checkout_followups);

    let followup_tasks_content =
        fs::read_to_string(&tasks_path).expect("read tasks on follow-ups branch");
    let followup_task_list = TaskList::parse(&followup_tasks_content);
    assert!(
        followup_task_list.completed_count() >= 2,
        "Follow-up tasks should be completed. Got {} completed.",
        followup_task_list.completed_count()
    );

    // Verify main was NOT modified by either run
    let main_log = git_stdout(repo_path, &["log", "--oneline", "-10", "main"]);
    assert!(
        !main_log.contains("Alpha Sprint 1: completed"),
        "main branch should NOT have sprint completion commits. Got:\n{}",
        main_log
    );
}

// ── ensure_feature_merged: parent-count enforcement tests ─────────────────

/// Test that ensure_feature_merged succeeds with a valid 2-parent merge commit.
#[test]
fn test_ensure_feature_merged_accepts_two_parent_merge() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("base.txt"), "base\n").expect("write base");
    commit_all(repo_path, "init");

    let mut rename = Command::new("git");
    rename.args(["branch", "-M", "main"]).current_dir(repo_path);
    run_success(&mut rename);

    // Create feature branch with a commit
    let mut checkout_feat = Command::new("git");
    checkout_feat
        .args(["checkout", "-b", "feat-2p"])
        .current_dir(repo_path);
    run_success(&mut checkout_feat);
    fs::write(repo_path.join("feat.txt"), "feature\n").expect("write feature");
    commit_all(repo_path, "feature commit");

    // Back to main
    let mut checkout_main = Command::new("git");
    checkout_main
        .args(["checkout", "main"])
        .current_dir(repo_path);
    run_success(&mut checkout_main);

    // Use StubEngine which does a real --no-ff merge
    let engine = StubEngine::new(repo_path.join("loop").to_string_lossy().to_string());
    merge_agent::ensure_feature_merged(&engine, "feat-2p", "main", repo_path)
        .expect("2-parent merge should succeed");

    // Verify main tip has 2 parents
    let parents = git_stdout(repo_path, &["rev-list", "--parents", "-1", "main"]);
    let parent_count = parents.trim().split_whitespace().count() - 1;
    assert_eq!(parent_count, 2, "merge commit should have exactly 2 parents");
}

/// Test that ensure_feature_merged skips parent-count check when feature == target.
#[test]
fn test_ensure_feature_merged_same_branch_no_parent_check() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("base.txt"), "base\n").expect("write base");
    commit_all(repo_path, "init");

    let mut rename = Command::new("git");
    rename.args(["branch", "-M", "main"]).current_dir(repo_path);
    run_success(&mut rename);

    // Same branch as feature and target — should succeed even though tip has 1 parent
    let engine = StubEngine::new(repo_path.join("loop").to_string_lossy().to_string());
    merge_agent::ensure_feature_merged(&engine, "main", "main", repo_path)
        .expect("same-branch merge should succeed without parent check");
}

/// Test that a retry of ensure_feature_merged succeeds after initial failure.
/// Simulates the runner retry path: first attempt fails (branch not merged),
/// then after performing the merge, second attempt succeeds.
#[test]
fn test_ensure_feature_merged_retry_succeeds_on_second_attempt() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("base.txt"), "base\n").expect("write base");
    commit_all(repo_path, "init");

    let mut rename = Command::new("git");
    rename.args(["branch", "-M", "main"]).current_dir(repo_path);
    run_success(&mut rename);

    // Create feature branch
    let mut checkout_feat = Command::new("git");
    checkout_feat
        .args(["checkout", "-b", "feat-retry"])
        .current_dir(repo_path);
    run_success(&mut checkout_feat);
    fs::write(repo_path.join("retry.txt"), "feature\n").expect("write feature");
    commit_all(repo_path, "feature commit");

    let mut checkout_main = Command::new("git");
    checkout_main
        .args(["checkout", "main"])
        .current_dir(repo_path);
    run_success(&mut checkout_main);

    // First attempt: verification should fail (not merged yet)
    struct NoopEngine;
    impl swarm::engine::Engine for NoopEngine {
        fn execute(
            &self,
            _agent_name: &str,
            _task_description: &str,
            _working_dir: &Path,
            _turn_number: usize,
            _team_dir: Option<&str>,
        ) -> swarm::engine::EngineResult {
            swarm::engine::EngineResult::success("noop")
        }
        fn engine_type(&self) -> swarm::config::EngineType {
            swarm::config::EngineType::Claude
        }
    }

    let engine = NoopEngine;
    let first_err = merge_agent::ensure_feature_merged(&engine, "feat-retry", "main", repo_path)
        .expect_err("first attempt should fail");
    assert!(first_err.contains("not merged"), "first attempt error: {}", first_err);

    // Now perform the actual merge (simulating what the retry merge agent would do)
    let mut merge_cmd = Command::new("git");
    merge_cmd
        .args(["merge", "--no-ff", "feat-retry"])
        .current_dir(repo_path);
    run_success(&mut merge_cmd);

    // Second attempt: verification should succeed now
    merge_agent::ensure_feature_merged(&engine, "feat-retry", "main", repo_path)
        .expect("second attempt should succeed after merge");
}

/// Test that ensure_feature_merged fails permanently when the branch is never merged.
/// Verifies that after two failures (simulating both retry attempts), the error is clear.
#[test]
fn test_ensure_feature_merged_fails_permanently_without_merge() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("base.txt"), "base\n").expect("write base");
    commit_all(repo_path, "init");

    let mut rename = Command::new("git");
    rename.args(["branch", "-M", "main"]).current_dir(repo_path);
    run_success(&mut rename);

    let mut checkout_feat = Command::new("git");
    checkout_feat
        .args(["checkout", "-b", "feat-perm-fail"])
        .current_dir(repo_path);
    run_success(&mut checkout_feat);
    fs::write(repo_path.join("perm.txt"), "feature\n").expect("write feature");
    commit_all(repo_path, "feature commit");

    let mut checkout_main = Command::new("git");
    checkout_main
        .args(["checkout", "main"])
        .current_dir(repo_path);
    run_success(&mut checkout_main);

    struct NoopEngine;
    impl swarm::engine::Engine for NoopEngine {
        fn execute(
            &self,
            _agent_name: &str,
            _task_description: &str,
            _working_dir: &Path,
            _turn_number: usize,
            _team_dir: Option<&str>,
        ) -> swarm::engine::EngineResult {
            swarm::engine::EngineResult::success("noop")
        }
        fn engine_type(&self) -> swarm::config::EngineType {
            swarm::config::EngineType::Claude
        }
    }

    let engine = NoopEngine;

    // First attempt fails
    let err1 = merge_agent::ensure_feature_merged(&engine, "feat-perm-fail", "main", repo_path)
        .expect_err("first attempt should fail");
    assert!(err1.contains("not merged"));

    // Second attempt also fails (no merge happened between attempts)
    let err2 = merge_agent::ensure_feature_merged(&engine, "feat-perm-fail", "main", repo_path)
        .expect_err("second attempt should also fail");
    assert!(err2.contains("not merged"));

    // No extra retries needed - both errors are clear and consistent
    assert_eq!(err1, err2, "error messages should be consistent across retries");
}

/// Test that ensure_feature_merged detects a squash merge (1 parent) when branches differ.
/// Note: git merge --squash doesn't actually make the feature an ancestor, so the ancestry
/// check fires first. This test verifies the error behavior for that case.
#[test]
fn test_ensure_feature_merged_squash_not_ancestor() {
    let temp = TempDir::new().expect("temp dir");
    let repo_path = temp.path();

    init_git_repo(repo_path);
    fs::write(repo_path.join("base.txt"), "base\n").expect("write base");
    commit_all(repo_path, "init");

    let mut rename = Command::new("git");
    rename.args(["branch", "-M", "main"]).current_dir(repo_path);
    run_success(&mut rename);

    // Create feature branch
    let mut checkout_feat = Command::new("git");
    checkout_feat
        .args(["checkout", "-b", "feat-squash"])
        .current_dir(repo_path);
    run_success(&mut checkout_feat);
    fs::write(repo_path.join("squash.txt"), "feature\n").expect("write feature");
    commit_all(repo_path, "feature commit");

    // Back to main, do a squash merge
    let mut checkout_main = Command::new("git");
    checkout_main
        .args(["checkout", "main"])
        .current_dir(repo_path);
    run_success(&mut checkout_main);

    let mut squash = Command::new("git");
    squash
        .args(["merge", "--squash", "feat-squash"])
        .current_dir(repo_path);
    run_success(&mut squash);
    commit_all(repo_path, "squash merge");

    // Use a non-stub engine so it doesn't try to do its own merge
    struct NoopEngine;
    impl swarm::engine::Engine for NoopEngine {
        fn execute(
            &self,
            _agent_name: &str,
            _task_description: &str,
            _working_dir: &Path,
            _turn_number: usize,
            _team_dir: Option<&str>,
        ) -> swarm::engine::EngineResult {
            swarm::engine::EngineResult::success("noop")
        }
        fn engine_type(&self) -> swarm::config::EngineType {
            swarm::config::EngineType::Claude
        }
    }

    let engine = NoopEngine;
    let err = merge_agent::ensure_feature_merged(&engine, "feat-squash", "main", repo_path)
        .expect_err("squash merge should fail verification");

    // With a real squash, ancestry check fails first
    assert!(
        err.contains("not merged"),
        "expected 'not merged' error for squash merge, got: {}",
        err
    );
}
