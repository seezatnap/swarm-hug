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
    init              Initialize a new swarm project (config, TASKS.md, CHAT.md)
    run               Run sprints until done or max-sprints reached (default)
    sprint            Run exactly one sprint
    plan              Run sprint planning only (assign tasks)
    status            Show task counts and recent chat lines
    agents            List agent names and initials
    worktrees         List active git worktrees
    worktrees-branch  List worktree branches
    cleanup           Remove worktrees and branches
    merge             Merge agent branches to main
    tail              Tail CHAT.md (stream output)

OPTIONS:
    -h, --help              Show this help message
    -V, --version           Show version
    -c, --config <PATH>     Path to config file (default: swarm.toml)
    --max-agents <N>        Maximum number of agents to spawn
    --tasks-per-agent <N>   Tasks to assign per agent per sprint
    --tasks-file <PATH>     Path to tasks file (default: TASKS.md)
    --chat-file <PATH>      Path to chat file (default: CHAT.md)
    --log-dir <PATH>        Path to log directory (default: loop)
    --engine <TYPE>         Engine type: claude, codex, stub
    --stub                  Enable stub mode for testing
    --max-sprints <N>       Maximum sprints to run (0 = unlimited)
    --no-tail               Don't tail CHAT.md during run

EXAMPLES:
    swarm init              Create default config and files
    swarm run               Run sprints with default config
    swarm --stub --max-sprints 1 run
                            Run one sprint with stub engine (for testing)
    swarm status            Show current task status
    swarm agents            List available agents
"#
    );
}

/// Initialize a new swarm project.
fn cmd_init(config: &Config) -> Result<(), String> {
    println!("Initializing swarm project...");

    // Create config file if it doesn't exist
    if !Path::new("swarm.toml").exists() {
        fs::write("swarm.toml", Config::default_toml())
            .map_err(|e| format!("failed to create swarm.toml: {}", e))?;
        println!("  Created swarm.toml");
    } else {
        println!("  swarm.toml already exists");
    }

    // Create TASKS.md if it doesn't exist
    if !Path::new(&config.files_tasks).exists() {
        let default_tasks = "# Tasks\n\n- [ ] Add your tasks here\n";
        fs::write(&config.files_tasks, default_tasks)
            .map_err(|e| format!("failed to create {}: {}", config.files_tasks, e))?;
        println!("  Created {}", config.files_tasks);
    } else {
        println!("  {} already exists", config.files_tasks);
    }

    // Create CHAT.md if it doesn't exist
    if !Path::new(&config.files_chat).exists() {
        fs::write(&config.files_chat, "")
            .map_err(|e| format!("failed to create {}: {}", config.files_chat, e))?;
        println!("  Created {}", config.files_chat);
    } else {
        println!("  {} already exists", config.files_chat);
    }

    // Create log directory
    if !Path::new(&config.files_log_dir).exists() {
        fs::create_dir_all(&config.files_log_dir)
            .map_err(|e| format!("failed to create {}: {}", config.files_log_dir, e))?;
        println!("  Created {}/", config.files_log_dir);
    } else {
        println!("  {}/ already exists", config.files_log_dir);
    }

    println!("\nSwarm project initialized. Edit {} to add tasks.", config.files_tasks);
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
fn cmd_worktrees(_config: &Config) -> Result<(), String> {
    println!("Git Worktrees:");
    // TODO: Implement worktree listing
    println!("  (not yet implemented)");
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
fn cmd_cleanup(_config: &Config) -> Result<(), String> {
    println!("Cleaning up worktrees and branches...");
    worktree::cleanup_worktrees(Path::new("."))
        .map_err(|e| format!("cleanup failed: {}", e))?;
    println!("  Worktrees removed");
    Ok(())
}

/// Merge agent branches.
fn cmd_merge(_config: &Config) -> Result<(), String> {
    println!("Merging agent branches...");
    // TODO: Implement merge
    println!("  (not yet implemented)");
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
    worktree::create_worktrees(Path::new("."), &assignments)
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
