use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::thread;
use std::time::Duration;

use swarm::agent;
use swarm::color::{self, emoji};
use swarm::config::{self, Command, Config};
use swarm::shutdown;
use swarm::team::{self, Assignments};

mod commands;
mod runner;

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

fn commit_files(paths: &[&str], message: &str) -> Result<bool, String> {
    let existing: Vec<&str> = paths
        .iter()
        .copied()
        .filter(|p| !p.is_empty() && Path::new(p).exists())
        .collect();

    if existing.is_empty() {
        return Ok(false);
    }

    let mut add_args = vec!["add"];
    add_args.extend(existing);
    let add_result = process::Command::new("git").args(add_args).output();

    match add_result {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git add failed: {}", stderr));
        }
        Err(e) => return Err(format!("git add failed: {}", e)),
    }

    // Check if there are staged changes
    let diff_result = process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .output();

    let has_changes = match diff_result {
        Ok(output) => !output.status.success(), // exit code 1 means changes exist
        Err(_) => false,
    };

    if !has_changes {
        return Ok(false); // No changes to commit
    }

    // Commit the changes
    let commit_result = process::Command::new("git")
        .args(["commit", "-m", message])
        .env("GIT_AUTHOR_NAME", "Swarm ScrumMaster")
        .env("GIT_AUTHOR_EMAIL", "swarm@local")
        .env("GIT_COMMITTER_NAME", "Swarm ScrumMaster")
        .env("GIT_COMMITTER_EMAIL", "swarm@local")
        .output();

    match commit_result {
        Ok(output) if output.status.success() => Ok(true),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't fail if there's nothing to commit
            if stderr.contains("nothing to commit") {
                Ok(false)
            } else {
                Err(format!("git commit failed: {}", stderr))
            }
        }
        Err(e) => Err(format!("git commit failed: {}", e)),
    }
}

/// Commit task assignment changes to git.
///
/// # Arguments
/// * `tasks_file` - Path to the team's tasks.md file
/// * `sprint_history_file` - Path to the team's sprint-history.json file
/// * `team_name` - Formatted team name for commit message (e.g., "Greenfield")
/// * `sprint_number` - The historical sprint number for this team
fn commit_task_assignments(
    tasks_file: &str,
    sprint_history_file: &str,
    team_name: &str,
    sprint_number: usize,
) -> Result<(), String> {
    let assignments_path = format!("{}/{}", team::SWARM_HUG_DIR, team::ASSIGNMENTS_FILE);
    let commit_msg = format!("{} Sprint {}: task assignments", team_name, sprint_number);
    if commit_files(
        &[tasks_file, sprint_history_file, assignments_path.as_str()],
        &commit_msg,
    )? {
        println!("  Committed task assignments to git.");
    }
    Ok(())
}

/// Commit sprint completion (updated tasks and released assignments).
///
/// # Arguments
/// * `tasks_file` - Path to the team's tasks.md file
/// * `team_name` - Formatted team name for commit message (e.g., "Greenfield")
/// * `sprint_number` - The historical sprint number for this team
fn commit_sprint_completion(
    tasks_file: &str,
    team_name: &str,
    sprint_number: usize,
) -> Result<(), String> {
    let assignments_path = format!("{}/{}", team::SWARM_HUG_DIR, team::ASSIGNMENTS_FILE);
    let commit_msg = format!("{} Sprint {}: completed", team_name, sprint_number);
    if commit_files(&[tasks_file, assignments_path.as_str()], &commit_msg)? {
        println!("  Committed sprint completion to git.");
    }
    Ok(())
}

/// Tail a file and stream appended content.
fn tail_follow(path: &str, allow_missing: bool, stop: Option<Arc<AtomicBool>>) -> Result<(), String> {
    let mut offset: u64 = 0;

    loop {
        if let Some(flag) = stop.as_ref() {
            if flag.load(Ordering::SeqCst) {
                break;
            }
        }

        if !Path::new(path).exists() {
            if allow_missing {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
            return Err(format!("{} not found", path));
        }

        let mut file = fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|e| format!("failed to open {}: {}", path, e))?;

        let len = file
            .metadata()
            .map_err(|e| format!("failed to stat {}: {}", path, e))?
            .len();
        if len < offset {
            offset = 0;
        }

        file.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("failed to seek {}: {}", path, e))?;

        let mut buffer = String::new();
        let bytes = file
            .read_to_string(&mut buffer)
            .map_err(|e| format!("failed to read {}: {}", path, e))?;

        if bytes > 0 {
            // Colorize each line of the chat output
            for line in buffer.lines() {
                println!("{}", color::chat_line(line));
            }
            let _ = io::stdout().flush();
            offset += bytes as u64;
        }

        thread::sleep(Duration::from_millis(200));
    }

    Ok(())
}

/// Print a banner for starting a sprint.
fn print_sprint_start_banner(team_name: &str, sprint_number: usize) {
    println!();
    println!("=== {} {}: {} Sprint {} ===",
             emoji::ROCKET,
             color::label("STARTING SPRINT"),
             color::info(team_name),
             color::number(sprint_number));
    println!();
}

/// Print a team status banner after sprint completion.
fn print_team_status_banner(
    team_name: &str,
    sprint_number: usize,
    completed_this_sprint: usize,
    failed_this_sprint: usize,
    remaining_tasks: usize,
    total_tasks: usize,
    task_durations: &[Duration],
    max_sprints: usize,
    agent_count: usize,
) {
    println!();
    println!("=== {} {} ===", emoji::SPARKLES, color::label("TEAM STATUS"));
    println!();
    println!("  {} Team: {}", emoji::TEAM, color::info(team_name));
    println!("  {} Sprint: {}", emoji::NUMBER, color::number(sprint_number));
    println!();
    println!("  {} {}: {}", emoji::CHECK, color::completed("Completed this sprint"), color::number(completed_this_sprint));
    println!("  {} {}: {}", emoji::CROSS, color::failed("Failed this sprint"), color::number(failed_this_sprint));
    println!("  {} Remaining tasks: {}", emoji::TASK, color::number(remaining_tasks));
    println!("  {} Total tasks: {}", emoji::PACKAGE, color::number(total_tasks));
    println!();

    // Calculate timing stats
    if !task_durations.is_empty() {
        let total_secs: f64 = task_durations.iter().map(|d| d.as_secs_f64()).sum();
        let avg_secs = total_secs / task_durations.len() as f64;
        let avg_duration = Duration::from_secs_f64(avg_secs);

        println!("  {} {}:", emoji::CLOCK, color::label("Agent Performance"));
        println!("     Tasks completed: {}", color::number(task_durations.len()));
        println!("     Avg task duration: {}", color::info(&format_duration(avg_duration)));

        // Estimate time remaining (accounting for parallel agents)
        if remaining_tasks > 0 && agent_count > 0 {
            // Use min of: remaining tasks OR (max_sprints * tasks_per_sprint) if max_sprints is set
            let implied_remaining = if max_sprints > 0 {
                // Rough estimate: assume similar task count per sprint
                let tasks_this_sprint = completed_this_sprint + failed_this_sprint;
                let sprints_remaining = max_sprints.saturating_sub(1); // current sprint counts as 1
                let implied = sprints_remaining * tasks_this_sprint.max(1);
                remaining_tasks.min(implied.max(remaining_tasks))
            } else {
                remaining_tasks
            };

            // Divide by agent count since agents work in parallel
            let estimated_secs = (avg_secs * implied_remaining as f64) / agent_count as f64;
            let estimated_duration = Duration::from_secs_f64(estimated_secs);
            println!("     {} Est. time remaining: {} ({} tasks, {} agents)",
                     emoji::HOURGLASS,
                     color::info(&format_duration(estimated_duration)),
                     color::number(implied_remaining),
                     color::number(agent_count));
        }
    }
    println!();
    println!("==========================");
    println!();
}

/// Format a duration in human-readable form.
fn format_duration(d: Duration) -> String {
    let total_secs = d.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

/// Get the current git commit hash.
fn get_current_commit() -> Option<String> {
    let output = process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get git log between two commits (messages and stats, no diffs).
fn get_git_log_range(from: &str, to: &str) -> Result<String, String> {
    let range = format!("{}..{}", from, to);
    let output = process::Command::new("git")
        .args(["log", "--stat", &range])
        .output()
        .map_err(|e| format!("failed to run git log: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        // If range is invalid (no commits), return empty string
        Ok(String::new())
    }
}

/// Commit an agent's work in their worktree.
/// Each agent makes one commit per task (enforces one task = one commit rule).
fn commit_agent_work(worktree_path: &Path, agent_name: &str, task_description: &str) -> Result<(), String> {
    // Stage all changes in the worktree
    let add_result = process::Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["add", "-A"])
        .output();

    match add_result {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If nothing to add, that's okay
            if !stderr.contains("Nothing specified") {
                return Err(format!("git add failed in worktree: {}", stderr));
            }
        }
        Err(e) => return Err(format!("git add failed: {}", e)),
    }

    // Check if there are staged changes
    let diff_result = process::Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["diff", "--cached", "--quiet"])
        .output();

    let has_changes = match diff_result {
        Ok(output) => !output.status.success(), // exit code 1 means changes exist
        Err(_) => false,
    };

    if !has_changes {
        return Ok(()); // No changes to commit
    }

    // Commit with agent attribution
    let commit_msg = format!("{}: {}", agent_name, task_description);
    let initial = agent::initial_from_name(agent_name).unwrap_or('?');
    let commit_result = process::Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["commit", "-m", &commit_msg])
        .env("GIT_AUTHOR_NAME", format!("Agent {}", agent_name))
        .env("GIT_AUTHOR_EMAIL", format!("agent-{}@swarm.local", initial))
        .env("GIT_COMMITTER_NAME", format!("Agent {}", agent_name))
        .env("GIT_COMMITTER_EMAIL", format!("agent-{}@swarm.local", initial))
        .output();

    match commit_result {
        Ok(output) if output.status.success() => {
            println!("  {} committed: {}", agent_name, task_description);
            Ok(())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't fail if there's nothing to commit
            if stderr.contains("nothing to commit") {
                Ok(())
            } else {
                Err(format!("git commit failed: {}", stderr))
            }
        }
        Err(e) => Err(format!("git commit failed: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_seconds_only() {
        let d = Duration::from_secs(45);
        assert_eq!(format_duration(d), "45s");
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        let d = Duration::from_secs(125); // 2m 5s
        assert_eq!(format_duration(d), "2m 5s");
    }

    #[test]
    fn test_format_duration_hours_minutes_seconds() {
        let d = Duration::from_secs(3725); // 1h 2m 5s
        assert_eq!(format_duration(d), "1h 2m 5s");
    }

    #[test]
    fn test_format_duration_zero() {
        let d = Duration::from_secs(0);
        assert_eq!(format_duration(d), "0s");
    }

    #[test]
    fn test_format_duration_exact_minute() {
        let d = Duration::from_secs(60);
        assert_eq!(format_duration(d), "1m 0s");
    }

    #[test]
    fn test_format_duration_exact_hour() {
        let d = Duration::from_secs(3600);
        assert_eq!(format_duration(d), "1h 0m 0s");
    }
}
