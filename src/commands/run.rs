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
use swarm::run_hash;
use swarm::shutdown;
use swarm::team;

use crate::runner::run_sprint;
use crate::tail::tail_follow;

/// Run sprints until done or max-sprints reached.
/// Maximum consecutive sprints where all tasks fail before stopping.
const MAX_CONSECUTIVE_FAILURES: usize = 3;

pub fn cmd_run(config: &Config) -> Result<(), String> {
    team::init_root()?;
    println!(
        "{} {} (max_sprints={}, engine={})...",
        emoji::ROCKET,
        color::label("Running swarm"),
        color::number(if config.sprints_max == 0 {
            "unlimited".to_string()
        } else {
            config.sprints_max.to_string()
        }),
        color::info(&config.engines_display())
    );

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
    let run_instance = run_hash::generate_run_hash();

    loop {
        sprint_number += 1;

        // Check for shutdown request before starting new sprint
        if shutdown::requested() {
            println!(
                "{} Shutdown requested, not starting new sprint.",
                emoji::STOP
            );
            interrupted = true;
            break;
        }

        // Check sprint limit
        if config.sprints_max > 0 && sprint_number > config.sprints_max {
            println!("Reached max sprints ({}), stopping.", config.sprints_max);
            break;
        }

        // Run one sprint (may return early if shutdown requested)
        let result = run_sprint(config, sprint_number, &run_instance);

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
                println!(
                    "{} {}: {} consecutive sprints with all tasks failing.",
                    emoji::WARNING,
                    color::warning("WARNING"),
                    color::failed(&consecutive_failures.to_string())
                );
                println!("   This usually indicates a configuration or authentication issue.");
                println!("   Please check:");
                println!(
                    "     - CLI authentication (run 'claude' or 'codex login' to authenticate)"
                );
                println!("     - Engine configuration (--engine flag or swarm.toml)");
                println!("     - File permissions in worktrees directory");
                println!();
                println!(
                    "{} Stopping to prevent further failed sprints.",
                    emoji::STOP
                );
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

    team::init_root()?;

    // Clear chat.md before the TUI starts so we preserve the full session history in one run.
    if should_reset_chat() {
        chat::write_boot_message(&config.files_chat)
            .map_err(|e| format!("failed to write boot message: {}", e))?;
    }

    let args = build_tui_subprocess_args(config);

    run_tui_with_subprocess(&config.files_chat, args, true).map_err(|e| format!("TUI error: {}", e))
}

/// Build command-line args to re-run swarm as a --no-tui subprocess.
///
/// SWARM_NO_TAIL env var is set by run_tui_with_subprocess to disable tailing.
fn build_tui_subprocess_args(config: &Config) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    args.push("run".to_string());
    args.push("--no-tui".to_string());

    if let Some(ref project) = config.project {
        args.push("--project".to_string());
        args.push(project.clone());
    }
    if let Some(ref source_branch) = config.source_branch {
        args.push("--source-branch".to_string());
        args.push(source_branch.clone());
    }
    if let Some(ref target_branch) = config.target_branch {
        args.push("--target-branch".to_string());
        args.push(target_branch.clone());
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

    args
}

fn should_reset_chat() -> bool {
    env::var("SWARM_SKIP_CHAT_RESET").is_err()
}

fn should_skip_tail() -> bool {
    env::var("SWARM_NO_TAIL").is_ok()
}

#[cfg(test)]
mod tests {
    use super::{build_tui_subprocess_args, should_reset_chat};
    use std::sync::Mutex;
    use swarm::config::Config;

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

    /// Helper: find the value following a flag in the args list.
    fn flag_value(args: &[String], flag: &str) -> Option<String> {
        args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
    }

    /// Helper: check whether a flag is present at all.
    fn has_flag(args: &[String], flag: &str) -> bool {
        args.iter().any(|a| a == flag)
    }

    #[test]
    fn tui_args_pass_source_branch_when_set() {
        let mut config = Config::default();
        config.source_branch = Some("feature-x".to_string());
        config.target_branch = Some("main".to_string());
        config.target_branch_explicit = true;

        let args = build_tui_subprocess_args(&config);

        assert_eq!(
            flag_value(&args, "--source-branch"),
            Some("feature-x".to_string())
        );
        assert_eq!(
            flag_value(&args, "--target-branch"),
            Some("main".to_string())
        );
    }

    #[test]
    fn tui_args_omit_source_branch_when_none() {
        let mut config = Config::default();
        config.source_branch = None;
        config.target_branch = None;

        let args = build_tui_subprocess_args(&config);

        assert!(!has_flag(&args, "--source-branch"));
        assert!(!has_flag(&args, "--target-branch"));
    }

    #[test]
    fn tui_args_source_only_sets_source_without_target() {
        let mut config = Config::default();
        config.source_branch = Some("develop".to_string());
        config.target_branch = None;

        let args = build_tui_subprocess_args(&config);

        assert_eq!(
            flag_value(&args, "--source-branch"),
            Some("develop".to_string())
        );
        assert!(!has_flag(&args, "--target-branch"));
    }

    #[test]
    fn tui_args_both_branches_passed_through() {
        let mut config = Config::default();
        config.source_branch = Some("main".to_string());
        config.target_branch = Some("feature-1".to_string());
        config.target_branch_explicit = true;

        let args = build_tui_subprocess_args(&config);

        assert_eq!(
            flag_value(&args, "--source-branch"),
            Some("main".to_string())
        );
        assert_eq!(
            flag_value(&args, "--target-branch"),
            Some("feature-1".to_string())
        );
    }

    #[test]
    fn tui_args_source_before_target() {
        let mut config = Config::default();
        config.source_branch = Some("main".to_string());
        config.target_branch = Some("feature-1".to_string());
        config.target_branch_explicit = true;

        let args = build_tui_subprocess_args(&config);

        let source_pos = args.iter().position(|a| a == "--source-branch").unwrap();
        let target_pos = args.iter().position(|a| a == "--target-branch").unwrap();
        assert!(
            source_pos < target_pos,
            "--source-branch should appear before --target-branch"
        );
    }

    #[test]
    fn tui_args_include_target_even_when_not_explicit() {
        let mut config = Config::default();
        config.source_branch = Some("main".to_string());
        config.target_branch = Some("main".to_string());
        config.target_branch_explicit = false;

        let args = build_tui_subprocess_args(&config);

        assert_eq!(
            flag_value(&args, "--source-branch"),
            Some("main".to_string())
        );
        assert_eq!(
            flag_value(&args, "--target-branch"),
            Some("main".to_string())
        );
    }

    #[test]
    fn tui_args_include_derived_target_branch_when_not_explicit() {
        let mut config = Config::default();
        config.source_branch = Some("develop".to_string());
        config.target_branch = Some("develop".to_string());
        config.target_branch_explicit = false;

        let args = build_tui_subprocess_args(&config);

        assert_eq!(
            flag_value(&args, "--source-branch"),
            Some("develop".to_string())
        );
        assert_eq!(
            flag_value(&args, "--target-branch"),
            Some("develop".to_string())
        );
    }

    #[test]
    fn tui_args_include_target_branch_when_not_explicit() {
        let mut config = Config::default();
        config.source_branch = Some("main".to_string());
        config.target_branch = Some("feature-1".to_string());
        config.target_branch_explicit = false;

        let args = build_tui_subprocess_args(&config);

        assert_eq!(
            flag_value(&args, "--source-branch"),
            Some("main".to_string())
        );
        assert_eq!(
            flag_value(&args, "--target-branch"),
            Some("feature-1".to_string())
        );
    }

    #[test]
    fn tui_args_always_include_no_tui() {
        let config = Config::default();
        let args = build_tui_subprocess_args(&config);

        assert_eq!(args[0], "run");
        assert_eq!(args[1], "--no-tui");
    }

    #[test]
    fn tui_args_include_all_standard_flags() {
        let mut config = Config::default();
        config.project = Some("my-proj".to_string());
        config.sprints_max = 5;
        config.agents_max_count = 4;
        config.agents_tasks_per_agent = 3;
        config.agent_timeout_secs = 1800;
        config.engine_stub_mode = true;

        let args = build_tui_subprocess_args(&config);

        assert_eq!(flag_value(&args, "--project"), Some("my-proj".to_string()));
        assert_eq!(flag_value(&args, "--max-sprints"), Some("5".to_string()));
        assert_eq!(flag_value(&args, "--max-agents"), Some("4".to_string()));
        assert_eq!(
            flag_value(&args, "--tasks-per-agent"),
            Some("3".to_string())
        );
        assert_eq!(
            flag_value(&args, "--agent-timeout"),
            Some("1800".to_string())
        );
        assert!(has_flag(&args, "--stub"));
    }
}
