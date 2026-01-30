use super::*;
use super::types::detect_target_branch_in;
use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

fn run_git(repo: &Path, args: &[&str]) {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("failed to run git command");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_git_repo(repo: &Path) {
    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.name", "Swarm Test"]);
    run_git(repo, &["config", "user.email", "swarm-test@example.com"]);
    fs::write(repo.join("README.md"), "init").expect("write README");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "init"]);
}

fn git_branch_exists(repo: &Path, branch: &str) -> bool {
    let ref_name = format!("refs/heads/{}", branch);
    ProcessCommand::new("git")
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet", &ref_name])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn git_current_branch(repo: &Path) -> Option<String> {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        None
    } else {
        Some(branch)
    }
}

fn ensure_branch(repo: &Path, branch: &str) {
    if !git_branch_exists(repo, branch) {
        run_git(repo, &[ "branch", branch ]);
    }
}

fn delete_branch_if_exists(repo: &Path, branch: &str) {
    if !git_branch_exists(repo, branch) {
        return;
    }
    if git_current_branch(repo).as_deref() == Some(branch) {
        return;
    }
    run_git(repo, &["branch", "-D", branch]);
}

#[test]
fn test_engine_type_parse() {
    assert_eq!(EngineType::parse("claude"), Some(EngineType::Claude));
    assert_eq!(EngineType::parse("CLAUDE"), Some(EngineType::Claude));
    assert_eq!(EngineType::parse("codex"), Some(EngineType::Codex));
    assert_eq!(EngineType::parse("stub"), Some(EngineType::Stub));
    assert_eq!(EngineType::parse("unknown"), None);
}

#[test]
fn test_engine_type_as_str() {
    assert_eq!(EngineType::Claude.as_str(), "claude");
    assert_eq!(EngineType::Codex.as_str(), "codex");
    assert_eq!(EngineType::Stub.as_str(), "stub");
}

#[test]
fn test_engine_type_parse_list_single() {
    assert_eq!(EngineType::parse_list("claude"), Some(vec![EngineType::Claude]));
    assert_eq!(EngineType::parse_list("codex"), Some(vec![EngineType::Codex]));
    assert_eq!(EngineType::parse_list("stub"), Some(vec![EngineType::Stub]));
}

#[test]
fn test_engine_type_parse_list_multiple() {
    assert_eq!(
        EngineType::parse_list("claude,codex"),
        Some(vec![EngineType::Claude, EngineType::Codex])
    );
    assert_eq!(
        EngineType::parse_list("codex,claude,stub"),
        Some(vec![EngineType::Codex, EngineType::Claude, EngineType::Stub])
    );
}

#[test]
fn test_engine_type_parse_list_weighted() {
    // Test weighted selection format (codex,codex,claude = 2/3 codex, 1/3 claude)
    assert_eq!(
        EngineType::parse_list("codex,codex,claude"),
        Some(vec![EngineType::Codex, EngineType::Codex, EngineType::Claude])
    );
}

#[test]
fn test_engine_type_parse_list_with_spaces() {
    assert_eq!(
        EngineType::parse_list("claude, codex"),
        Some(vec![EngineType::Claude, EngineType::Codex])
    );
    assert_eq!(
        EngineType::parse_list(" codex , claude "),
        Some(vec![EngineType::Codex, EngineType::Claude])
    );
}

#[test]
fn test_engine_type_parse_list_case_insensitive() {
    assert_eq!(
        EngineType::parse_list("CLAUDE,Codex"),
        Some(vec![EngineType::Claude, EngineType::Codex])
    );
}

#[test]
fn test_engine_type_parse_list_invalid() {
    assert_eq!(EngineType::parse_list("unknown"), None);
    assert_eq!(EngineType::parse_list("claude,unknown"), None);
    assert_eq!(EngineType::parse_list(""), None);
}

#[test]
fn test_engine_type_list_to_string() {
    assert_eq!(EngineType::list_to_string(&[EngineType::Claude]), "claude");
    assert_eq!(
        EngineType::list_to_string(&[EngineType::Claude, EngineType::Codex]),
        "claude,codex"
    );
    assert_eq!(
        EngineType::list_to_string(&[EngineType::Codex, EngineType::Codex, EngineType::Claude]),
        "codex,codex,claude"
    );
}

#[test]
fn test_config_select_random_engine_single() {
    let config = Config {
        engine_types: vec![EngineType::Codex],
        ..Default::default()
    };
    // With single engine, should always return that engine
    for _ in 0..10 {
        assert_eq!(config.select_random_engine(), EngineType::Codex);
    }
}

#[test]
fn test_config_select_random_engine_stub_mode_override() {
    let config = Config {
        engine_types: vec![EngineType::Claude, EngineType::Codex],
        engine_stub_mode: true,
        ..Default::default()
    };
    // Stub mode should always return Stub regardless of engine_types
    for _ in 0..10 {
        assert_eq!(config.select_random_engine(), EngineType::Stub);
    }
}

#[test]
fn test_config_select_random_engine_empty_fallback() {
    let config = Config {
        engine_types: vec![],
        ..Default::default()
    };
    // Empty list should fall back to Claude
    assert_eq!(config.select_random_engine(), EngineType::Claude);
}

#[test]
fn test_config_select_random_engine_distribution() {
    let config = Config {
        engine_types: vec![EngineType::Codex, EngineType::Claude],
        ..Default::default()
    };

    // Run many iterations and verify both engines are selected
    let mut claude_count = 0;
    let mut codex_count = 0;
    for _ in 0..100 {
        match config.select_random_engine() {
            EngineType::Claude => claude_count += 1,
            EngineType::Codex => codex_count += 1,
            _ => panic!("unexpected engine type"),
        }
    }
    // Both should be selected at least once (very unlikely to fail with 100 iterations)
    assert!(claude_count > 0, "Claude should be selected at least once");
    assert!(codex_count > 0, "Codex should be selected at least once");
}

#[test]
fn test_config_engines_display() {
    let config = Config {
        engine_types: vec![EngineType::Claude],
        ..Default::default()
    };
    assert_eq!(config.engines_display(), "claude");

    let config = Config {
        engine_types: vec![EngineType::Claude, EngineType::Codex],
        ..Default::default()
    };
    assert_eq!(config.engines_display(), "claude,codex");

    let config = Config {
        engine_types: vec![EngineType::Codex, EngineType::Codex, EngineType::Claude],
        ..Default::default()
    };
    assert_eq!(config.engines_display(), "codex,codex,claude");

    // Stub mode overrides display
    let config = Config {
        engine_types: vec![EngineType::Codex, EngineType::Codex, EngineType::Claude],
        engine_stub_mode: true,
        ..Default::default()
    };
    assert_eq!(config.engines_display(), "stub");
}

#[test]
fn test_config_effective_engine_with_multiple() {
    let config = Config {
        engine_types: vec![EngineType::Codex, EngineType::Claude],
        ..Default::default()
    };
    // effective_engine returns first in list
    assert_eq!(config.effective_engine(), EngineType::Codex);
}

#[test]
fn test_config_parse_toml_with_engine_list() {
    let toml = r#"
[engine]
type = "codex,codex,claude"
"#;
    let config = Config::parse_toml(toml).unwrap();
    assert_eq!(
        config.engine_types,
        vec![EngineType::Codex, EngineType::Codex, EngineType::Claude]
    );
}

#[test]
fn test_config_default() {
    let config = Config::default();
    assert_eq!(config.agents_max_count, 3);
    assert_eq!(config.agents_tasks_per_agent, 2);
    assert_eq!(config.agent_timeout_secs, DEFAULT_AGENT_TIMEOUT_SECS);
    assert_eq!(config.files_tasks, ".swarm-hug/default/tasks.md");
    assert_eq!(config.files_chat, ".swarm-hug/default/chat.md");
    assert_eq!(config.files_log_dir, ".swarm-hug/default/loop");
    assert_eq!(config.files_worktrees_dir, ".swarm-hug/default/worktrees");
    assert_eq!(config.engine_types, vec![EngineType::Claude]);
    assert!(!config.engine_stub_mode);
    assert_eq!(config.sprints_max, 0);
    assert_eq!(config.target_branch, None);
}

#[test]
fn test_config_parse_toml() {
    let toml = r#"
[agents]
max_count = 8
tasks_per_agent = 3

[files]
tasks = "MY_TASKS.md"
chat = "MY_CHAT.md"
log_dir = "logs"

[engine]
type = "codex"
stub_mode = true

[sprints]
max = 5
"#;
    let config = Config::parse_toml(toml).unwrap();
    assert_eq!(config.agents_max_count, 8);
    assert_eq!(config.agents_tasks_per_agent, 3);
    assert_eq!(config.files_tasks, "MY_TASKS.md");
    assert_eq!(config.files_chat, "MY_CHAT.md");
    assert_eq!(config.files_log_dir, "logs");
    assert_eq!(config.engine_types, vec![EngineType::Codex]);
    assert!(config.engine_stub_mode);
    assert_eq!(config.sprints_max, 5);
}

#[test]
fn test_config_effective_engine() {
    let config = Config {
        engine_types: vec![EngineType::Claude],
        ..Default::default()
    };
    assert_eq!(config.effective_engine(), EngineType::Claude);

    let config = Config {
        engine_types: vec![EngineType::Claude],
        engine_stub_mode: true,
        ..Default::default()
    };
    assert_eq!(config.effective_engine(), EngineType::Stub);
}

#[test]
fn test_parse_args_command() {
    let args = vec!["swarm".to_string(), "init".to_string()];
    let cli = parse_args(args);
    assert_eq!(cli.command, Some(Command::Init));
}

#[test]
fn test_parse_args_run() {
    let args = vec!["swarm".to_string(), "run".to_string()];
    let cli = parse_args(args);
    assert_eq!(cli.command, Some(Command::Run));
}

#[test]
fn test_parse_args_flags() {
    let args = vec![
        "swarm".to_string(),
        "--max-sprints".to_string(),
        "3".to_string(),
        "--stub".to_string(),
        "run".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.command, Some(Command::Run));
    assert_eq!(cli.max_sprints, Some(3));
    assert!(cli.stub);
}

#[test]
fn test_parse_args_engine_list() {
    let args = vec![
        "swarm".to_string(),
        "--engine".to_string(),
        "codex,codex,claude".to_string(),
        "run".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.engine, Some("codex,codex,claude".to_string()));
}

#[test]
fn test_parse_args_target_branch() {
    let args = vec![
        "swarm".to_string(),
        "--target-branch".to_string(),
        "develop".to_string(),
        "run".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.target_branch, Some("develop".to_string()));
}

#[test]
fn test_config_apply_cli_engine_list() {
    let mut config = Config::default();
    let cli = CliArgs {
        engine: Some("codex,claude".to_string()),
        ..Default::default()
    };
    config.apply_cli(&cli);
    assert_eq!(config.engine_types, vec![EngineType::Codex, EngineType::Claude]);
}

#[test]
fn test_config_apply_cli_target_branch() {
    let mut config = Config::default();
    let cli = CliArgs {
        target_branch: Some("mainline".to_string()),
        ..Default::default()
    };
    config.apply_cli(&cli);
    assert_eq!(config.target_branch, Some("mainline".to_string()));
}

#[test]
fn test_parse_args_help() {
    let args = vec!["swarm".to_string(), "--help".to_string()];
    let cli = parse_args(args);
    assert!(cli.help);
}

#[test]
fn test_parse_args_config() {
    let args = vec![
        "swarm".to_string(),
        "-c".to_string(),
        "custom.toml".to_string(),
        "run".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.config, Some("custom.toml".to_string()));
    assert_eq!(cli.command, Some(Command::Run));
}

#[test]
fn test_command_parse() {
    assert_eq!(Command::parse("init"), Some(Command::Init));
    assert_eq!(Command::parse("run"), Some(Command::Run));
    assert_eq!(Command::parse("sprint"), None); // sprint command removed
    assert_eq!(Command::parse("plan"), None); // plan command removed
    assert_eq!(Command::parse("status"), None); // status command removed
    assert_eq!(Command::parse("agents"), Some(Command::Agents));
    assert_eq!(Command::parse("worktrees"), None); // worktrees command removed
    assert_eq!(Command::parse("worktrees-branch"), None); // worktrees-branch command removed
    assert_eq!(Command::parse("cleanup"), None); // cleanup command removed
    assert_eq!(Command::parse("projects"), Some(Command::Projects));
    assert_eq!(Command::parse("project"), Some(Command::ProjectInit));
    assert_eq!(Command::parse("customize-prompts"), Some(Command::CustomizePrompts));
    assert_eq!(Command::parse("cleanup-worktrees"), Some(Command::CleanupWorktrees));
    assert_eq!(Command::parse("set-email"), Some(Command::SetEmail));
    assert_eq!(Command::parse("unknown"), None);
}

#[test]
fn test_parse_args_unknown_command() {
    let args = vec!["swarm".to_string(), "sprint".to_string()];
    let cli = parse_args(args);
    assert_eq!(cli.command, None);
    assert_eq!(cli.unknown_command, Some("sprint".to_string()));
}

#[test]
fn test_parse_args_set_email() {
    let args = vec![
        "swarm".to_string(),
        "set-email".to_string(),
        "user@example.com".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.command, Some(Command::SetEmail));
    assert_eq!(cli.email_arg, Some("user@example.com".to_string()));
}

#[test]
fn test_detect_target_branch_prefers_main() {
    let temp = TempDir::new().expect("temp dir");
    init_git_repo(temp.path());
    ensure_branch(temp.path(), "main");
    run_git(temp.path(), &["checkout", "-b", "dev"]);

    let detected = detect_target_branch_in(Some(temp.path()));
    assert_eq!(detected.as_deref(), Some("main"));
}

#[test]
fn test_detect_target_branch_falls_back_to_master() {
    let temp = TempDir::new().expect("temp dir");
    init_git_repo(temp.path());
    ensure_branch(temp.path(), "master");
    run_git(temp.path(), &["checkout", "-b", "feature"]);
    delete_branch_if_exists(temp.path(), "main");

    let detected = detect_target_branch_in(Some(temp.path()));
    assert_eq!(detected.as_deref(), Some("master"));
}

#[test]
fn test_detect_target_branch_falls_back_to_current_branch() {
    let temp = TempDir::new().expect("temp dir");
    init_git_repo(temp.path());
    run_git(temp.path(), &["checkout", "-b", "feature"]);
    delete_branch_if_exists(temp.path(), "main");
    delete_branch_if_exists(temp.path(), "master");

    let detected = detect_target_branch_in(Some(temp.path()));
    assert_eq!(detected.as_deref(), Some("feature"));
}

#[test]
fn test_parse_args_project() {
    let args = vec![
        "swarm".to_string(),
        "--project".to_string(),
        "authentication".to_string(),
        "run".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.command, Some(Command::Run));
    assert_eq!(cli.project, Some("authentication".to_string()));
}

#[test]
fn test_parse_args_project_init() {
    let args = vec![
        "swarm".to_string(),
        "project".to_string(),
        "init".to_string(),
        "payments".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.command, Some(Command::ProjectInit));
    assert_eq!(cli.project_arg, Some("payments".to_string()));
}

#[test]
fn test_project_path_resolution() {
    let cli = CliArgs {
        project: Some("authentication".to_string()),
        ..Default::default()
    };
    let config = Config::load(&cli);
    assert_eq!(config.project, Some("authentication".to_string()));
    assert_eq!(config.files_tasks, ".swarm-hug/authentication/tasks.md");
    assert_eq!(config.files_chat, ".swarm-hug/authentication/chat.md");
    assert_eq!(config.files_log_dir, ".swarm-hug/authentication/loop");
    assert_eq!(config.files_worktrees_dir, ".swarm-hug/authentication/worktrees");
}

#[test]
fn test_default_toml() {
    let toml = Config::default_toml();
    assert!(toml.contains("max_count = 3"));
    assert!(toml.contains("tasks_per_agent = 2"));
    assert!(toml.contains("tasks = \".swarm-hug/default/tasks.md\""));
    assert!(toml.contains("chat = \".swarm-hug/default/chat.md\""));
    assert!(toml.contains("log_dir = \".swarm-hug/default/loop\""));
}

#[test]
fn test_config_load_with_cli_precedence() {
    let cli = CliArgs {
        max_sprints: Some(10),
        stub: true,
        ..Default::default()
    };

    let config = Config::load(&cli);
    assert_eq!(config.sprints_max, 10);
    assert!(config.engine_stub_mode);
    assert_eq!(config.effective_engine(), EngineType::Stub);
}

#[test]
fn test_config_load_cli_target_branch_overrides_auto_detection() {
    let cli = CliArgs {
        target_branch: Some("override-branch".to_string()),
        ..Default::default()
    };

    let config = Config::load(&cli);
    assert_eq!(config.target_branch.as_deref(), Some("override-branch"));
}

#[test]
fn test_parse_args_with_prd() {
    let args = vec![
        "swarm".to_string(),
        "project".to_string(),
        "init".to_string(),
        "myproject".to_string(),
        "--with-prd".to_string(),
        "specs/prd.md".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.command, Some(Command::ProjectInit));
    assert_eq!(cli.project_arg, Some("myproject".to_string()));
    assert_eq!(cli.prd_file_arg, Some("specs/prd.md".to_string()));
}

#[test]
fn test_parse_args_with_prd_before_project_name() {
    // Test that --with-prd can appear before the project name
    let args = vec![
        "swarm".to_string(),
        "--with-prd".to_string(),
        "prd.md".to_string(),
        "project".to_string(),
        "init".to_string(),
        "myproject".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.command, Some(Command::ProjectInit));
    assert_eq!(cli.project_arg, Some("myproject".to_string()));
    assert_eq!(cli.prd_file_arg, Some("prd.md".to_string()));
}

#[test]
fn test_parse_args_with_prd_no_value() {
    // If --with-prd is at the end with no value, prd_file_arg should be None
    let args = vec![
        "swarm".to_string(),
        "project".to_string(),
        "init".to_string(),
        "myproject".to_string(),
        "--with-prd".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.command, Some(Command::ProjectInit));
    assert_eq!(cli.project_arg, Some("myproject".to_string()));
    assert_eq!(cli.prd_file_arg, None);
}

#[test]
fn test_parse_args_agent_timeout() {
    let args = vec![
        "swarm".to_string(),
        "--agent-timeout".to_string(),
        "1800".to_string(),
        "run".to_string(),
    ];
    let cli = parse_args(args);
    assert_eq!(cli.command, Some(Command::Run));
    assert_eq!(cli.agent_timeout, Some(1800));
}

#[test]
fn test_config_with_agent_timeout_cli() {
    let cli = CliArgs {
        agent_timeout: Some(900),
        ..Default::default()
    };

    let config = Config::load(&cli);
    assert_eq!(config.agent_timeout_secs, 900);
}

#[test]
fn test_config_parse_toml_with_timeout() {
    let toml = r#"
[agents]
max_count = 4
tasks_per_agent = 2
timeout = 1800
"#;
    let config = Config::parse_toml(toml).unwrap();
    assert_eq!(config.agent_timeout_secs, 1800);
}

#[test]
fn test_default_toml_includes_timeout() {
    let toml = Config::default_toml();
    assert!(toml.contains("timeout = 3600"));
}
