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
    /// Agent timeout in seconds.
    pub agent_timeout: Option<u64>,
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
    /// Disable TUI mode (use plain text output).
    pub no_tui: bool,
    /// Show help.
    pub help: bool,
    /// Show version.
    pub version: bool,
    /// Project name for multi-project mode.
    pub project: Option<String>,
    /// Project name for project-specific subcommands (positional arg).
    pub project_arg: Option<String>,
    /// Email for set-email command (positional arg).
    pub email_arg: Option<String>,
    /// Path to PRD file for project init --with-prd.
    pub prd_file_arg: Option<String>,
}

/// Swarm subcommands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Initialize a new swarm project.
    Init,
    /// Run sprints until done or max reached.
    Run,
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
    /// List all projects and their assigned agents.
    Projects,
    /// Initialize a new project (use with project name argument).
    ProjectInit,
    /// Copy embedded prompts to .swarm-hug/prompts for customization.
    CustomizePrompts,
    /// Set the co-author email for commits.
    SetEmail,
}

impl Command {
    /// Parse command from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "init" => Some(Self::Init),
            "run" => Some(Self::Run),
            "plan" => Some(Self::Plan),
            "status" => Some(Self::Status),
            "agents" => Some(Self::Agents),
            "worktrees" => Some(Self::Worktrees),
            "worktrees-branch" => Some(Self::WorktreesBranch),
            "cleanup" => Some(Self::Cleanup),
            "projects" => Some(Self::Projects),
            "project" => Some(Self::ProjectInit),
            "customize-prompts" => Some(Self::CustomizePrompts),
            "set-email" => Some(Self::SetEmail),
            _ => None,
        }
    }
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
            "-p" | "--project" => cli.project = args.next(),
            "--max-agents" => cli.max_agents = args.next().and_then(|s| s.parse().ok()),
            "--tasks-per-agent" => cli.tasks_per_agent = args.next().and_then(|s| s.parse().ok()),
            "--agent-timeout" => cli.agent_timeout = args.next().and_then(|s| s.parse().ok()),
            "--tasks-file" => cli.tasks_file = args.next(),
            "--chat-file" => cli.chat_file = args.next(),
            "--log-dir" => cli.log_dir = args.next(),
            "--engine" => cli.engine = args.next(),
            "--stub" => cli.stub = true,
            "--max-sprints" => cli.max_sprints = args.next().and_then(|s| s.parse().ok()),
            "--no-tail" => cli.no_tail = true,
            "--no-tui" => cli.no_tui = true,
            "--with-prd" => cli.prd_file_arg = args.next(),
            _ if !arg.starts_with('-') && cli.command.is_none() => {
                cli.command = Command::from_str(&arg);
                // For "project init <name>", capture the next arg as project_arg
                if cli.command == Some(Command::ProjectInit) {
                    // Check if next arg is "init" (project init <name> format)
                    if let Some(next) = args.peek() {
                        if next == "init" {
                            args.next(); // consume "init"
                            cli.project_arg = args.next(); // project name
                        } else if !next.starts_with('-') {
                            // Just "project <name>" - treat as project init
                            cli.project_arg = args.next();
                        }
                    }
                }
                // For "set-email <email>", capture the email argument
                if cli.command == Some(Command::SetEmail) {
                    if let Some(next) = args.peek() {
                        if !next.starts_with('-') {
                            cli.email_arg = args.next();
                        }
                    }
                }
            }
            _ => {} // Ignore unknown flags
        }
    }

    cli
}
