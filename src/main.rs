use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process;
use std::thread;
use std::time::Duration;

use swarm::agent;
use swarm::chat;
use swarm::config::{self, Command, Config};
use swarm::engine;
use swarm::task::TaskList;
use swarm::team::{self, Assignments, Team};
use swarm::worktree;

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

    let result = match command {
        Command::Init => cmd_init(&config),
        Command::Run => cmd_run(&config),
        Command::Sprint => cmd_sprint(&config),
        Command::Plan => cmd_plan(&config),
        Command::Status => cmd_status(&config),
        Command::Agents => cmd_agents(&config),
        Command::Worktrees => cmd_worktrees(&config),
        Command::WorktreesBranch => cmd_worktrees_branch(&config),
        Command::Cleanup => cmd_cleanup(&config),
        Command::Merge => cmd_merge(&config),
        Command::Tail => cmd_tail(&config),
        Command::Teams => cmd_teams(&config),
        Command::TeamInit => cmd_team_init(&config, &cli),
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
    init              Initialize a new swarm project (creates .swarm-hug/)
    run               Run sprints until done or max-sprints reached (default)
    sprint            Run exactly one sprint
    plan              Run sprint planning only (assign tasks)
    status            Show task counts and recent chat lines
    agents            List agent names and initials
    teams             List all teams and their assigned agents
    team init <name>  Initialize a new team
    worktrees         List active git worktrees
    worktrees-branch  List worktree branches
    cleanup           Remove worktrees and branches
    merge             Merge agent branches to main
    tail              Tail chat.md (stream output)

OPTIONS:
    -h, --help              Show this help message
    -V, --version           Show version
    -c, --config <PATH>     Path to config file (default: swarm.toml)
    -t, --team <NAME>       Team to operate on (uses .swarm-hug/<team>/)
    --max-agents <N>        Maximum number of agents to spawn
    --tasks-per-agent <N>   Tasks to assign per agent per sprint
    --tasks-file <PATH>     Path to tasks file (default: tasks.md in team dir)
    --chat-file <PATH>      Path to chat file (default: chat.md in team dir)
    --log-dir <PATH>        Path to log directory (default: loop/ in team dir)
    --engine <TYPE>         Engine type: claude, codex, stub
    --stub                  Enable stub mode for testing
    --max-sprints <N>       Maximum sprints to run (0 = unlimited)
    --no-tail               Don't tail chat.md during run

MULTI-TEAM MODE:
    All config and artifacts live in .swarm-hug/:
      .swarm-hug/assignments.toml     Agent-to-team assignments
      .swarm-hug/<team>/tasks.md      Team's task list
      .swarm-hug/<team>/chat.md       Team's chat log
      .swarm-hug/<team>/loop/         Team's agent logs
      .swarm-hug/<team>/worktrees/    Team's git worktrees

EXAMPLES:
    swarm init                        Initialize .swarm-hug/ structure
    swarm team init authentication    Create a new team
    swarm team init payments          Create another team
    swarm teams                       List all teams
    swarm --team authentication run   Run sprints for authentication team
    swarm -t payments status          Show status for payments team
"#
    );
}

/// Initialize a new swarm project.
fn cmd_init(config: &Config) -> Result<(), String> {
    println!("Initializing swarm project...");

    // Create .swarm-hug root directory and assignments file
    team::init_root()?;
    println!("  Created .swarm-hug/");
    println!("  Created .swarm-hug/assignments.toml");

    // Create config file if it doesn't exist
    if !Path::new("swarm.toml").exists() {
        fs::write("swarm.toml", Config::default_toml())
            .map_err(|e| format!("failed to create swarm.toml: {}", e))?;
        println!("  Created swarm.toml");
    } else {
        println!("  swarm.toml already exists");
    }

    // If a team is specified, initialize that team's directory
    if let Some(ref team_name) = config.team {
        let team = Team::new(team_name);
        team.init()?;
        println!("  Created team: {}", team_name);
        println!("    - {}", team.tasks_path().display());
        println!("    - {}", team.chat_path().display());
        println!("    - {}", team.loop_dir().display());
        println!("    - {}", team.worktrees_dir().display());
    }

    println!("\nSwarm project initialized.");
    println!("  Use 'swarm team init <name>' to create teams.");
    println!("  Use 'swarm --team <name> run' to run sprints for a team.");
    Ok(())
}

/// Run sprints until done or max-sprints reached.
fn cmd_run(config: &Config) -> Result<(), String> {
    println!("Running swarm (max_sprints={}, engine={})...",
             if config.sprints_max == 0 { "unlimited".to_string() } else { config.sprints_max.to_string() },
             config.effective_engine().as_str());

    let mut sprint_number = 0;

    loop {
        sprint_number += 1;

        // Check sprint limit
        if config.sprints_max > 0 && sprint_number > config.sprints_max {
            println!("Reached max sprints ({}), stopping.", config.sprints_max);
            break;
        }

        // Run one sprint
        let tasks_assigned = run_sprint(config, sprint_number)?;

        if tasks_assigned == 0 {
            println!("No tasks to assign, sprints complete.");
            break;
        }

        // Small delay between sprints
        thread::sleep(Duration::from_millis(100));
    }

    Ok(())
}

/// Run exactly one sprint.
fn cmd_sprint(config: &Config) -> Result<(), String> {
    println!("Running single sprint (engine={})...", config.effective_engine().as_str());
    run_sprint(config, 1)?;
    Ok(())
}

/// Run sprint planning only.
fn cmd_plan(config: &Config) -> Result<(), String> {
    println!("Running sprint planning...");

    // Load tasks
    let content = fs::read_to_string(&config.files_tasks)
        .map_err(|e| format!("failed to read {}: {}", config.files_tasks, e))?;
    let mut task_list = TaskList::parse(&content);

    // Determine how many agents to spawn based on assignable tasks
    let assignable = task_list.assignable_count();
    if assignable == 0 {
        println!("No assignable tasks found.");
        return Ok(());
    }

    let tasks_per_agent = config.agents_tasks_per_agent;
    let agents_needed = (assignable + tasks_per_agent - 1) / tasks_per_agent;
    let agent_count = agents_needed.min(config.agents_max_count);

    let initials = agent::get_initials(agent_count);
    let assigned = task_list.assign_sprint(&initials, tasks_per_agent);

    // Write updated tasks
    fs::write(&config.files_tasks, task_list.to_string())
        .map_err(|e| format!("failed to write {}: {}", config.files_tasks, e))?;

    // Collect assignments for chat
    let assignments: Vec<(char, &str)> = task_list
        .tasks
        .iter()
        .filter_map(|t| {
            if let swarm::task::TaskStatus::Assigned(initial) = t.status {
                Some((initial, t.description.as_str()))
            } else {
                None
            }
        })
        .collect();

    // Write sprint plan to chat
    chat::write_sprint_plan(&config.files_chat, 1, &assignments)
        .map_err(|e| format!("failed to write chat: {}", e))?;

    println!("Assigned {} task(s) to {} agent(s).", assigned, agent_count);
    for (initial, desc) in &assignments {
        let name = agent::name_from_initial(*initial).unwrap_or("Unknown");
        println!("  {} ({}): {}", name, initial, desc);
    }

    Ok(())
}

/// Show task status.
fn cmd_status(config: &Config) -> Result<(), String> {
    // Load and parse tasks
    let content = fs::read_to_string(&config.files_tasks)
        .map_err(|e| format!("failed to read {}: {}", config.files_tasks, e))?;
    let task_list = TaskList::parse(&content);

    println!("Task Status ({}):", config.files_tasks);
    println!("  Unassigned: {}", task_list.unassigned_count());
    println!("  Assigned:   {}", task_list.assigned_count());
    println!("  Completed:  {}", task_list.completed_count());
    println!("  Assignable: {}", task_list.assignable_count());
    println!("  Total:      {}", task_list.tasks.len());

    // Show recent chat lines
    println!("\nRecent Chat ({}):", config.files_chat);
    if Path::new(&config.files_chat).exists() {
        match chat::read_recent(&config.files_chat, 5) {
            Ok(lines) => {
                if lines.is_empty() {
                    println!("  (no messages)");
                } else {
                    for line in lines {
                        println!("  {}", line);
                    }
                }
            }
            Err(e) => println!("  (error reading chat: {})", e),
        }
    } else {
        println!("  (file not found)");
    }

    Ok(())
}

/// List agent names and initials.
fn cmd_agents(_config: &Config) -> Result<(), String> {
    println!("Available Agents:");
    for (i, name) in agent::NAMES.iter().enumerate() {
        let initial = agent::INITIALS[i];
        println!("  {} - {}", initial, name);
    }
    Ok(())
}

/// List active worktrees.
fn cmd_worktrees(config: &Config) -> Result<(), String> {
    println!("Git Worktrees ({}):", config.files_worktrees_dir);
    let worktrees = worktree::list_worktrees(Path::new(&config.files_worktrees_dir))?;

    if worktrees.is_empty() {
        println!("  (no worktrees)");
    } else {
        for wt in &worktrees {
            println!("  {} ({}) - {}", wt.name, wt.initial, wt.path.display());
        }
    }
    Ok(())
}

/// List worktree branches.
fn cmd_worktrees_branch(_config: &Config) -> Result<(), String> {
    println!("Worktree Branches:");
    // TODO: Implement worktree branch listing
    println!("  (not yet implemented)");
    Ok(())
}

/// Clean up worktrees and branches.
fn cmd_cleanup(config: &Config) -> Result<(), String> {
    println!("Cleaning up worktrees and branches...");
    worktree::cleanup_worktrees_in(Path::new(&config.files_worktrees_dir))
        .map_err(|e| format!("cleanup failed: {}", e))?;
    println!("  Worktrees removed from {}", config.files_worktrees_dir);
    Ok(())
}

/// Merge agent branches.
fn cmd_merge(_config: &Config) -> Result<(), String> {
    println!("Merging agent branches...");
    // TODO: Implement merge
    println!("  (not yet implemented)");
    Ok(())
}

/// List all teams and their assigned agents.
fn cmd_teams(_config: &Config) -> Result<(), String> {
    if !team::root_exists() {
        println!("No .swarm-hug/ directory found. Run 'swarm init' first.");
        return Ok(());
    }

    let teams = team::list_teams()?;
    let assignments = Assignments::load()?;

    if teams.is_empty() {
        println!("No teams found. Use 'swarm team init <name>' to create one.");
        return Ok(());
    }

    println!("Teams:");
    for t in &teams {
        let agents = assignments.team_agents(&t.name);
        let agent_str = if agents.is_empty() {
            "(no agents assigned)".to_string()
        } else {
            agents
                .iter()
                .map(|&i| {
                    let name = agent::name_from_initial(i).unwrap_or("?");
                    format!("{} ({})", name, i)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!("  {} - {}", t.name, agent_str);
    }

    // Show available agents
    let available = assignments.next_available(5);
    if !available.is_empty() {
        println!("\nNext available agents:");
        for i in available {
            let name = agent::name_from_initial(i).unwrap_or("?");
            println!("  {} - {}", i, name);
        }
    }

    Ok(())
}

/// Initialize a new team.
fn cmd_team_init(_config: &Config, cli: &config::CliArgs) -> Result<(), String> {
    let team_name = cli.team_arg.as_ref()
        .ok_or("Usage: swarm team init <name>")?;

    // Validate team name (alphanumeric and hyphens only)
    if !team_name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err("Team name must contain only letters, numbers, hyphens, and underscores".to_string());
    }

    // Initialize root if needed
    team::init_root()?;

    let team = Team::new(team_name);
    if team.exists() {
        println!("Team '{}' already exists.", team_name);
        return Ok(());
    }

    team.init()?;
    println!("Created team: {}", team_name);
    println!("  Directory: {}", team.root.display());
    println!("  Tasks:     {}", team.tasks_path().display());
    println!("  Chat:      {}", team.chat_path().display());
    println!("  Logs:      {}", team.loop_dir().display());
    println!("  Worktrees: {}", team.worktrees_dir().display());
    println!("\nTo work on this team, use:");
    println!("  swarm --team {} run", team_name);
    println!("  swarm -t {} status", team_name);

    Ok(())
}

/// Tail CHAT.md.
fn cmd_tail(config: &Config) -> Result<(), String> {
    let path = &config.files_chat;

    if !Path::new(path).exists() {
        return Err(format!("{} not found", path));
    }

    println!("Tailing {}... (Ctrl+C to stop)", path);

    // Simple tail implementation - read and print new lines
    let file = fs::File::open(path)
        .map_err(|e| format!("failed to open {}: {}", path, e))?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        match line {
            Ok(l) => println!("{}", l),
            Err(e) => eprintln!("error reading line: {}", e),
        }
    }

    // In a real implementation, we'd watch for new content
    // For now, just print what's there
    Ok(())
}

/// Run a single sprint.
fn run_sprint(config: &Config, sprint_number: usize) -> Result<usize, String> {
    // Load tasks
    let content = fs::read_to_string(&config.files_tasks)
        .map_err(|e| format!("failed to read {}: {}", config.files_tasks, e))?;
    let mut task_list = TaskList::parse(&content);

    // Determine how many agents to spawn
    let assignable = task_list.assignable_count();
    if assignable == 0 {
        return Ok(0);
    }

    let tasks_per_agent = config.agents_tasks_per_agent;
    let agents_needed = (assignable + tasks_per_agent - 1) / tasks_per_agent;
    let agent_count = agents_needed.min(config.agents_max_count);

    let initials = agent::get_initials(agent_count);
    let assigned = task_list.assign_sprint(&initials, tasks_per_agent);

    if assigned == 0 {
        return Ok(0);
    }

    // Write updated tasks
    fs::write(&config.files_tasks, task_list.to_string())
        .map_err(|e| format!("failed to write {}: {}", config.files_tasks, e))?;

    // Collect assignments
    let assignments: Vec<(char, String)> = task_list
        .tasks
        .iter()
        .filter_map(|t| {
            if let swarm::task::TaskStatus::Assigned(initial) = t.status {
                Some((initial, t.description.clone()))
            } else {
                None
            }
        })
        .collect();

    // Write sprint plan to chat
    let assignments_ref: Vec<(char, &str)> = assignments
        .iter()
        .map(|(i, d)| (*i, d.as_str()))
        .collect();
    chat::write_sprint_plan(&config.files_chat, sprint_number, &assignments_ref)
        .map_err(|e| format!("failed to write chat: {}", e))?;

    println!("Sprint {}: assigned {} task(s) to {} agent(s)",
             sprint_number, assigned, agent_count);

    // Create worktrees for assigned agents (placeholder dirs for now).
    worktree::create_worktrees_in(Path::new(&config.files_worktrees_dir), &assignments)
        .map_err(|e| format!("failed to create worktrees: {}", e))?;

    // Create engine
    let engine = engine::create_engine(config.effective_engine(), &config.files_log_dir);

    // Execute tasks for each agent
    for (initial, description) in &assignments {
        let agent_name = agent::name_from_initial(*initial).unwrap_or("Unknown");

        // Write agent start to chat
        chat::write_message(&config.files_chat, agent_name, &format!("Starting: {}", description))
            .map_err(|e| format!("failed to write chat: {}", e))?;

        // Execute via engine
        let result = engine.execute(
            agent_name,
            description,
            Path::new("."),
            sprint_number,
        );

        if result.success {
            // Mark task as completed
            for task in &mut task_list.tasks {
                if let swarm::task::TaskStatus::Assigned(i) = task.status {
                    if i == *initial && task.description == *description {
                        task.complete(*initial);
                        break;
                    }
                }
            }

            chat::write_message(&config.files_chat, agent_name, &format!("Completed: {}", description))
                .map_err(|e| format!("failed to write chat: {}", e))?;
        } else {
            let error = result.error.unwrap_or_else(|| "unknown error".to_string());
            chat::write_message(&config.files_chat, agent_name, &format!("Failed: {} - {}", description, error))
                .map_err(|e| format!("failed to write chat: {}", e))?;
        }
    }

    // Write final task state
    fs::write(&config.files_tasks, task_list.to_string())
        .map_err(|e| format!("failed to write {}: {}", config.files_tasks, e))?;

    Ok(assigned)
}
