use std::collections::HashSet;
use std::env;
use std::process;

use swarm::config::{self, Command, Config};
use swarm::shutdown;
use swarm::team::Assignments;

mod commands;
mod git;
mod output;
mod runner;
mod tail;

use commands::{
    cmd_agents, cmd_cleanup, cmd_customize_prompts, cmd_init, cmd_plan, cmd_project_init,
    cmd_projects, cmd_run, cmd_run_tui, cmd_set_email, cmd_sprint, cmd_status, cmd_worktrees,
    cmd_worktrees_branch,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = env::args().collect();
    let cli = config::parse_args(args);

    if cli.help {
        print_help();
        return;
    }

    if cli.version {
        println!("swarm {}", VERSION);
        return;
    }

    let config = Config::load(&cli);

    // Default command is Run if none specified
    let command = cli.command.clone().unwrap_or(Command::Run);

    // Register Ctrl+C handler for commands that run sprints
    if matches!(command, Command::Run | Command::Sprint) {
        if let Err(e) = shutdown::register_handler() {
            eprintln!("warning: {}", e);
        }
    }

    let result = match command {
        Command::Init => cmd_init(&config),
        Command::Run => {
            if cli.no_tui {
                cmd_run(&config)
            } else {
                cmd_run_tui(&config)
            }
        }
        Command::Sprint => cmd_sprint(&config),
        Command::Plan => cmd_plan(&config),
        Command::Status => cmd_status(&config),
        Command::Agents => cmd_agents(&config),
        Command::Worktrees => cmd_worktrees(&config),
        Command::WorktreesBranch => cmd_worktrees_branch(&config),
        Command::Cleanup => cmd_cleanup(&config),
        Command::Projects => cmd_projects(&config),
        Command::ProjectInit => cmd_project_init(&config, &cli),
        Command::CustomizePrompts => cmd_customize_prompts(),
        Command::SetEmail => cmd_set_email(&cli),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        process::exit(1);
    }
}

fn print_help() {
    println!(
        r#"swarm - multi-agent sprint-based orchestration system

USAGE:
    swarm [OPTIONS] [COMMAND]

COMMANDS:
    init                  Initialize a new swarm repo (creates .swarm-hug/)
    run                   Run sprints until done or max-sprints reached (default)
    sprint                Run exactly one sprint
    plan                  Run sprint planning only (assign tasks)
    status                Show task counts and recent chat lines
    agents                List agent names and initials
    projects              List all projects and their assigned agents
    project init <name>   Initialize a new project
                          Use --with-prd <file> to auto-generate tasks from a PRD
    worktrees             List active git worktrees
    worktrees-branch      List worktree branches
    cleanup               Remove worktrees and branches
    customize-prompts     Copy prompts to .swarm-hug/prompts/ for customization
    set-email <email>     Set co-author email for commits (stored in .swarm-hug/email.txt)

OPTIONS:
    -h, --help                Show this help message
    -V, --version             Show version
    -c, --config <PATH>       Path to config file [default: swarm.toml]
    -p, --project <NAME>      Project to operate on (uses .swarm-hug/<project>/)
    --max-agents <N>          Maximum number of agents to spawn [default: {max_agents}]
    --tasks-per-agent <N>     Tasks to assign per agent per sprint [default: {tasks_per_agent}]
    --agent-timeout <SECS>    Agent execution timeout in seconds [default: {timeout}]
    --tasks-file <PATH>       Path to tasks file [default: <project>/tasks.md]
    --chat-file <PATH>        Path to chat file [default: <project>/chat.md]
    --log-dir <PATH>          Path to log directory [default: <project>/loop/]
    --engine <TYPE>           Engine type: claude, codex, stub [default: claude]
    --stub                    Enable stub mode for testing [default: false]
    --max-sprints <N>         Maximum sprints to run (0 = unlimited) [default: 0]
    --no-tail                 Don't tail chat.md during run [default: false]
    --no-tui                  Disable TUI mode (use plain text output) [default: false]

MULTI-PROJECT MODE:
    All config and artifacts live in .swarm-hug/:
      .swarm-hug/assignments.toml       Agent-to-project assignments
      .swarm-hug/<project>/tasks.md     Project's task list
      .swarm-hug/<project>/chat.md      Project's chat log
      .swarm-hug/<project>/loop/        Project's agent logs
      .swarm-hug/<project>/worktrees/   Project's git worktrees

EXAMPLES:
    swarm init                            Initialize .swarm-hug/ structure
    swarm project init authentication     Create a new project
    swarm project init payments           Create another project
    swarm projects                        List all projects
    swarm --project authentication run    Run sprints for authentication project
    swarm -p payments status              Show status for payments project
"#,
        max_agents = 3,
        tasks_per_agent = 2,
        timeout = config::DEFAULT_AGENT_TIMEOUT_SECS,
    );
}

fn project_name_for_config(config: &Config) -> String {
    config.project.clone().unwrap_or_else(|| "default".to_string())
}

fn release_assignments_for_project(project_name: &str, initials: &[char]) -> Result<usize, String> {
    let mut assignments = Assignments::load()?;

    if initials.is_empty() {
        let released = assignments.project_agents(project_name).len();
        if released > 0 {
            assignments.release_project(project_name);
            assignments.save()?;
        }
        return Ok(released);
    }

    let mut released = 0usize;
    let mut seen = HashSet::new();

    for initial in initials {
        let upper = initial.to_ascii_uppercase();
        if !seen.insert(upper) {
            continue;
        }
        if assignments.get_project(upper) == Some(project_name) {
            assignments.release(upper);
            released += 1;
        }
    }

    if released > 0 {
        assignments.save()?;
    }

    Ok(released)
}
