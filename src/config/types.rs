use std::path::Path;
use std::process::Command;

use super::cli::CliArgs;
use super::{env, toml};

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
    pub fn parse(s: &str) -> Option<Self> {
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

    /// Parse a comma-separated list of engine types.
    /// Returns None if any engine type is invalid or the list is empty.
    /// Duplicates are allowed (e.g., "codex,codex,claude" for weighted selection).
    pub fn parse_list(s: &str) -> Option<Vec<Self>> {
        let engines: Vec<Self> = s
            .split(',')
            .map(|part| Self::parse(part.trim()))
            .collect::<Option<Vec<_>>>()?;

        if engines.is_empty() {
            None
        } else {
            Some(engines)
        }
    }

    /// Format a list of engine types as a comma-separated string.
    pub fn list_to_string(engines: &[Self]) -> String {
        engines.iter().map(|e| e.as_str()).collect::<Vec<_>>().join(",")
    }
}

/// Default agent timeout in seconds (60 minutes).
pub const DEFAULT_AGENT_TIMEOUT_SECS: u64 = 3600;

/// Swarm configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Maximum number of agents that may be spawned.
    pub agents_max_count: usize,
    /// Number of tasks to assign per agent per sprint.
    pub agents_tasks_per_agent: usize,
    /// Agent execution timeout in seconds.
    pub agent_timeout_secs: u64,
    /// Path to TASKS.md file.
    pub files_tasks: String,
    /// Path to CHAT.md file.
    pub files_chat: String,
    /// Path to log directory.
    pub files_log_dir: String,
    /// Path to worktrees directory.
    pub files_worktrees_dir: String,
    /// Engine types for agent execution (supports weighted random selection).
    /// When multiple engines are specified, one is randomly selected per agent.
    pub engine_types: Vec<EngineType>,
    /// Enable stub mode for testing (overrides engine_types to Stub).
    pub engine_stub_mode: bool,
    /// Maximum sprints to run (0 means unlimited).
    pub sprints_max: usize,
    /// Project name for multi-project mode.
    pub project: Option<String>,
    /// Target branch for base/merge operations (defaults to auto-detected main/master).
    pub target_branch: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agents_max_count: 3,
            agents_tasks_per_agent: 2,
            agent_timeout_secs: DEFAULT_AGENT_TIMEOUT_SECS,
            files_tasks: ".swarm-hug/default/tasks.md".to_string(),
            files_chat: ".swarm-hug/default/chat.md".to_string(),
            files_log_dir: ".swarm-hug/default/loop".to_string(),
            files_worktrees_dir: ".swarm-hug/default/worktrees".to_string(),
            engine_types: vec![EngineType::Claude],
            engine_stub_mode: false,
            sprints_max: 0,
            project: None,
            target_branch: None,
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

        // Stub mode overrides engine types
        if config.engine_stub_mode {
            config.engine_types = vec![EngineType::Stub];
        }

        // Apply project-based path resolution if project is set and paths weren't explicitly overridden
        if config.project.is_some() {
            let project_name = config.project.clone().unwrap();
            config.apply_project_paths(&project_name, cli_args);
        }

        if config.target_branch.is_none() {
            config.target_branch = detect_target_branch();
        }

        config
    }

    /// Apply project-based path defaults.
    /// Only applies if the path wasn't explicitly set via CLI.
    fn apply_project_paths(&mut self, project_name: &str, cli_args: &CliArgs) {
        let project_root = format!(".swarm-hug/{}", project_name);

        // Only override if not explicitly set
        if cli_args.tasks_file.is_none() {
            self.files_tasks = format!("{}/tasks.md", project_root);
        }
        if cli_args.chat_file.is_none() {
            self.files_chat = format!("{}/chat.md", project_root);
        }
        if cli_args.log_dir.is_none() {
            self.files_log_dir = format!("{}/loop", project_root);
        }
        // Worktrees always use project path when project is set
        self.files_worktrees_dir = format!("{}/worktrees", project_root);
    }

    /// Load configuration from a TOML file.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        toml::load_from_file(path)
    }

    /// Parse TOML content into configuration.
    pub(super) fn parse_toml(content: &str) -> Result<Self, ConfigError> {
        toml::parse_toml(content)
    }

    /// Apply environment variables.
    fn apply_env(&mut self) {
        env::apply_env(self);
    }

    /// Apply CLI arguments.
    pub(super) fn apply_cli(&mut self, args: &CliArgs) {
        if let Some(n) = args.max_agents {
            self.agents_max_count = n;
        }
        if let Some(n) = args.tasks_per_agent {
            self.agents_tasks_per_agent = n;
        }
        if let Some(n) = args.agent_timeout {
            self.agent_timeout_secs = n;
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
            if let Some(engines) = EngineType::parse_list(engine) {
                self.engine_types = engines;
            }
        }
        if args.stub {
            self.engine_stub_mode = true;
        }
        if let Some(n) = args.max_sprints {
            self.sprints_max = n;
        }
        if let Some(ref project) = args.project {
            self.project = Some(project.clone());
        }
        if let Some(ref target) = args.target_branch {
            self.target_branch = Some(target.clone());
        }
    }

    /// Merge values from another config (for file-based config).
    fn merge_from(&mut self, other: &Self) {
        self.agents_max_count = other.agents_max_count;
        self.agents_tasks_per_agent = other.agents_tasks_per_agent;
        self.agent_timeout_secs = other.agent_timeout_secs;
        self.files_tasks = other.files_tasks.clone();
        self.files_chat = other.files_chat.clone();
        self.files_log_dir = other.files_log_dir.clone();
        self.engine_types = other.engine_types.clone();
        self.engine_stub_mode = other.engine_stub_mode;
        self.sprints_max = other.sprints_max;
        self.target_branch = other.target_branch.clone();
    }

    /// Generate default swarm.toml content.
    pub fn default_toml() -> String {
        format!(
            r#"# Swarm configuration

[agents]
max_count = 3
tasks_per_agent = 2
timeout = {}  # seconds (60 minutes)

[files]
tasks = ".swarm-hug/default/tasks.md"
chat = ".swarm-hug/default/chat.md"
log_dir = ".swarm-hug/default/loop"

[engine]
type = "claude"
stub_mode = false

[sprints]
max = 0

"#,
            DEFAULT_AGENT_TIMEOUT_SECS
        )
    }

    /// Get the effective engine type (considering stub_mode).
    /// Get the primary engine type (first in list, considering stub_mode).
    /// Use this for deterministic operations like PRD conversion.
    pub fn effective_engine(&self) -> EngineType {
        if self.engine_stub_mode {
            EngineType::Stub
        } else {
            self.engine_types.first().copied().unwrap_or(EngineType::Claude)
        }
    }

    /// Select a random engine from the configured list.
    /// Use this for agent execution to enable weighted random selection.
    /// If stub_mode is enabled, always returns Stub.
    pub fn select_random_engine(&self) -> EngineType {
        if self.engine_stub_mode {
            return EngineType::Stub;
        }
        if self.engine_types.is_empty() {
            return EngineType::Claude;
        }
        if self.engine_types.len() == 1 {
            return self.engine_types[0];
        }
        // Use thread_rng for random selection
        use rand::seq::SliceRandom;
        *self.engine_types.choose(&mut rand::thread_rng()).unwrap()
    }

    /// Get a display string for the configured engines.
    /// Shows all engines if multiple are configured.
    pub fn engines_display(&self) -> String {
        if self.engine_stub_mode {
            return "stub".to_string();
        }
        EngineType::list_to_string(&self.engine_types)
    }
}

pub(crate) fn detect_target_branch() -> Option<String> {
    detect_target_branch_in(None)
}

pub(crate) fn detect_target_branch_in(repo_root: Option<&Path>) -> Option<String> {
    if git_branch_exists(repo_root, "main") {
        return Some("main".to_string());
    }
    if git_branch_exists(repo_root, "master") {
        return Some("master".to_string());
    }
    git_current_branch(repo_root)
}

fn git_branch_exists(repo_root: Option<&Path>, branch: &str) -> bool {
    let mut cmd = Command::new("git");
    if let Some(root) = repo_root {
        cmd.arg("-C").arg(root);
    }
    let ref_name = format!("refs/heads/{}", branch);
    cmd.args(["show-ref", "--verify", "--quiet", &ref_name]);
    match cmd.output() {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

fn git_current_branch(repo_root: Option<&Path>) -> Option<String> {
    let mut cmd = Command::new("git");
    if let Some(root) = repo_root {
        cmd.arg("-C").arg(root);
    }
    cmd.args(["rev-parse", "--abbrev-ref", "HEAD"]);
    let output = cmd.output().ok()?;
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    use super::detect_target_branch_in;

    fn run_git(repo: &Path, args: &[&str]) -> Output {
        let output = Command::new("git")
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
        output
    }

    fn init_repo(repo: &Path) {
        run_git(repo, &["init"]);
        run_git(repo, &["config", "user.name", "Swarm Test"]);
        run_git(repo, &["config", "user.email", "swarm-test@example.com"]);
        fs::write(repo.join("README.md"), "init").expect("write README");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "init"]);
    }

    fn init_repo_on_branch(repo: &Path, branch: &str) {
        init_repo(repo);
        run_git(repo, &["branch", "-M", branch]);
    }

    #[test]
    fn test_detect_target_branch_prefers_main() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo_on_branch(repo, "main");
        run_git(repo, &["branch", "master"]);

        let detected = detect_target_branch_in(Some(repo));
        assert_eq!(detected, Some("main".to_string()));
    }

    #[test]
    fn test_detect_target_branch_falls_back_to_master() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo_on_branch(repo, "master");

        let detected = detect_target_branch_in(Some(repo));
        assert_eq!(detected, Some("master".to_string()));
    }

    #[test]
    fn test_detect_target_branch_falls_back_to_current_branch() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo_on_branch(repo, "trunk");

        let detected = detect_target_branch_in(Some(repo));
        assert_eq!(detected, Some("trunk".to_string()));
    }
}
