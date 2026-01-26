use std::time::Duration;

use swarm::color::{self, emoji};
use swarm::config;

/// Print a banner for starting a sprint.
pub(crate) fn print_sprint_start_banner(team_name: &str, sprint_number: usize) {
    println!();
    println!("=== {} {}: {} Sprint {} ===",
             emoji::ROCKET,
             color::label("STARTING SPRINT"),
             color::info(team_name),
             color::number(sprint_number));
    println!();
}

/// Print a team status banner after sprint completion.
pub(crate) fn print_team_status_banner(
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

pub(crate) fn print_help() {
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

#[cfg(test)]
mod tests {
    use super::format_duration;
    use std::time::Duration;

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
