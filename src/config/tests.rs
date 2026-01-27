use super::*;

#[test]
fn test_engine_type_from_str() {
    assert_eq!(EngineType::from_str("claude"), Some(EngineType::Claude));
    assert_eq!(EngineType::from_str("CLAUDE"), Some(EngineType::Claude));
    assert_eq!(EngineType::from_str("codex"), Some(EngineType::Codex));
    assert_eq!(EngineType::from_str("stub"), Some(EngineType::Stub));
    assert_eq!(EngineType::from_str("unknown"), None);
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
    let mut config = Config::default();
    config.engine_types = vec![EngineType::Codex];
    // With single engine, should always return that engine
    for _ in 0..10 {
        assert_eq!(config.select_random_engine(), EngineType::Codex);
    }
}

#[test]
fn test_config_select_random_engine_stub_mode_override() {
    let mut config = Config::default();
    config.engine_types = vec![EngineType::Claude, EngineType::Codex];
    config.engine_stub_mode = true;
    // Stub mode should always return Stub regardless of engine_types
    for _ in 0..10 {
        assert_eq!(config.select_random_engine(), EngineType::Stub);
    }
}

#[test]
fn test_config_select_random_engine_empty_fallback() {
    let mut config = Config::default();
    config.engine_types = vec![];
    // Empty list should fall back to Claude
    assert_eq!(config.select_random_engine(), EngineType::Claude);
}

#[test]
fn test_config_select_random_engine_distribution() {
    let mut config = Config::default();
    config.engine_types = vec![EngineType::Codex, EngineType::Claude];

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
    let mut config = Config::default();
    config.engine_types = vec![EngineType::Claude];
    assert_eq!(config.engines_display(), "claude");

    config.engine_types = vec![EngineType::Claude, EngineType::Codex];
    assert_eq!(config.engines_display(), "claude,codex");

    config.engine_types = vec![EngineType::Codex, EngineType::Codex, EngineType::Claude];
    assert_eq!(config.engines_display(), "codex,codex,claude");

    // Stub mode overrides display
    config.engine_stub_mode = true;
    assert_eq!(config.engines_display(), "stub");
}

#[test]
fn test_config_effective_engine_with_multiple() {
    let mut config = Config::default();
    config.engine_types = vec![EngineType::Codex, EngineType::Claude];
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
    let mut config = Config::default();
    config.engine_types = vec![EngineType::Claude];
    assert_eq!(config.effective_engine(), EngineType::Claude);

    config.engine_stub_mode = true;
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
fn test_command_from_str() {
    assert_eq!(Command::from_str("init"), Some(Command::Init));
    assert_eq!(Command::from_str("run"), Some(Command::Run));
    assert_eq!(Command::from_str("sprint"), None); // sprint command removed
    assert_eq!(Command::from_str("plan"), None); // plan command removed
    assert_eq!(Command::from_str("status"), None); // status command removed
    assert_eq!(Command::from_str("agents"), Some(Command::Agents));
    assert_eq!(Command::from_str("worktrees"), None); // worktrees command removed
    assert_eq!(Command::from_str("worktrees-branch"), None); // worktrees-branch command removed
    assert_eq!(Command::from_str("cleanup"), None); // cleanup command removed
    assert_eq!(Command::from_str("projects"), Some(Command::Projects));
    assert_eq!(Command::from_str("project"), Some(Command::ProjectInit));
    assert_eq!(Command::from_str("customize-prompts"), Some(Command::CustomizePrompts));
    assert_eq!(Command::from_str("set-email"), Some(Command::SetEmail));
    assert_eq!(Command::from_str("unknown"), None);
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
    let mut cli = CliArgs::default();
    cli.project = Some("authentication".to_string());
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
    let mut cli = CliArgs::default();
    cli.max_sprints = Some(10);
    cli.stub = true;

    let config = Config::load(&cli);
    assert_eq!(config.sprints_max, 10);
    assert!(config.engine_stub_mode);
    assert_eq!(config.effective_engine(), EngineType::Stub);
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
    let mut cli = CliArgs::default();
    cli.agent_timeout = Some(900);

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
