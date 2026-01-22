use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::thread;
use std::time::Duration;

use swarm::agent;
use swarm::chat;
use swarm::config::{self, Command, Config};
use swarm::engine;
use swarm::lifecycle::LifecycleTracker;
use swarm::log::{self, AgentLogger};
use swarm::planning;
use swarm::prompt;
use swarm::task::TaskList;
use swarm::team::{self, Assignments, Team};
use swarm::worktree::{self, Worktree};

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
        Command::CustomizePrompts => cmd_customize_prompts(),
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
    customize-prompts Copy prompts to .swarm-hug/prompts/ for customization

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

    // If a team is specified, initialize that team's directory
    if let Some(ref team_name) = config.team {
        let team = Team::new(team_name);
        team.init()?;
        println!("  Created team: {}", team_name);
        println!("    - {}", team.tasks_path().display());
        println!("    - {}", team.chat_path().display());
        println!("    - {}", team.loop_dir().display());
        println!("    - {}", team.worktrees_dir().display());
    } else {
        init_default_files(config)?;
    }

    println!("\nSwarm project initialized.");
    println!("  Use 'swarm team init <name>' to create teams.");
    println!("  Use 'swarm --team <name> run' to run sprints for a team.");
    Ok(())
}

fn init_default_files(config: &Config) -> Result<(), String> {
    let tasks_path = Path::new(&config.files_tasks);
    if !tasks_path.exists() {
        ensure_parent_dir(tasks_path)?;
        let default_tasks = "# Tasks\n\n- [ ] Add your tasks here\n";
        fs::write(tasks_path, default_tasks)
            .map_err(|e| format!("failed to create {}: {}", config.files_tasks, e))?;
        println!("  Created {}", config.files_tasks);
    } else {
        println!("  Task file already exists: {}", config.files_tasks);
    }

    let chat_path = Path::new(&config.files_chat);
    if !chat_path.exists() {
        ensure_parent_dir(chat_path)?;
        fs::write(chat_path, "")
            .map_err(|e| format!("failed to create {}: {}", config.files_chat, e))?;
        println!("  Created {}", config.files_chat);
    } else {
        println!("  Chat file already exists: {}", config.files_chat);
    }

    if config.files_log_dir.is_empty() {
        return Err("log dir path is empty".to_string());
    }

    fs::create_dir_all(&config.files_log_dir)
        .map_err(|e| format!("failed to create log dir {}: {}", config.files_log_dir, e))?;
    println!("  Created log directory: {}", config.files_log_dir);

    if config.files_worktrees_dir.is_empty() {
        return Err("worktrees dir path is empty".to_string());
    }

    fs::create_dir_all(&config.files_worktrees_dir)
        .map_err(|e| {
            format!(
                "failed to create worktrees dir {}: {}",
                config.files_worktrees_dir, e
            )
        })?;
    println!("  Created worktrees directory: {}", config.files_worktrees_dir);

    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directory {}: {}", parent.display(), e))?;
        }
    }
    Ok(())
}

fn team_name_for_config(config: &Config) -> String {
    config.team.clone().unwrap_or_else(|| "default".to_string())
}

fn release_assignments_for_team(team_name: &str, initials: &[char]) -> Result<usize, String> {
    let mut assignments = Assignments::load()?;

    if initials.is_empty() {
        let released = assignments.team_agents(team_name).len();
        if released > 0 {
            assignments.release_team(team_name);
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
        if assignments.get_team(upper) == Some(team_name) {
            assignments.release(upper);
            released += 1;
        }
    }

    if released > 0 {
        assignments.save()?;
    }

    Ok(released)
}

/// Run sprints until done or max-sprints reached.
fn cmd_run(config: &Config) -> Result<(), String> {
    println!("Running swarm (max_sprints={}, engine={})...",
             if config.sprints_max == 0 { "unlimited".to_string() } else { config.sprints_max.to_string() },
             config.effective_engine().as_str());

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

    if let Some(stop) = tail_stop {
        stop.store(true, Ordering::SeqCst);
    }
    if let Some(handle) = tail_handle {
        let _ = handle.join();
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

    let team_name = team_name_for_config(config);
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
    chat::write_sprint_plan(&config.files_chat, 1, &assignments)
        .map_err(|e| format!("failed to write chat: {}", e))?;

    // Commit assignment changes to git so worktrees can see them
    commit_task_assignments(&config.files_tasks, sprint_number_for_plan(1))?;

    println!("Assigned {} task(s) to {} agent(s).", assigned, agent_count);
    for (initial, desc) in &assignments {
        let name = agent::name_from_initial(*initial).unwrap_or("Unknown");
        println!("  {} ({}): {}", name, initial, desc);
    }

    Ok(())
}

/// Helper to generate sprint number string for plan command.
fn sprint_number_for_plan(_: usize) -> usize {
    1
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
fn commit_task_assignments(tasks_file: &str, sprint_number: usize) -> Result<(), String> {
    let assignments_path = format!("{}/{}", team::SWARM_HUG_DIR, team::ASSIGNMENTS_FILE);
    let commit_msg = format!("Sprint {}: task assignments", sprint_number);
    if commit_files(&[tasks_file, assignments_path.as_str()], &commit_msg)? {
        println!("  Committed task assignments to git.");
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
    println!("Agent Branches:");
    let branches = worktree::list_agent_branches()?;

    if branches.is_empty() {
        println!("  (no agent branches found)");
    } else {
        for b in &branches {
            let status = if b.exists { "active" } else { "missing" };
            let name = agent::name_from_initial(b.initial).unwrap_or("?");
            println!("  {} ({}) - {} [{}]", name, b.initial, b.branch, status);
        }
    }
    Ok(())
}

/// Clean up worktrees and branches.
fn cmd_cleanup(config: &Config) -> Result<(), String> {
    println!("Cleaning up worktrees and branches...");
    let team_name = team_name_for_config(config);
    let worktrees_dir = Path::new(&config.files_worktrees_dir);
    let mut errors: Vec<String> = Vec::new();

    // Get agents currently assigned to this team (before we release them)
    let team_agents: Vec<char> = match Assignments::load() {
        Ok(assignments) => assignments.team_agents(&team_name),
        Err(_) => Vec::new(),
    };

    // Also get agents from existing worktrees (in case assignments already released)
    let worktree_agents: Vec<char> = worktree::list_worktrees(worktrees_dir)
        .unwrap_or_default()
        .iter()
        .map(|wt| wt.initial)
        .collect();

    // Combine both lists (union)
    let mut agents_to_cleanup: Vec<char> = team_agents.clone();
    for initial in worktree_agents {
        if !agents_to_cleanup.contains(&initial) {
            agents_to_cleanup.push(initial);
        }
    }

    // Clean up worktrees in the team directory
    if let Err(e) = worktree::cleanup_worktrees_in(worktrees_dir) {
        errors.push(format!("worktree cleanup failed: {}", e));
    } else {
        println!("  Worktrees removed from {}", config.files_worktrees_dir);
    }

    // Delete branches only for this team's agents
    let mut deleted = 0usize;
    for initial in &agents_to_cleanup {
        match worktree::delete_agent_branch(*initial) {
            Ok(true) => {
                let name = agent::name_from_initial(*initial).unwrap_or("?");
                println!("  Deleted branch: agent/{}", name.to_lowercase());
                deleted += 1;
            }
            Ok(false) => {}
            Err(e) => {
                let name = agent::name_from_initial(*initial).unwrap_or("?");
                errors.push(format!("failed to delete branch for {}: {}", name, e));
            }
        }
    }

    // Also clean up team-specific scrummaster branch if it exists
    let scrummaster_branch = format!("agent/scrummaster-{}", team_name);
    if let Ok(true) = worktree::delete_branch(&scrummaster_branch) {
        println!("  Deleted branch: {}", scrummaster_branch);
        deleted += 1;
    }
    // Clean up legacy scrummaster branch too (from before this fix)
    if let Ok(true) = worktree::delete_branch("agent/scrummaster") {
        println!("  Deleted branch: agent/scrummaster (legacy)");
        deleted += 1;
    }

    if deleted > 0 {
        println!("  Deleted {} branch(es) total", deleted);
    }

    // Release agent assignments for this team
    match release_assignments_for_team(&team_name, &[]) {
        Ok(released) => {
            if released > 0 {
                println!("  Released {} agent assignment(s) for team {}", released, team_name);
            }
        }
        Err(e) => errors.push(format!("assignment release failed: {}", e)),
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Merge agent branches.
fn cmd_merge(config: &Config) -> Result<(), String> {
    println!("Merging agent branches...");
    let team_name = team_name_for_config(config);

    // Find all agent branches
    let branches = worktree::list_agent_branches()?;

    if branches.is_empty() {
        println!("  No agent branches found.");
        return Ok(());
    }

    // Get the target branch (current branch or main)
    let target = get_current_branch().unwrap_or_else(|| "main".to_string());
    println!("  Target branch: {}", target);

    let initials: Vec<char> = branches.iter().map(|b| b.initial).collect();
    let summary = worktree::merge_all_agent_branches(&initials, &target);

    // Report results
    if !summary.success.is_empty() {
        println!("\nSuccessful merges:");
        for initial in &summary.success {
            let name = agent::name_from_initial(*initial).unwrap_or("?");
            let branch = worktree::agent_branch_name(*initial).unwrap_or_default();
            println!("  {} ({}) - merged", name, initial);

            // Write to chat
            let msg = format!("Merged branch {} to {}", branch, target);
            if let Err(e) = chat::write_merge_status(&config.files_chat, name, true, &msg) {
                eprintln!("  warning: failed to write chat: {}", e);
            }
        }
    }

    if !summary.no_changes.is_empty() {
        println!("\nSkipped (no changes):");
        for initial in &summary.no_changes {
            let name = agent::name_from_initial(*initial).unwrap_or("?");
            println!("  {} ({}) - no changes", name, initial);
        }
    }

    if !summary.conflicts.is_empty() {
        println!("\nConflicts:");
        for (initial, files) in &summary.conflicts {
            let name = agent::name_from_initial(*initial).unwrap_or("?");
            println!("  {} ({}) - conflict in {} file(s):", name, initial, files.len());
            for f in files {
                println!("    - {}", f);
            }

            // Write to chat
            let files_str = format!("Conflicts in: {}", files.join(", "));
            if let Err(e) = chat::write_merge_status(
                &config.files_chat,
                name,
                false,
                &files_str,
            ) {
                eprintln!("  warning: failed to write chat: {}", e);
            }
        }
    }

    if !summary.errors.is_empty() {
        println!("\nErrors:");
        for (initial, err) in &summary.errors {
            let name = agent::name_from_initial(*initial).unwrap_or("?");
            println!("  {} ({}) - {}", name, initial, err);
        }
    }

    // Summary
    println!(
        "\nMerge summary: {} success, {} conflicts, {} skipped",
        summary.success_count(),
        summary.conflict_count(),
        summary.no_changes.len()
    );

    // Automatically clean up worktrees and branches for successful merges
    // Collect agents to clean up: successful merges and those with no changes
    let mut cleanup_initials: Vec<char> = summary.success.clone();
    cleanup_initials.extend(&summary.no_changes);

    if !cleanup_initials.is_empty() {
        println!("\nCleaning up merged worktrees...");
        let worktrees_dir = Path::new(&config.files_worktrees_dir);
        let cleanup_summary = worktree::cleanup_agent_worktrees(
            worktrees_dir,
            &cleanup_initials,
            true, // Also delete branches
        );

        if cleanup_summary.cleaned_count() > 0 {
            println!("  Cleaned up {} worktree(s) and branch(es)", cleanup_summary.cleaned_count());
        }

        if cleanup_summary.has_errors() {
            for (initial, err) in &cleanup_summary.errors {
                let name = agent::name_from_initial(*initial).unwrap_or("?");
                eprintln!("  warning: cleanup failed for {} ({}): {}", name, initial, err);
            }
        }
    }

    if !cleanup_initials.is_empty() {
        match release_assignments_for_team(&team_name, &cleanup_initials) {
            Ok(released) => {
                if released > 0 {
                    println!("Released {} agent assignment(s) for team {}", released, team_name);
                }
            }
            Err(e) => {
                eprintln!("warning: assignment release failed: {}", e);
            }
        }
    }

    if summary.has_conflicts() {
        Err("Some merges had conflicts".to_string())
    } else {
        Ok(())
    }
}

/// Get the current git branch.
fn get_current_branch() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
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

/// Copy embedded prompts to .swarm-hug/prompts/ for customization.
fn cmd_customize_prompts() -> Result<(), String> {
    let target_dir = Path::new(".swarm-hug/prompts");

    if target_dir.exists() {
        println!("Prompts directory already exists: {}", target_dir.display());
        println!("To reset to defaults, remove the directory first:");
        println!("  rm -rf .swarm-hug/prompts");
        return Ok(());
    }

    println!("Copying embedded prompts to {}...", target_dir.display());
    let created = prompt::copy_prompts_to(target_dir)?;

    println!("\nCreated {} prompt file(s):", created.len());
    for path in &created {
        println!("  {}", path.display());
    }

    println!("\nYou can now customize these prompts. They will be used instead of the built-in defaults.");
    println!("Available variables:");
    println!("  agent.md:        {{{{agent_name}}}}, {{{{task_description}}}}, {{{{agent_name_lower}}}}, {{{{agent_initial}}}}, {{{{task_short}}}}");
    println!("  scrum_master.md: {{{{to_assign}}}}, {{{{num_agents}}}}, {{{{tasks_per_agent}}}}, {{{{num_unassigned}}}}, {{{{agent_list}}}}, {{{{task_list}}}}");
    println!("  review.md:       {{{{git_log}}}}, {{{{tasks_content}}}}");

    Ok(())
}

/// Tail CHAT.md.
fn cmd_tail(config: &Config) -> Result<(), String> {
    let path = &config.files_chat;

    println!("Tailing {}... (Ctrl+C to stop)", path);

    tail_follow(path, false, None)
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
            print!("{}", buffer);
            let _ = io::stdout().flush();
            offset += bytes as u64;
        }

        thread::sleep(Duration::from_millis(200));
    }

    Ok(())
}

/// Run a single sprint.
fn run_sprint(config: &Config, sprint_number: usize) -> Result<usize, String> {
    // Load tasks
    let content = fs::read_to_string(&config.files_tasks)
        .map_err(|e| format!("failed to read {}: {}", config.files_tasks, e))?;
    let mut task_list = TaskList::parse(&content);

    // Unassign any incomplete tasks from previous sprints so they can be reassigned fresh
    let unassigned = task_list.unassign_all();
    if unassigned > 0 {
        // Write the updated task list to reflect unassignment
        fs::write(&config.files_tasks, task_list.to_string())
            .map_err(|e| format!("failed to write {}: {}", config.files_tasks, e))?;
    }

    // Determine how many agents to spawn
    let assignable = task_list.assignable_count();
    if assignable == 0 {
        return Ok(0);
    }

    let team_name = team_name_for_config(config);
    let mut assignments_state = Assignments::load()?;

    let tasks_per_agent = config.agents_tasks_per_agent;
    let agents_needed = (assignable + tasks_per_agent - 1) / tasks_per_agent;
    let agent_cap = agents_needed.min(config.agents_max_count);
    let initials = assignments_state.available_for_team(&team_name, agent_cap);
    if initials.is_empty() {
        println!("No available agents for team '{}'.", team_name);
        return Ok(0);
    }
    let agent_count = initials.len();

    // Assign tasks via LLM planning (with fallback to algorithmic)
    let engine = engine::create_engine(config.effective_engine(), &config.files_log_dir);
    let log_dir = Path::new(&config.files_log_dir);

    let plan_result = planning::run_llm_assignment(
        engine.as_ref(),
        &task_list,
        &initials,
        tasks_per_agent,
        log_dir,
    );

    let assigned = if !plan_result.success {
        eprintln!("LLM planning failed: {}, falling back to algorithmic assignment",
                 plan_result.error.unwrap_or_default());
        task_list.assign_sprint(&initials, tasks_per_agent)
    } else {
        // Apply LLM assignments (line numbers are 1-indexed in the response)
        let mut count = 0;
        for (line_num, initial) in &plan_result.assignments {
            // Convert line number to task index (0-indexed)
            let task_idx = line_num.saturating_sub(1);
            if task_idx < task_list.tasks.len() {
                task_list.tasks[task_idx].assign(*initial);
                count += 1;
            }
        }
        count
    };

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
    let assignments_ref: Vec<(char, &str)> = assignments
        .iter()
        .map(|(i, d)| (*i, d.as_str()))
        .collect();
    chat::write_sprint_plan(&config.files_chat, sprint_number, &assignments_ref)
        .map_err(|e| format!("failed to write chat: {}", e))?;

    // Commit assignment changes to git so worktrees can see them
    commit_task_assignments(&config.files_tasks, sprint_number)?;

    println!("Sprint {}: assigned {} task(s) to {} agent(s)",
             sprint_number, assigned, agent_count);

    // Clean up any existing worktrees for assigned agents before creating new ones
    // This ensures a clean slate from master for each sprint
    let worktrees_dir = Path::new(&config.files_worktrees_dir);
    let cleanup_summary = worktree::cleanup_agent_worktrees(
        worktrees_dir,
        &assigned_initials,
        true, // Also delete branches so they're recreated fresh from HEAD
    );
    if cleanup_summary.cleaned_count() > 0 {
        println!("  Pre-sprint cleanup: removed {} worktree(s)", cleanup_summary.cleaned_count());
    }
    for (initial, err) in &cleanup_summary.errors {
        let name = agent::name_from_initial(*initial).unwrap_or("?");
        eprintln!("  warning: pre-sprint cleanup failed for {} ({}): {}", name, initial, err);
    }

    // Create worktrees for assigned agents
    let worktrees: Vec<Worktree> = worktree::create_worktrees_in(
        worktrees_dir,
        &assignments,
    ).map_err(|e| format!("failed to create worktrees: {}", e))?;

    // Build a map from initial to worktree path (owned for thread safety)
    let worktree_map: std::collections::HashMap<char, std::path::PathBuf> = worktrees
        .iter()
        .map(|wt| (wt.initial, wt.path.clone()))
        .collect();

    // Initialize lifecycle tracker (wrapped for thread-safe access)
    let tracker = Arc::new(Mutex::new(LifecycleTracker::new()));
    for (initial, description) in &assignments {
        let agent_name = agent::name_from_initial(*initial).unwrap_or("Unknown");
        let wt_path = worktree_map
            .get(initial)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        tracker.lock().unwrap().register(*initial, agent_name, description, &wt_path);
    }

    // Create engine (wrapped for thread-safe sharing)
    let engine: Arc<dyn engine::Engine> = engine::create_engine(config.effective_engine(), &config.files_log_dir);

    // Rotate any large logs before starting
    let log_dir_path = config.files_log_dir.clone();
    if let Err(e) = log::rotate_logs_in_dir(Path::new(&log_dir_path), log::DEFAULT_MAX_LINES) {
        eprintln!("warning: failed to rotate logs: {}", e);
    }

    // Group assignments by agent (each agent processes their tasks sequentially)
    let mut agent_tasks: std::collections::HashMap<char, Vec<String>> = std::collections::HashMap::new();
    for (initial, description) in &assignments {
        agent_tasks.entry(*initial).or_default().push(description.clone());
    }

    // Execute agents in parallel, each agent processes their tasks sequentially
    let mut handles: Vec<thread::JoinHandle<Vec<(char, String, bool, Option<String>)>>> = Vec::new();

    for (initial, tasks) in agent_tasks {
        let working_dir = worktree_map
            .get(&initial)
            .cloned()
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let engine = Arc::clone(&engine);
        let tracker = Arc::clone(&tracker);
        let chat_path = config.files_chat.clone();
        let log_dir = log_dir_path.clone();
        let engine_type_str = config.effective_engine().as_str().to_string();

        let handle = thread::spawn(move || {
            let agent_name = agent::name_from_initial(initial).unwrap_or("Unknown");
            let mut task_results: Vec<(char, String, bool, Option<String>)> = Vec::new();

            // Create agent logger
            let logger = AgentLogger::new(Path::new(&log_dir), initial, agent_name);

            // Log session start
            if let Err(e) = logger.log_session_start() {
                eprintln!("warning: failed to write log: {}", e);
            }
            if let Err(e) = logger.log(&format!("Working directory: {}", working_dir.display())) {
                eprintln!("warning: failed to write log: {}", e);
            }

            // Process each task sequentially for this agent
            for description in tasks {
                // Log assignment
                if let Err(e) = logger.log(&format!("Assigned task: {}", description)) {
                    eprintln!("warning: failed to write log: {}", e);
                }

                // Transition: Assigned -> Working
                {
                    let mut t = tracker.lock().unwrap();
                    t.start(initial);
                }
                if let Err(e) = logger.log("State: ASSIGNED -> WORKING") {
                    eprintln!("warning: failed to write log: {}", e);
                }

                // Write agent start to chat
                if let Err(e) = chat::write_message(&chat_path, agent_name, &format!("Starting: {}", description)) {
                    eprintln!("warning: failed to write chat: {}", e);
                }

                // Execute via engine in the agent's worktree
                if let Err(e) = logger.log(&format!("Executing with engine: {}", engine_type_str)) {
                    eprintln!("warning: failed to write log: {}", e);
                }

                let result = engine.execute(
                    agent_name,
                    &description,
                    &working_dir,
                    sprint_number,
                );

                // Log engine output for debugging (truncated if very long)
                let output_preview = if result.output.len() > 500 {
                    format!("{}... [truncated, {} bytes total]", &result.output[..500], result.output.len())
                } else {
                    result.output.clone()
                };
                if !output_preview.is_empty() {
                    if let Err(e) = logger.log(&format!("Engine output:\n{}", output_preview)) {
                        eprintln!("warning: failed to write log: {}", e);
                    }
                }
                if let Some(ref err) = result.error {
                    if let Err(e) = logger.log(&format!("Engine error: {} (exit code: {})", err, result.exit_code)) {
                        eprintln!("warning: failed to write log: {}", e);
                    }
                }

                let (success, error) = if result.success {
                    // Transition: Working -> Done (success)
                    {
                        let mut t = tracker.lock().unwrap();
                        t.complete(initial);
                    }
                    if let Err(e) = logger.log("State: WORKING -> DONE (success)") {
                        eprintln!("warning: failed to write log: {}", e);
                    }

                    if let Err(e) = logger.log(&format!("Task completed: {}", description)) {
                        eprintln!("warning: failed to write log: {}", e);
                    }

                    if let Err(e) = chat::write_message(&chat_path, agent_name, &format!("Completed: {}", description)) {
                        eprintln!("warning: failed to write chat: {}", e);
                    }

                    // Commit the agent's work in their worktree (one commit per task)
                    if let Err(e) = logger.log("Committing changes...") {
                        eprintln!("warning: failed to write log: {}", e);
                    }
                    if let Err(e) = commit_agent_work(&working_dir, agent_name, &description) {
                        eprintln!("warning: failed to commit: {}", e);
                    }
                    if let Err(e) = logger.log("Commit successful") {
                        eprintln!("warning: failed to write log: {}", e);
                    }

                    (true, None)
                } else {
                    let err = result.error.unwrap_or_else(|| "unknown error".to_string());

                    // Transition: Working -> Done (failure)
                    {
                        let mut t = tracker.lock().unwrap();
                        t.fail(initial, &err);
                    }
                    if let Err(e) = logger.log(&format!("State: WORKING -> DONE (failed: {})", err)) {
                        eprintln!("warning: failed to write log: {}", e);
                    }

                    if let Err(e) = chat::write_message(&chat_path, agent_name, &format!("Failed: {} - {}", description, err)) {
                        eprintln!("warning: failed to write chat: {}", e);
                    }

                    (false, Some(err))
                };

                // Transition: Done -> Terminated
                {
                    let mut t = tracker.lock().unwrap();
                    t.terminate(initial);
                }
                if let Err(e) = logger.log("State: DONE -> TERMINATED") {
                    eprintln!("warning: failed to write log: {}", e);
                }

                task_results.push((initial, description, success, error));
            }

            task_results
        });

        handles.push(handle);
    }

    // Wait for all agents to complete and collect results
    let mut results: Vec<(char, String, bool, Option<String>)> = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok(agent_results) => results.extend(agent_results),
            Err(_) => eprintln!("warning: agent thread panicked"),
        }
    }

    // Update task list based on results
    for (initial, description, success, _error) in &results {
        if *success {
            for task in &mut task_list.tasks {
                if let swarm::task::TaskStatus::Assigned(i) = task.status {
                    if i == *initial && task.description == *description {
                        task.complete(*initial);
                        break;
                    }
                }
            }
        }
    }

    // Log lifecycle summary
    let tracker_guard = tracker.lock().unwrap();
    let (_, _, _, terminated) = tracker_guard.counts();
    println!("  Lifecycle: {} agents terminated ({} success, {} failed)",
             terminated, tracker_guard.success_count(), tracker_guard.failure_count());
    drop(tracker_guard);

    // Write final task state
    fs::write(&config.files_tasks, task_list.to_string())
        .map_err(|e| format!("failed to write {}: {}", config.files_tasks, e))?;

    // Clean up worktrees after sprint completes
    // This ensures worktrees are recreated fresh from master on the next sprint
    let cleanup_summary = worktree::cleanup_agent_worktrees(
        worktrees_dir,
        &assigned_initials,
        true, // Also delete branches
    );
    if cleanup_summary.cleaned_count() > 0 {
        println!("  Post-sprint cleanup: removed {} worktree(s)", cleanup_summary.cleaned_count());
    }
    for (initial, err) in &cleanup_summary.errors {
        let name = agent::name_from_initial(*initial).unwrap_or("?");
        eprintln!("  warning: post-sprint cleanup failed for {} ({}): {}", name, initial, err);
    }

    Ok(assigned)
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
