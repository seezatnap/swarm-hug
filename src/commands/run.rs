use std::fs;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use swarm::agent;
use swarm::chat;
use swarm::color::{self, emoji};
use swarm::config::Config;
use swarm::shutdown;
use swarm::task::{TaskList, TaskStatus};
use swarm::team::{self, Assignments};

use crate::git::commit_task_assignments;
use crate::project_name_for_config;
use crate::tail::tail_follow;
use crate::runner::run_sprint;

/// Run sprints until done or max-sprints reached.
/// Maximum consecutive sprints where all tasks fail before stopping.
const MAX_CONSECUTIVE_FAILURES: usize = 3;

pub fn cmd_run(config: &Config) -> Result<(), String> {
    println!("{} {} (max_sprints={}, engine={})...",
             emoji::ROCKET,
             color::label("Running swarm"),
             color::number(if config.sprints_max == 0 { "unlimited".to_string() } else { config.sprints_max.to_string() }),
             color::info(&config.engines_display()));

    // Clear chat.md and write boot message before the first sprint
    chat::write_boot_message(&config.files_chat)
        .map_err(|e| format!("failed to write boot message: {}", e))?;

    let mut tail_stop: Option<Arc<AtomicBool>> = None;
    let mut tail_handle: Option<thread::JoinHandle<()>> = None;

    if !config.no_tail {
        let stop = Arc::new(AtomicBool::new(false));
        let path = config.files_chat.clone();
        let stop_clone = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            if let Err(e) = tail_follow(&path, true, Some(stop_clone)) {
                eprintln!("warning: tail stopped: {}", e);
            }
        });
        tail_stop = Some(stop);
        tail_handle = Some(handle);
    }

    let mut sprint_number = 0;
    let mut interrupted = false;
    let mut consecutive_failures = 0;

    loop {
        sprint_number += 1;

        // Check for shutdown request before starting new sprint
        if shutdown::requested() {
            println!("{} Shutdown requested, not starting new sprint.", emoji::STOP);
            interrupted = true;
            break;
        }

        // Check sprint limit
        if config.sprints_max > 0 && sprint_number > config.sprints_max {
            println!("Reached max sprints ({}), stopping.", config.sprints_max);
            break;
        }

        // Run one sprint (may return early if shutdown requested)
        let result = run_sprint(config, sprint_number);

        // Check if we were interrupted during the sprint
        if shutdown::requested() {
            println!("Sprint interrupted by shutdown request.");
            interrupted = true;
            // Still process the result to ensure cleanup happened
            if let Err(e) = result {
                eprintln!("Sprint error during shutdown: {}", e);
            }
            break;
        }

        let sprint_result = result?;

        if sprint_result.tasks_assigned == 0 {
            println!("{} No tasks to assign, sprints complete.", emoji::PARTY);
            break;
        }

        // Track consecutive failures (sprints where all tasks failed)
        if sprint_result.all_failed() {
            consecutive_failures += 1;
            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                println!();
                println!("{} {}: {} consecutive sprints with all tasks failing.",
                         emoji::WARNING,
                         color::warning("WARNING"),
                         color::failed(&consecutive_failures.to_string()));
                println!("   This usually indicates a configuration or authentication issue.");
                println!("   Please check:");
                println!("     - CLI authentication (run 'claude' or 'codex login' to authenticate)");
                println!("     - Engine configuration (--engine flag or swarm.toml)");
                println!("     - File permissions in worktrees directory");
                println!();
                println!("{} Stopping to prevent further failed sprints.", emoji::STOP);
                break;
            }
        } else {
            // Reset consecutive failure count on any successful task
            consecutive_failures = 0;
        }

        // Small delay between sprints
        thread::sleep(Duration::from_millis(100));
    }

    if interrupted {
        println!("{} Graceful shutdown complete.", emoji::WAVE);
    }

    if let Some(stop) = tail_stop {
        stop.store(true, Ordering::SeqCst);
    }
    if let Some(handle) = tail_handle {
        let _ = handle.join();
    }

    Ok(())
}

/// Run sprints with TUI interface.
///
/// Runs the sprint as a subprocess to avoid stdout corruption of the TUI.
pub fn cmd_run_tui(config: &Config) -> Result<(), String> {
    use swarm::tui::run_tui_with_subprocess;

    // Build command-line args to re-run swarm with --no-tui (plain text mode)
    let mut args: Vec<String> = Vec::new();
    args.push("run".to_string());
    args.push("--no-tui".to_string());  // Subprocess uses plain text mode
    args.push("--no-tail".to_string()); // TUI handles display

    if let Some(ref project) = config.project {
        args.push("--project".to_string());
        args.push(project.clone());
    }
    if config.sprints_max > 0 {
        args.push("--max-sprints".to_string());
        args.push(config.sprints_max.to_string());
    }
    args.push("--max-agents".to_string());
    args.push(config.agents_max_count.to_string());
    args.push("--tasks-per-agent".to_string());
    args.push(config.agents_tasks_per_agent.to_string());
    args.push("--agent-timeout".to_string());
    args.push(config.agent_timeout_secs.to_string());
    args.push("--engine".to_string());
    args.push(config.engines_display());
    if config.engine_stub_mode {
        args.push("--stub".to_string());
    }

    run_tui_with_subprocess(&config.files_chat, args)
        .map_err(|e| format!("TUI error: {}", e))
}

/// Run exactly one sprint.
pub fn cmd_sprint(config: &Config) -> Result<(), String> {
    println!("{} {} (engine={})...",
             emoji::SPRINT,
             color::label("Running single sprint"),
             color::info(&config.engines_display()));
    let result = run_sprint(config, 1)?;
    if result.all_failed() {
        println!();
        println!("{} {}: All {} task(s) failed in this sprint.",
                 emoji::WARNING,
                 color::warning("WARNING"),
                 color::failed(&result.tasks_failed.to_string()));
        println!("   This usually indicates a configuration or authentication issue.");
        println!("   Please check CLI authentication (run 'claude' or 'codex login').");
    }
    Ok(())
}

/// Run sprint planning only.
pub fn cmd_plan(config: &Config) -> Result<(), String> {
    println!("Running sprint planning...");

    // Load tasks
    let content = fs::read_to_string(&config.files_tasks)
        .map_err(|e| format!("failed to read {}: {}", config.files_tasks, e))?;
    let mut task_list = TaskList::parse(&content);

    // Determine how many agents to spawn based on assignable tasks
    let assignable = task_list.assignable_count();
    if assignable == 0 {
        println!("{} No assignable tasks found.", emoji::CHECK);
        return Ok(());
    }

    let team_name = project_name_for_config(config);
    let mut assignments_state = Assignments::load()?;

    let tasks_per_agent = config.agents_tasks_per_agent;
    let agents_needed = (assignable + tasks_per_agent - 1) / tasks_per_agent;
    let agent_cap = agents_needed.min(config.agents_max_count);
    let initials = assignments_state.available_for_team(&team_name, agent_cap);
    if initials.is_empty() {
        println!("No available agents for team '{}'.", team_name);
        return Ok(());
    }
    let agent_count = initials.len();
    let assigned = task_list.assign_sprint(&initials, tasks_per_agent);

    // Load sprint history and increment for this planning session
    let mut sprint_history = team::SprintHistory::load(&team_name)?;
    let historical_sprint = sprint_history.next_sprint();
    let formatted_team = sprint_history.formatted_team_name();
    sprint_history.save()?;

    // Write updated tasks
    fs::write(&config.files_tasks, task_list.to_string())
        .map_err(|e| format!("failed to write {}: {}", config.files_tasks, e))?;

    // Collect assignments for chat
    let assignments: Vec<(char, &str)> = task_list
        .tasks
        .iter()
        .filter_map(|t| {
            if let TaskStatus::Assigned(initial) = t.status {
                Some((initial, t.description.as_str()))
            } else {
                None
            }
        })
        .collect();

    let mut assigned_initials: Vec<char> = Vec::new();
    for (initial, _) in &assignments {
        if !assigned_initials.contains(initial) {
            assigned_initials.push(*initial);
        }
    }
    if !assigned_initials.is_empty() {
        for initial in &assigned_initials {
            if let Some(existing) = assignments_state.get_team(*initial) {
                if existing != team_name.as_str() {
                    return Err(format!(
                        "Agent {} is already assigned to team '{}'",
                        initial, existing
                    ));
                }
            } else {
                assignments_state.assign(*initial, &team_name)?;
            }
        }
        assignments_state.save()?;
    }

    // Write sprint plan to chat
    chat::write_sprint_plan(&config.files_chat, historical_sprint, &assignments)
        .map_err(|e| format!("failed to write chat: {}", e))?;

    // Commit assignment changes to git so worktrees can see them
    let sprint_history_path = team::Team::new(&team_name).sprint_history_path();
    commit_task_assignments(
        &config.files_tasks,
        sprint_history_path.to_str().unwrap_or(""),
        &formatted_team,
        historical_sprint,
    )?;

    println!("{} {} Sprint {}: assigned {} task(s) to {} agent(s).",
             emoji::SPRINT,
             color::info(&formatted_team),
             color::number(historical_sprint),
             color::number(assigned),
             color::number(agent_count));
    for (initial, desc) in &assignments {
        let name = agent::name_from_initial(*initial).unwrap_or("Unknown");
        println!("  {} {}: {}", emoji::ROBOT, color::agent(name), desc);
    }

    Ok(())
}
