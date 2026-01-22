//! Configuration loading for swarm.
//!
//! Supports swarm.toml, CLI flags, and environment variables.
//! Precedence (highest to lowest): CLI flags > env vars > config file > defaults.

use std::env;
use std::fs;
use std::path::Path;

/// Engine type for agent execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EngineType {
    /// Claude CLI engine.
    #[default]
    Claude,
    /// Codex CLI engine.
    Codex,
    /// Stubbed engine for tests (no network).
    Stub,
}

impl EngineType {
    /// Parse engine type from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "stub" => Some(Self::Stub),
            _ => None,
        }
    }

    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Stub => "stub",
        }
    }
}

/// Swarm configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Maximum number of agents that may be spawned.
    pub agents_max_count: usize,
    /// Number of tasks to assign per agent per sprint.
    pub agents_tasks_per_agent: usize,
    /// Path to TASKS.md file.
    pub files_tasks: String,
    /// Path to CHAT.md file.
    pub files_chat: String,
    /// Path to log directory.
    pub files_log_dir: String,
    /// Path to worktrees directory.
    pub files_worktrees_dir: String,
    /// Engine type for agent execution.
    pub engine_type: EngineType,
    /// Enable stub mode for testing (overrides engine_type to Stub).
    pub engine_stub_mode: bool,
    /// Maximum sprints to run (0 means unlimited).
    pub sprints_max: usize,
    /// Disable tailing CHAT.md during run.
    pub no_tail: bool,
    /// Team name for multi-team mode.
    pub team: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agents_max_count: 4,
            agents_tasks_per_agent: 2,
            files_tasks: ".swarm-hug/default/tasks.md".to_string(),
            files_chat: ".swarm-hug/default/chat.md".to_string(),
            files_log_dir: ".swarm-hug/default/loop".to_string(),
            files_worktrees_dir: ".swarm-hug/default/worktrees".to_string(),
            engine_type: EngineType::Claude,
            engine_stub_mode: false,
            sprints_max: 0,
            no_tail: false,
            team: None,
        }
    }
}

impl Config {
    /// Load configuration from all sources with proper precedence.
    ///
    /// Precedence: CLI args > env vars > config file > defaults.
    ///
    /// When a team is specified via `--team`, paths are resolved relative to
    /// `.swarm-hug/<team>/` unless explicitly overridden.
    pub fn load(cli_args: &CliArgs) -> Self {
        let mut config = Self::default();

        // Load from config file if present
        if let Some(ref path) = cli_args.config {
            if let Ok(file_config) = Self::load_from_file(path) {
                config.merge_from(&file_config);
            }
        } else if Path::new("swarm.toml").exists() {
            if let Ok(file_config) = Self::load_from_file("swarm.toml") {
                config.merge_from(&file_config);
            }
        }

        // Apply environment variables
        config.apply_env();

        // Apply CLI args (highest precedence)
        config.apply_cli(cli_args);

        // Stub mode overrides engine type
        if config.engine_stub_mode {
            config.engine_type = EngineType::Stub;
        }

        // Apply team-based path resolution if team is set and paths weren't explicitly overridden
        if config.team.is_some() {
            let team_name = config.team.clone().unwrap();
            config.apply_team_paths(&team_name, cli_args);
        }

        config
    }

    /// Apply team-based path defaults.
    /// Only applies if the path wasn't explicitly set via CLI.
    fn apply_team_paths(&mut self, team_name: &str, cli_args: &CliArgs) {
        let team_root = format!(".swarm-hug/{}", team_name);

        // Only override if not explicitly set
        if cli_args.tasks_file.is_none() {
            self.files_tasks = format!("{}/tasks.md", team_root);
        }
        if cli_args.chat_file.is_none() {
            self.files_chat = format!("{}/chat.md", team_root);
        }
        if cli_args.log_dir.is_none() {
            self.files_log_dir = format!("{}/loop", team_root);
        }
        // Worktrees always use team path when team is set
        self.files_worktrees_dir = format!("{}/worktrees", team_root);
    }

    /// Load configuration from a TOML file.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(&path)
            .map_err(|e| ConfigError::Io(e.to_string()))?;
        Self::parse_toml(&content)
    }

    /// Parse TOML content into configuration.
    fn parse_toml(content: &str) -> Result<Self, ConfigError> {
        let mut config = Self::default();
        let mut current_section = String::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Handle section headers like [agents]
            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len()-1].to_string();
                continue;
            }

            if let Some((key, value)) = parse_toml_line(line) {
                // Build full key with section prefix
                let full_key = if current_section.is_empty() {
                    key.to_string()
                } else {
                    format!("{}.{}", current_section, key)
                };

                match full_key.as_str() {
                    "agents.max_count" => {
                        config.agents_max_count = value.parse()
                            .map_err(|_| ConfigError::Parse(format!("invalid agents.max_count: {}", value)))?;
                    }
                    "agents.tasks_per_agent" => {
                        config.agents_tasks_per_agent = value.parse()
                            .map_err(|_| ConfigError::Parse(format!("invalid agents.tasks_per_agent: {}", value)))?;
                    }
                    "files.tasks" => {
                        config.files_tasks = value.trim_matches('"').to_string();
                    }
                    "files.chat" => {
                        config.files_chat = value.trim_matches('"').to_string();
                    }
                    "files.log_dir" => {
                        config.files_log_dir = value.trim_matches('"').to_string();
                    }
                    "engine.type" => {
                        let engine_str = value.trim_matches('"');
                        config.engine_type = EngineType::from_str(engine_str)
                            .ok_or_else(|| ConfigError::Parse(format!("invalid engine.type: {}", engine_str)))?;
                    }
                    "engine.stub_mode" => {
                        config.engine_stub_mode = value == "true";
                    }
                    "sprints.max" => {
                        config.sprints_max = value.parse()
                            .map_err(|_| ConfigError::Parse(format!("invalid sprints.max: {}", value)))?;
                    }
                    _ => {} // Ignore unknown keys
                }
            }
        }

        Ok(config)
    }

    /// Apply environment variables.
    fn apply_env(&mut self) {
        if let Ok(val) = env::var("SWARM_AGENTS_MAX_COUNT") {
            if let Ok(n) = val.parse() {
                self.agents_max_count = n;
            }
        }
        if let Ok(val) = env::var("SWARM_AGENTS_TASKS_PER_AGENT") {
            if let Ok(n) = val.parse() {
                self.agents_tasks_per_agent = n;
            }
        }
        if let Ok(val) = env::var("SWARM_FILES_TASKS") {
            self.files_tasks = val;
        }
        if let Ok(val) = env::var("SWARM_FILES_CHAT") {
            self.files_chat = val;
        }
        if let Ok(val) = env::var("SWARM_FILES_LOG_DIR") {
            self.files_log_dir = val;
        }
        if let Ok(val) = env::var("SWARM_ENGINE_TYPE") {
            if let Some(engine) = EngineType::from_str(&val) {
                self.engine_type = engine;
            }
        }
        if let Ok(val) = env::var("SWARM_ENGINE_STUB_MODE") {
            self.engine_stub_mode = val == "true" || val == "1";
        }
        if let Ok(val) = env::var("SWARM_SPRINTS_MAX") {
            if let Ok(n) = val.parse() {
                self.sprints_max = n;
            }
        }
    }

    /// Apply CLI arguments.
    fn apply_cli(&mut self, args: &CliArgs) {
        if let Some(n) = args.max_agents {
            self.agents_max_count = n;
        }
        if let Some(n) = args.tasks_per_agent {
            self.agents_tasks_per_agent = n;
        }
        if let Some(ref path) = args.tasks_file {
            self.files_tasks = path.clone();
        }
        if let Some(ref path) = args.chat_file {
            self.files_chat = path.clone();
        }
        if let Some(ref path) = args.log_dir {
            self.files_log_dir = path.clone();
        }
        if let Some(ref engine) = args.engine {
            if let Some(e) = EngineType::from_str(engine) {
                self.engine_type = e;
            }
        }
        if args.stub {
            self.engine_stub_mode = true;
        }
        if let Some(n) = args.max_sprints {
            self.sprints_max = n;
        }
        if args.no_tail {
            self.no_tail = true;
        }
        if let Some(ref team) = args.team {
            self.team = Some(team.clone());
        }
    }

    /// Merge values from another config (for file-based config).
    fn merge_from(&mut self, other: &Self) {
        self.agents_max_count = other.agents_max_count;
        self.agents_tasks_per_agent = other.agents_tasks_per_agent;
        self.files_tasks = other.files_tasks.clone();
        self.files_chat = other.files_chat.clone();
        self.files_log_dir = other.files_log_dir.clone();
        self.engine_type = other.engine_type;
        self.engine_stub_mode = other.engine_stub_mode;
        self.sprints_max = other.sprints_max;
    }

    /// Generate default swarm.toml content.
    pub fn default_toml() -> String {
        r#"# Swarm configuration

[agents]
max_count = 4
tasks_per_agent = 2

[files]
tasks = ".swarm-hug/default/tasks.md"
chat = ".swarm-hug/default/chat.md"
log_dir = ".swarm-hug/default/loop"

[engine]
type = "claude"
stub_mode = false

[sprints]
max = 0
"#.to_string()
    }

    /// Get the effective engine type (considering stub_mode).
    pub fn effective_engine(&self) -> EngineType {
        if self.engine_stub_mode {
            EngineType::Stub
        } else {
            self.engine_type
        }
    }
}

/// CLI arguments parsed from command line.
#[derive(Debug, Default)]
pub struct CliArgs {
    /// Subcommand to execute.
    pub command: Option<Command>,
    /// Path to config file.
    pub config: Option<String>,
    /// Maximum number of agents.
    pub max_agents: Option<usize>,
    /// Tasks per agent per sprint.
    pub tasks_per_agent: Option<usize>,
    /// Path to tasks file.
    pub tasks_file: Option<String>,
    /// Path to chat file.
    pub chat_file: Option<String>,
    /// Path to log directory.
    pub log_dir: Option<String>,
    /// Engine type.
    pub engine: Option<String>,
    /// Enable stub mode.
    pub stub: bool,
    /// Maximum sprints to run.
    pub max_sprints: Option<usize>,
    /// Disable tailing.
    pub no_tail: bool,
    /// Show help.
    pub help: bool,
    /// Show version.
    pub version: bool,
    /// Team name for multi-team mode.
    pub team: Option<String>,
    /// Team name for team-specific subcommands (positional arg).
    pub team_arg: Option<String>,
}

/// Swarm subcommands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Initialize a new swarm project.
    Init,
    /// Run sprints until done or max reached.
    Run,
    /// Run exactly one sprint.
    Sprint,
    /// Run sprint planning only.
    Plan,
    /// Show task status.
    Status,
    /// List agent names/initials.
    Agents,
    /// List worktrees.
    Worktrees,
    /// List worktree branches.
    WorktreesBranch,
    /// Clean up worktrees and branches.
    Cleanup,
    /// Merge agent branches.
    Merge,
    /// Tail CHAT.md.
    Tail,
    /// List all teams and their assigned agents.
    Teams,
    /// Initialize a new team (use with team name argument).
    TeamInit,
    /// Copy embedded prompts to .swarm-hug/prompts for customization.
    CustomizePrompts,
}

impl Command {
    /// Parse command from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "init" => Some(Self::Init),
            "run" => Some(Self::Run),
            "sprint" => Some(Self::Sprint),
            "plan" => Some(Self::Plan),
            "status" => Some(Self::Status),
            "agents" => Some(Self::Agents),
            "worktrees" => Some(Self::Worktrees),
            "worktrees-branch" => Some(Self::WorktreesBranch),
            "cleanup" => Some(Self::Cleanup),
            "merge" => Some(Self::Merge),
            "tail" => Some(Self::Tail),
            "teams" => Some(Self::Teams),
            "team" => Some(Self::TeamInit),
            "customize-prompts" => Some(Self::CustomizePrompts),
            _ => None,
        }
    }
}

/// Configuration errors.
#[derive(Debug)]
pub enum ConfigError {
    /// I/O error reading config file.
    Io(String),
    /// Parse error in config file.
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "config I/O error: {}", msg),
            Self::Parse(msg) => write!(f, "config parse error: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Parse a TOML line into key-value pair.
/// Handles dotted keys like "agents.max_count = 4".
fn parse_toml_line(line: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return None;
    }
    Some((parts[0].trim(), parts[1].trim()))
}

/// Parse CLI arguments from an iterator.
pub fn parse_args<I>(args: I) -> CliArgs
where
    I: IntoIterator<Item = String>,
{
    let mut cli = CliArgs::default();
    let mut args = args.into_iter().peekable();

    // Skip program name
    args.next();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => cli.help = true,
            "-V" | "--version" => cli.version = true,
            "-c" | "--config" => cli.config = args.next(),
            "-t" | "--team" => cli.team = args.next(),
            "--max-agents" => cli.max_agents = args.next().and_then(|s| s.parse().ok()),
            "--tasks-per-agent" => cli.tasks_per_agent = args.next().and_then(|s| s.parse().ok()),
            "--tasks-file" => cli.tasks_file = args.next(),
            "--chat-file" => cli.chat_file = args.next(),
            "--log-dir" => cli.log_dir = args.next(),
            "--engine" => cli.engine = args.next(),
            "--stub" => cli.stub = true,
            "--max-sprints" => cli.max_sprints = args.next().and_then(|s| s.parse().ok()),
            "--no-tail" => cli.no_tail = true,
            _ if !arg.starts_with('-') && cli.command.is_none() => {
                cli.command = Command::from_str(&arg);
                // For "team init <name>", capture the next arg as team_arg
                if cli.command == Some(Command::TeamInit) {
                    // Check if next arg is "init" (team init <name> format)
                    if let Some(next) = args.peek() {
                        if next == "init" {
                            args.next(); // consume "init"
                            cli.team_arg = args.next(); // team name
                        } else if !next.starts_with('-') {
                            // Just "team <name>" - treat as team init
                            cli.team_arg = args.next();
                        }
                    }
                }
            }
            _ => {} // Ignore unknown flags
        }
    }

    cli
}

#[cfg(test)]
mod tests {
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
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.agents_max_count, 4);
        assert_eq!(config.agents_tasks_per_agent, 2);
        assert_eq!(config.files_tasks, ".swarm-hug/default/tasks.md");
        assert_eq!(config.files_chat, ".swarm-hug/default/chat.md");
        assert_eq!(config.files_log_dir, ".swarm-hug/default/loop");
        assert_eq!(config.files_worktrees_dir, ".swarm-hug/default/worktrees");
        assert_eq!(config.engine_type, EngineType::Claude);
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
        assert_eq!(config.engine_type, EngineType::Codex);
        assert!(config.engine_stub_mode);
        assert_eq!(config.sprints_max, 5);
    }

    #[test]
    fn test_config_effective_engine() {
        let mut config = Config::default();
        config.engine_type = EngineType::Claude;
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
            "--no-tail".to_string(),
            "run".to_string(),
        ];
        let cli = parse_args(args);
        assert_eq!(cli.command, Some(Command::Run));
        assert_eq!(cli.max_sprints, Some(3));
        assert!(cli.stub);
        assert!(cli.no_tail);
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
        assert_eq!(Command::from_str("sprint"), Some(Command::Sprint));
        assert_eq!(Command::from_str("plan"), Some(Command::Plan));
        assert_eq!(Command::from_str("status"), Some(Command::Status));
        assert_eq!(Command::from_str("agents"), Some(Command::Agents));
        assert_eq!(Command::from_str("worktrees"), Some(Command::Worktrees));
        assert_eq!(Command::from_str("worktrees-branch"), Some(Command::WorktreesBranch));
        assert_eq!(Command::from_str("cleanup"), Some(Command::Cleanup));
        assert_eq!(Command::from_str("merge"), Some(Command::Merge));
        assert_eq!(Command::from_str("tail"), Some(Command::Tail));
        assert_eq!(Command::from_str("teams"), Some(Command::Teams));
        assert_eq!(Command::from_str("team"), Some(Command::TeamInit));
        assert_eq!(Command::from_str("unknown"), None);
    }

    #[test]
    fn test_parse_args_team() {
        let args = vec![
            "swarm".to_string(),
            "--team".to_string(),
            "authentication".to_string(),
            "run".to_string(),
        ];
        let cli = parse_args(args);
        assert_eq!(cli.command, Some(Command::Run));
        assert_eq!(cli.team, Some("authentication".to_string()));
    }

    #[test]
    fn test_parse_args_team_init() {
        let args = vec![
            "swarm".to_string(),
            "team".to_string(),
            "init".to_string(),
            "payments".to_string(),
        ];
        let cli = parse_args(args);
        assert_eq!(cli.command, Some(Command::TeamInit));
        assert_eq!(cli.team_arg, Some("payments".to_string()));
    }

    #[test]
    fn test_team_path_resolution() {
        let mut cli = CliArgs::default();
        cli.team = Some("authentication".to_string());
        let config = Config::load(&cli);
        assert_eq!(config.team, Some("authentication".to_string()));
        assert_eq!(config.files_tasks, ".swarm-hug/authentication/tasks.md");
        assert_eq!(config.files_chat, ".swarm-hug/authentication/chat.md");
        assert_eq!(config.files_log_dir, ".swarm-hug/authentication/loop");
        assert_eq!(config.files_worktrees_dir, ".swarm-hug/authentication/worktrees");
    }

    #[test]
    fn test_default_toml() {
        let toml = Config::default_toml();
        assert!(toml.contains("max_count = 4"));
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
}
