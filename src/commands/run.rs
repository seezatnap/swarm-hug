use std::env;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use swarm::chat;
use swarm::color::{self, emoji};
use swarm::config::Config;
use swarm::shutdown;

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
    if should_reset_chat() {
        chat::write_boot_message(&config.files_chat)
            .map_err(|e| format!("failed to write boot message: {}", e))?;
    }

    let mut tail_stop: Option<Arc<AtomicBool>> = None;
    let mut tail_handle: Option<thread::JoinHandle<()>> = None;

    // Only tail if SWARM_NO_TAIL is not set (TUI subprocess sets this)
    if !should_skip_tail() {
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

    // Clear chat.md before the TUI starts so we preserve the full session history in one run.
    if should_reset_chat() {
        chat::write_boot_message(&config.files_chat)
            .map_err(|e| format!("failed to write boot message: {}", e))?;
    }

    // Build command-line args to re-run swarm with --no-tui (plain text mode)
    // Note: SWARM_NO_TAIL env var is set by run_tui_with_subprocess to disable tailing
    let mut args: Vec<String> = Vec::new();
    args.push("run".to_string());
    args.push("--no-tui".to_string());  // Subprocess uses plain text mode

    if let Some(ref project) = config.project {
        args.push("--project".to_string());
        args.push(project.clone());
    }
    if let Some(ref target_branch) = config.target_branch {
        args.push("--target-branch".to_string());
        args.push(target_branch.clone());
    }
    if let Some(relative_paths) = config.worktree_relative_paths {
        if relative_paths {
            args.push("--relative-paths".to_string());
        } else {
            args.push("--no-relative-paths".to_string());
        }
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

    run_tui_with_subprocess(&config.files_chat, args, true)
        .map_err(|e| format!("TUI error: {}", e))
}

fn should_reset_chat() -> bool {
    env::var("SWARM_SKIP_CHAT_RESET").is_err()
}

fn should_skip_tail() -> bool {
    env::var("SWARM_NO_TAIL").is_ok()
}

#[cfg(test)]
mod tests {
    use super::should_reset_chat;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn should_reset_chat_defaults_true() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("SWARM_SKIP_CHAT_RESET");
        assert!(should_reset_chat());
    }

    #[test]
    fn should_reset_chat_skips_when_env_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("SWARM_SKIP_CHAT_RESET", "1");
        assert!(!should_reset_chat());
        std::env::remove_var("SWARM_SKIP_CHAT_RESET");
    }
}
