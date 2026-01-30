use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use swarm::agent;
use swarm::chat;
use swarm::color::{self, emoji};
use swarm::config::{Config, EngineType};
use swarm::engine;
use swarm::heartbeat;
use swarm::lifecycle::LifecycleTracker;
use swarm::log::{self, AgentLogger};
use swarm::merge_agent;
use swarm::planning;
use swarm::agent::INITIALS;
use swarm::run_context::RunContext;
use swarm::shutdown;
use swarm::task::TaskList;
use swarm::team;
use swarm::worktree::{self, Worktree};

use crate::git::{
    commit_files_in_worktree_on_branch, commit_sprint_completion, commit_task_assignments,
    get_current_commit_in, get_git_log_range_in, get_short_commit_for_ref_in, git_repo_root,
};
use crate::output::{print_sprint_start_banner, print_team_status_banner};
use crate::project::project_name_for_config;

type TaskResult = (char, String, bool, Option<String>, Option<Duration>);

/// Result of a single sprint execution.
#[derive(Debug, Clone)]
pub(crate) struct SprintResult {
    /// Number of tasks assigned in this sprint.
    pub(crate) tasks_assigned: usize,
    /// Number of tasks completed successfully.
    pub(crate) tasks_completed: usize,
    /// Number of tasks that failed.
    pub(crate) tasks_failed: usize,
}

impl SprintResult {
    /// Returns true if all assigned tasks failed.
    pub(crate) fn all_failed(&self) -> bool {
        self.tasks_assigned > 0 && self.tasks_completed == 0 && self.tasks_failed > 0
    }
}

/// Run a single sprint.
///
/// The `session_sprint_number` is the sprint number within this run session (1, 2, 3...).
/// The historical sprint number (used in commits) is loaded from sprint-history.json.
pub(crate) fn run_sprint(
    config: &Config,
    session_sprint_number: usize,
) -> Result<SprintResult, String> {
    // Load sprint history and determine sprint number (peek, don't write yet)
    let team_name = project_name_for_config(config);
    let target_branch = config
        .target_branch
        .as_deref()
        .ok_or_else(|| "target branch not configured".to_string())?;
    let repo_root = git_repo_root()?;
    sync_target_branch_state(&repo_root, target_branch, &team_name, config)?;

    // Load tasks from the synced main worktree state.
    let content = fs::read_to_string(&config.files_tasks)
        .map_err(|e| format!("failed to read {}: {}", config.files_tasks, e))?;
    let mut task_list = TaskList::parse(&content);

    let sprint_history = team::SprintHistory::load(&team_name)?;
    let historical_sprint = sprint_history.peek_next_sprint();
    let formatted_team = sprint_history.formatted_team_name();

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
        return Ok(SprintResult { tasks_assigned: 0, tasks_completed: 0, tasks_failed: 0 });
    }

    let tasks_per_agent = config.agents_tasks_per_agent;
    let agents_needed = assignable.div_ceil(tasks_per_agent);
    let agent_cap = agents_needed.min(config.agents_max_count);
    // With project-namespaced worktrees, all agents are available for any project
    let initials: Vec<char> = INITIALS.iter().take(agent_cap).copied().collect();
    if initials.is_empty() {
        println!("No agents available.");
        return Ok(SprintResult { tasks_assigned: 0, tasks_completed: 0, tasks_failed: 0 });
    }
    let agent_count = initials.len();

    // Assign tasks via LLM planning (with fallback to algorithmic)
    let engine = engine::create_engine(
        config.effective_engine(),
        &config.files_log_dir,
        config.agent_timeout_secs,
    );
    let log_dir = Path::new(&config.files_log_dir);

    if let Err(e) = chat::write_message(
        &config.files_chat,
        "ScrumMaster",
        "Sprint planning started",
    ) {
        eprintln!("warning: failed to write chat: {}", e);
    }

    let plan_result = planning::run_llm_assignment(
        engine.as_ref(),
        &task_list,
        &initials,
        tasks_per_agent,
        log_dir,
    );

    let assigned = if !plan_result.success {
        eprintln!(
            "LLM planning failed: {}, falling back to algorithmic assignment",
            plan_result.error.unwrap_or_default()
        );
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
        return Ok(SprintResult { tasks_assigned: 0, tasks_completed: 0, tasks_failed: 0 });
    }

    // Create run context for namespaced artifacts (worktrees, branches)
    // This is created early so the sprint branch uses the run hash
    let run_ctx = RunContext::new(&team_name, historical_sprint as u32);

    // Log run hash at sprint start for visibility
    println!(
        "{} {} Sprint {} (run {}): starting",
        emoji::SPRINT,
        color::info(&formatted_team),
        color::number(historical_sprint),
        color::info(run_ctx.hash())
    );

    // Compute sprint branch name using run context (includes run hash)
    let sprint_branch = run_ctx.sprint_branch();
    let worktrees_dir = Path::new(&config.files_worktrees_dir);

    let base_commit = get_short_commit_for_ref_in(&repo_root, target_branch)
        .or_else(|| get_short_commit_for_ref_in(&repo_root, "HEAD"))
        .unwrap_or_else(|| "unknown".to_string());
    if let Err(e) = chat::write_message(
        &config.files_chat,
        "ScrumMaster",
        &format!("Creating worktree {} from {}", sprint_branch, base_commit),
    ) {
        eprintln!("warning: failed to write chat: {}", e);
    }

    // Create sprint branch/worktree FIRST, before any file writes
    // This ensures all sprint setup files are written to the sprint worktree,
    // not the target branch (main/master)

    // Clean up any existing feature worktree from a failed previous sprint
    // This ensures we start fresh from the target branch
    if let Err(e) = worktree::cleanup_feature_worktree(worktrees_dir, &sprint_branch, true) {
        // Log but don't fail - the worktree might not exist
        eprintln!("  note: pre-sprint feature worktree cleanup: {}", e);
    }

    let feature_worktree_path =
        worktree::create_feature_worktree_in(
            worktrees_dir,
            &sprint_branch,
            target_branch,
        )
        .map_err(|e| format!("failed to create feature worktree: {}", e))?;

    // Print sprint start banner (after worktree creation to ensure we have a valid sprint)
    print_sprint_start_banner(&formatted_team, historical_sprint);

    // Construct the sprint worktree swarm directory path
    // All sprint state files are written here instead of the main repo
    let worktree_swarm_dir = feature_worktree_path
        .join(team::SWARM_HUG_DIR)
        .join(&team_name);

    // Load sprint history from worktree (creates default if first sprint)
    let worktree_history_path = worktree_swarm_dir.join(team::SPRINT_HISTORY_FILE);
    let mut sprint_history = team::SprintHistory::load_from(&worktree_history_path)?;
    // Set team name in case this is first sprint (load_from uses "unknown" for new files)
    if sprint_history.team_name == "unknown" {
        sprint_history.team_name = team_name.clone();
    }
    sprint_history.increment();
    sprint_history.save()?;

    // Load team state from worktree (creates default if first sprint)
    let worktree_state_path = worktree_swarm_dir.join(team::TEAM_STATE_FILE);
    let mut team_state = team::TeamState::load_from(&worktree_state_path)
        .map_err(|e| format!("failed to load team state from worktree: {}", e))?;
    team_state
        .set_feature_branch(&sprint_branch)
        .map_err(|e| format!("failed to set team state feature branch: {}", e))?;
    team_state
        .save()
        .map_err(|e| format!("failed to save team state to worktree: {}", e))?;
    let team_state_path = team_state.path().to_string_lossy().to_string();

    // Ensure worktree swarm dir exists
    let worktree_tasks_path = worktree_swarm_dir.join("tasks.md");
    fs::create_dir_all(&worktree_swarm_dir)
        .map_err(|e| format!("failed to create worktree swarm dir: {}", e))?;

    // Re-read task list from worktree to get any completions from previous sprints
    // that may have been committed to the sprint branch but not yet merged to main
    if worktree_tasks_path.exists() {
        let worktree_content = fs::read_to_string(&worktree_tasks_path)
            .map_err(|e| format!("failed to read {}: {}", worktree_tasks_path.display(), e))?;
        let worktree_task_list = TaskList::parse(&worktree_content);

        // Merge: keep completed tasks from worktree, apply new assignments from task_list
        for worktree_task in &worktree_task_list.tasks {
            if let swarm::task::TaskStatus::Completed(initial) = worktree_task.status {
                // Find matching task in our list and mark it completed
                for task in &mut task_list.tasks {
                    if task.description == worktree_task.description {
                        task.status = swarm::task::TaskStatus::Completed(initial);
                        break;
                    }
                }
            }
        }
    }

    // Write merged task list to worktree
    fs::write(&worktree_tasks_path, task_list.to_string())
        .map_err(|e| format!("failed to write {}: {}", worktree_tasks_path.display(), e))?;

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

    // Write sprint plan to chat
    let assignments_ref: Vec<(char, &str)> = assignments
        .iter()
        .map(|(i, d)| (*i, d.as_str()))
        .collect();
    chat::write_sprint_plan(&config.files_chat, historical_sprint, &assignments_ref)
        .map_err(|e| format!("failed to write chat: {}", e))?;

    // Commit assignment changes to git so worktrees can see them
    // Use worktree paths for all sprint state files
    commit_task_assignments(
        &feature_worktree_path,
        &sprint_branch,
        worktree_tasks_path.to_str().unwrap_or(""),
        worktree_history_path.to_str().unwrap_or(""),
        team_state_path.as_str(),
        &formatted_team,
        historical_sprint,
    )?;

    // Capture the commit hash at sprint start (after assignment commit)
    // This will be used to determine git range for post-sprint review
    let sprint_start_commit =
        get_current_commit_in(&feature_worktree_path).unwrap_or_else(|| "HEAD".to_string());

    println!(
        "{} {} Sprint {}: assigned {} task(s) to {} agent(s)",
        emoji::SPRINT,
        color::info(&formatted_team),
        color::number(historical_sprint),
        color::number(assigned),
        color::number(agent_count)
    );

    // Clean up any existing worktrees for assigned agents before creating new ones
    // This ensures a clean slate from the feature branch for each sprint
    let worktrees_dir = Path::new(&config.files_worktrees_dir);
    let cleanup_summary = worktree::cleanup_agent_worktrees(
        worktrees_dir,
        &assigned_initials,
        true, // Also delete branches so they're recreated fresh from the feature branch
        &run_ctx,
    );
    if cleanup_summary.cleaned_count() > 0 {
        println!(
            "  Pre-sprint cleanup: removed {} worktree(s)",
            cleanup_summary.cleaned_count()
        );
    }
    for (initial, err) in &cleanup_summary.errors {
        let name = agent::name_from_initial(*initial).unwrap_or("?");
        eprintln!(
            "  warning: pre-sprint cleanup failed for {} ({}): {}",
            name, initial, err
        );
    }

    // Create worktrees for assigned agents
    let worktrees: Vec<Worktree> =
        worktree::create_worktrees_in(
            worktrees_dir,
            &assignments,
            &sprint_branch,
            &run_ctx,
        )
            .map_err(|e| format!("failed to create worktrees: {}", e))?;

    // Build a map from initial to worktree path (owned for thread safety)
    let worktree_map: std::collections::HashMap<char, std::path::PathBuf> = worktrees
        .iter()
        .map(|wt| (wt.initial, wt.path.clone()))
        .collect();

    let worktrees_dir_buf = PathBuf::from(&config.files_worktrees_dir);

    // Initialize lifecycle tracker (wrapped for thread-safe access)
    let tracker = Arc::new(Mutex::new(LifecycleTracker::new()));
    for (initial, description) in &assignments {
        let agent_name = agent::name_from_initial(*initial).unwrap_or("Unknown");
        let wt_path = worktree_map
            .get(initial)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        tracker
            .lock()
            .unwrap()
            .register(*initial, agent_name, description, &wt_path);
    }

    let worktree_lock = Arc::new(Mutex::new(()));

    // Prepare engine configuration for per-agent random selection
    let engine_types = config.engine_types.clone();
    let engine_stub_mode = config.engine_stub_mode;
    let agent_timeout_secs = config.agent_timeout_secs;

    // Rotate any large logs before starting
    let log_dir_path = config.files_log_dir.clone();
    if let Err(e) = log::rotate_logs_in_dir(Path::new(&log_dir_path), log::DEFAULT_MAX_LINES) {
        eprintln!("warning: failed to rotate logs: {}", e);
    }

    // Group assignments by agent (each agent processes their tasks sequentially)
    let mut agent_tasks: std::collections::HashMap<char, Vec<String>> =
        std::collections::HashMap::new();
    for (initial, description) in &assignments {
        agent_tasks
            .entry(*initial)
            .or_default()
            .push(description.clone());
    }

    // Execute agents in parallel, each agent processes their tasks sequentially
    // Return type includes: (initial, description, success, error, duration)
    let mut handles: Vec<thread::JoinHandle<Vec<TaskResult>>> = Vec::new();

    // Derive team directory from tasks file path (e.g., ".swarm-hug/greenfield/tasks.md" -> ".swarm-hug/greenfield")
    let team_dir: Option<String> = Path::new(&config.files_tasks)
        .parent()
        .map(|p| p.to_string_lossy().to_string());

    for (initial, tasks) in agent_tasks {
        let mut working_dir = worktree_map
            .get(&initial)
            .cloned()
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let tracker = Arc::clone(&tracker);
        let chat_path = config.files_chat.clone();
        let log_dir = log_dir_path.clone();
        let team_dir = team_dir.clone();
        let worktrees_dir = worktrees_dir_buf.clone();
        let feature_worktree_path = feature_worktree_path.clone();
        let sprint_branch = sprint_branch.clone();
        let worktree_lock = Arc::clone(&worktree_lock);
        let run_ctx = run_ctx.clone();
        // Clone engine config for this thread
        let thread_engine_types = engine_types.clone();
        let thread_engine_stub_mode = engine_stub_mode;
        let thread_agent_timeout = agent_timeout_secs;

        let handle = thread::spawn(move || {
            let agent_name = agent::name_from_initial(initial).unwrap_or("Unknown");
            let mut task_results: Vec<TaskResult> = Vec::new();

            // Create agent logger
            let logger = AgentLogger::new(Path::new(&log_dir), initial, agent_name);

            // Log session start
            if let Err(e) = logger.log_session_start() {
                eprintln!("warning: failed to write log: {}", e);
            }
            if let Err(e) = logger.log(&format!("Working directory: {}", working_dir.display())) {
                eprintln!("warning: failed to write log: {}", e);
            }

            let total_tasks = tasks.len();

            // Process each task sequentially for this agent
            for (task_index, description) in tasks.iter().enumerate() {
                let description = description.clone();
                // Select and create random engine for this task (per-task engine selection)
                let (engine, selected_engine_type) = engine::create_random_engine(
                    &thread_engine_types,
                    thread_engine_stub_mode,
                    &log_dir,
                    thread_agent_timeout,
                );
                let engine_type_str = selected_engine_type.as_str().to_string();
                // Check for shutdown before starting a new task
                if shutdown::requested() {
                    if let Err(e) = logger.log("Shutdown requested, skipping remaining tasks") {
                        eprintln!("warning: failed to write log: {}", e);
                    }
                    // Mark remaining tasks as not completed (they stay assigned)
                    task_results.push((
                        initial,
                        description.clone(),
                        false,
                        Some("Shutdown requested".to_string()),
                        None,
                    ));
                    continue;
                }

                // Log assignment (including engine name for visibility)
                if let Err(e) = logger.log(&format!("Assigned task: {} [engine: {}]", description, engine_type_str)) {
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

                // Write agent start to chat (including engine name for visibility)
                if let Err(e) =
                    chat::write_message(&chat_path, agent_name, &format!("Starting: {} [engine: {}]", description, engine_type_str))
                {
                    eprintln!("warning: failed to write chat: {}", e);
                }

                // Execute via engine in the agent's worktree
                if let Err(e) = logger.log(&format!("Executing with engine: {}", engine_type_str)) {
                    eprintln!("warning: failed to write log: {}", e);
                }

                let task_start = Instant::now();
                let heartbeat_guard = heartbeat::HeartbeatGuard::start(
                    chat_path.as_str(),
                    agent_name,
                    &description,
                    heartbeat::default_interval(),
                );
                let result = engine.execute(
                    agent_name,
                    &description,
                    &working_dir,
                    session_sprint_number,
                    team_dir.as_deref(),
                );
                drop(heartbeat_guard);
                let task_duration = task_start.elapsed();

                // Log engine output for debugging (truncated if very long)
                let output_preview = if result.output.len() > 500 {
                    format!(
                        "{}... [truncated, {} bytes total]",
                        &result.output[..500],
                        result.output.len()
                    )
                } else {
                    result.output.clone()
                };
                if !output_preview.is_empty() {
                    if let Err(e) = logger.log(&format!("Engine output:\n{}", output_preview)) {
                        eprintln!("warning: failed to write log: {}", e);
                    }
                }
                if let Some(ref err) = result.error {
                    if let Err(e) =
                        logger.log(&format!("Engine error: {} (exit code: {})", err, result.exit_code))
                    {
                        eprintln!("warning: failed to write log: {}", e);
                    }
                }

                let mut allow_recreate = true;
                let (mut success, mut error) = if result.success {
                    // Transition: Working -> Done (success)
                    {
                        let mut t = tracker.lock().unwrap();
                        t.complete(initial);
                    }
                    if let Err(e) = logger.log("State: WORKING -> DONE (success)") {
                        eprintln!("warning: failed to write log: {}", e);
                    }

                    if let Err(e) = logger.log(&format!("Task completed: {} [engine: {}]", description, engine_type_str)) {
                        eprintln!("warning: failed to write log: {}", e);
                    }

                    if let Err(e) =
                        chat::write_message(&chat_path, agent_name, &format!("Completed: {}", description))
                    {
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
                    if let Err(e) = logger.log(&format!("State: WORKING -> DONE (failed: {})", err))
                    {
                        eprintln!("warning: failed to write log: {}", e);
                    }

                    if let Err(e) = chat::write_message(
                        &chat_path,
                        agent_name,
                        &format!("Failed: {} - {}", description, err),
                    ) {
                        eprintln!("warning: failed to write chat: {}", e);
                    }

                    (false, Some(err))
                };

                if success {
                    if let Err(e) = logger.log("Merging agent branch into sprint branch...") {
                        eprintln!("warning: failed to write log: {}", e);
                    }
                    let merge_result = {
                        let _guard = worktree_lock.lock().unwrap();
                        worktree::merge_agent_branch_in_with_ctx(
                            &feature_worktree_path,
                            &run_ctx,
                            initial,
                            Some(&sprint_branch),
                        )
                    };

                    let mut merge_error_detail = None;
                    let mut should_cleanup = false;

                    match merge_result {
                        worktree::MergeResult::Success => {
                            if let Err(e) = logger.log("Merge successful") {
                                eprintln!("warning: failed to write log: {}", e);
                            }
                            should_cleanup = true;
                        }
                        worktree::MergeResult::NoChanges => {
                            if let Err(e) = logger.log("Merge skipped: no changes detected") {
                                eprintln!("warning: failed to write log: {}", e);
                            }
                            should_cleanup = true;
                        }
                        worktree::MergeResult::NoBranch => {
                            merge_error_detail = Some("agent branch not found".to_string());
                        }
                        worktree::MergeResult::Conflict(files) => {
                            let detail = if files.is_empty() {
                                "conflicts detected".to_string()
                            } else {
                                format!("conflicts in {}", files.join(", "))
                            };
                            merge_error_detail = Some(detail);
                        }
                        worktree::MergeResult::Error(e) => {
                            merge_error_detail = Some(e);
                        }
                    }

                    if should_cleanup {
                        if let Err(e) = logger.log("Cleaning up agent worktree after merge...") {
                            eprintln!("warning: failed to write log: {}", e);
                        }
                        let cleanup_result = {
                            let _guard = worktree_lock.lock().unwrap();
                            worktree::cleanup_agent_worktree(&worktrees_dir, initial, true, &run_ctx)
                        };
                        if let Err(e) = cleanup_result {
                            let msg = format!("Worktree cleanup failed: {}", e);
                            if let Err(e) = logger.log(&msg) {
                                eprintln!("warning: failed to write log: {}", e);
                            }
                        } else if let Err(e) = logger.log("Worktree cleanup complete") {
                            eprintln!("warning: failed to write log: {}", e);
                        }
                    }

                    let merge_error = merge_error_detail
                        .as_ref()
                        .map(|detail| format!("Merge failed: {}", detail));

                    if let Some(detail) = merge_error_detail.as_ref() {
                        if let Err(e) = logger.log(&format!("Merge failed: {}", detail)) {
                            eprintln!("warning: failed to write log: {}", e);
                        }
                        if let Err(e) = write_merge_failure_chat(&chat_path, agent_name, detail) {
                            eprintln!("warning: failed to write chat: {}", e);
                        }
                    }

                    if let Some(msg) = merge_error {
                        success = false;
                        error = Some(msg);
                        allow_recreate = false;
                    }
                }

                // Transition: Done -> Terminated
                {
                    let mut t = tracker.lock().unwrap();
                    t.terminate(initial);
                }
                if let Err(e) = logger.log("State: DONE -> TERMINATED") {
                    eprintln!("warning: failed to write log: {}", e);
                }

                task_results.push((
                    initial,
                    description.clone(),
                    success,
                    error.clone(),
                    Some(task_duration),
                ));

                if task_index + 1 < total_tasks {
                    if !allow_recreate {
                        let msg = error
                            .clone()
                            .unwrap_or_else(|| "worktree recreation skipped".to_string());
                        for remaining in tasks.iter().skip(task_index + 1) {
                            task_results.push((initial, remaining.clone(), false, Some(msg.clone()), None));
                        }
                        break;
                    }
                    if let Err(e) = logger.log("Recreating worktree for next task...") {
                        eprintln!("warning: failed to write log: {}", e);
                    }
                    let recreate_assignments = vec![(initial, description.clone())];
                    let recreate_result = {
                        let _guard = worktree_lock.lock().unwrap();
                        worktree::create_worktrees_in(
                            &worktrees_dir,
                            &recreate_assignments,
                            &sprint_branch,
                            &run_ctx,
                        )
                    };
                    match recreate_result {
                        Ok(mut recreated) => {
                            if let Some(new_worktree) = recreated.pop() {
                                working_dir = new_worktree.path;
                                if let Err(e) = logger.log(&format!(
                                    "Worktree recreated at {}",
                                    working_dir.display()
                                )) {
                                    eprintln!("warning: failed to write log: {}", e);
                                }
                            } else {
                                let msg = "worktree recreation returned no worktree".to_string();
                                if let Err(e) = logger.log(&msg) {
                                    eprintln!("warning: failed to write log: {}", e);
                                }
                                for remaining in tasks.iter().skip(task_index + 1) {
                                    task_results.push((
                                        initial,
                                        remaining.clone(),
                                        false,
                                        Some(msg.clone()),
                                        None,
                                    ));
                                }
                                break;
                            }
                        }
                        Err(e) => {
                            let msg = format!("worktree recreation failed: {}", e);
                            if let Err(e) = logger.log(&msg) {
                                eprintln!("warning: failed to write log: {}", e);
                            }
                            for remaining in tasks.iter().skip(task_index + 1) {
                                task_results.push((
                                    initial,
                                    remaining.clone(),
                                    false,
                                    Some(msg.clone()),
                                    None,
                                ));
                            }
                            break;
                        }
                    }
                }
            }

            task_results
        });

        handles.push(handle);
    }

    // Wait for all agents to complete and collect results
    let mut results: Vec<TaskResult> = Vec::new();
    let shutdown_in_progress = shutdown::requested();
    let total_agents = handles.len();
    if shutdown_in_progress {
        println!("Waiting for {} agent(s) to finish current work...", total_agents);
    }
    for (idx, handle) in handles.into_iter().enumerate() {
        if shutdown_in_progress && idx > 0 {
            // Provide periodic status during shutdown
            println!("  {} agent(s) remaining...", total_agents - idx);
        }
        match handle.join() {
            Ok(agent_results) => results.extend(agent_results),
            Err(_) => eprintln!("warning: agent thread panicked"),
        }
    }
    if shutdown_in_progress {
        println!("All agents finished. Cleaning up sprint...");
    }

    // Collect task durations for successful tasks
    let task_durations: Vec<Duration> = results
        .iter()
        .filter_map(|(_, _, success, _, duration)| {
            if *success { duration.as_ref().copied() } else { None }
        })
        .collect();

    // Count successes and failures for this sprint
    let completed_this_sprint = results.iter().filter(|(_, _, s, _, _)| *s).count();
    let failed_this_sprint = results.iter().filter(|(_, _, s, _, _)| !*s).count();

    // Update task list based on results
    for (initial, description, success, _error, _duration) in &results {
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
    println!(
        "  {} Lifecycle: {} agents terminated ({} {}, {} {})",
        emoji::ROBOT,
        color::number(terminated),
        color::completed(&tracker_guard.success_count().to_string()),
        color::success("success"),
        color::failed(&tracker_guard.failure_count().to_string()),
        color::error("failed")
    );
    drop(tracker_guard);

    // Write final task state to worktree
    fs::write(&worktree_tasks_path, task_list.to_string())
        .map_err(|e| format!("failed to write {}: {}", worktree_tasks_path.display(), e))?;

    // Clean up worktrees after sprint completes
    // This ensures worktrees are recreated fresh from the feature branch on the next sprint
    let cleanup_summary = worktree::cleanup_agent_worktrees(
        worktrees_dir,
        &assigned_initials,
        true, // Also delete branches
        &run_ctx,
    );
    if cleanup_summary.cleaned_count() > 0 {
        println!(
            "  Post-sprint cleanup: removed {} worktree(s)",
            cleanup_summary.cleaned_count()
        );
    }
    for (initial, err) in &cleanup_summary.errors {
        let name = agent::name_from_initial(*initial).unwrap_or("?");
        eprintln!(
            "  warning: post-sprint cleanup failed for {} ({}): {}",
            name, initial, err
        );
    }

    // Commit sprint completion
    commit_sprint_completion(
        &feature_worktree_path,
        &sprint_branch,
        worktree_tasks_path.to_str().unwrap_or(""),
        &formatted_team,
        historical_sprint,
    )?;

    // Run post-sprint review to identify follow-up tasks (skip if shutting down)
    if shutdown::requested() {
        println!("  Skipping post-sprint review due to shutdown.");
    } else {
        run_post_sprint_review(
            config,
            engine.as_ref(),
            &feature_worktree_path,
            &sprint_branch,
            &sprint_start_commit,
            &task_list,
            &formatted_team,
            historical_sprint,
            &worktree_tasks_path,
        )?;
    }

    // Reload task list to get latest counts (post-sprint review may have added tasks)
    let final_content = fs::read_to_string(&worktree_tasks_path)
        .map_err(|e| format!("failed to read {}: {}", worktree_tasks_path.display(), e))?;
    let final_task_list = TaskList::parse(&final_content);
    let remaining_tasks = final_task_list.unassigned_count() + final_task_list.assigned_count();
    let total_tasks = final_task_list.tasks.len();

    if let Err(e) = chat::write_sprint_status(
        &config.files_chat,
        &formatted_team,
        historical_sprint,
        completed_this_sprint,
        failed_this_sprint,
        remaining_tasks,
        total_tasks,
    ) {
        eprintln!("warning: failed to write chat: {}", e);
    }

    // Print team status banner
    print_team_status_banner(
        &formatted_team,
        historical_sprint,
        completed_this_sprint,
        failed_this_sprint,
        remaining_tasks,
        total_tasks,
        &task_durations,
        config.sprints_max,
        agent_count,
    );

    // Merge sprint branch into target branch via merge agent.
    if shutdown::requested() {
        println!("  Skipping merge agent due to shutdown.");
    } else if sprint_branch == target_branch {
        println!("  Skipping merge agent: feature branch matches target branch.");
    } else {
        println!("  Merge agent: starting ({} -> {})", sprint_branch, target_branch);
        let merge_msg = format!("Merge agent: starting ({} -> {})", sprint_branch, target_branch);
        if let Err(e) = chat::write_message(&config.files_chat, "ScrumMaster", &merge_msg) {
            eprintln!("  warning: failed to write merge start to chat: {}", e);
        }
        let merge_cleanup_paths = vec![
            worktree_tasks_path.clone(),
            worktree_history_path.clone(),
            PathBuf::from(&team_state_path),
        ];
        merge_agent::prepare_merge_workspace(&feature_worktree_path, &merge_cleanup_paths)
            .map_err(|e| format!("merge agent failed: {}", e))?;
        let merge_result = merge_agent::run_merge_agent(
            engine.as_ref(),
            &sprint_branch,
            target_branch,
            &feature_worktree_path,
        )
        .map_err(|e| format!("merge agent failed: {}", e))?;
        if merge_result.success {
            merge_agent::ensure_feature_merged(
                engine.as_ref(),
                &sprint_branch,
                target_branch,
                &feature_worktree_path,
            )
            .map_err(|e| format!("merge agent failed: {}", e))?;
            println!("  Merge agent: completed");
            if let Err(e) = chat::write_message(&config.files_chat, "ScrumMaster", "Merge agent: completed") {
                eprintln!("  warning: failed to write merge complete to chat: {}", e);
            }
            let merged = worktree::branch_is_merged(&sprint_branch, target_branch)
                .map_err(|e| format!("merge verification failed: {}", e))?;
            let mut merged_ok = merged;
            if !merged {
                if engine.engine_type() == EngineType::Stub {
                    let merge_result =
                        worktree::merge_feature_branch(&sprint_branch, target_branch);
                    match merge_result {
                        worktree::MergeResult::Success | worktree::MergeResult::NoChanges => {
                            println!("  Merge agent: merged feature branch (stub)");
                            merged_ok = true;
                        }
                        worktree::MergeResult::NoBranch => {
                            return Err(format!(
                                "merge agent failed: feature branch '{}' not found",
                                sprint_branch
                            ));
                        }
                        worktree::MergeResult::Conflict(files) => {
                            let detail = if files.is_empty() {
                                "conflicts detected".to_string()
                            } else {
                                format!("conflicts in {}", files.join(", "))
                            };
                            return Err(format!("merge agent failed: {}", detail));
                        }
                        worktree::MergeResult::Error(e) => {
                            return Err(format!("merge agent failed: {}", e));
                        }
                    }
                } else {
                    return Err(format!(
                        "merge agent did not merge '{}' into '{}'",
                        sprint_branch, target_branch
                    ));
                }
            }

            if merged_ok {
                if let Err(e) =
                    worktree::cleanup_feature_worktree(worktrees_dir, &sprint_branch, true)
                {
                    eprintln!("  warning: feature worktree cleanup failed: {}", e);
                } else {
                    println!("  Feature cleanup: removed '{}'", sprint_branch);
                }
            }
        } else {
            let detail = merge_result
                .error
                .unwrap_or_else(|| "unknown error".to_string());
            println!("  Merge agent: failed");
            if let Err(e) = chat::write_message(&config.files_chat, "ScrumMaster", &format!("Merge agent: failed ({})", detail)) {
                eprintln!("  warning: failed to write merge failure to chat: {}", e);
            }
            return Err(format!("merge agent failed: {}", detail));
        }
    }

    Ok(SprintResult {
        tasks_assigned: assigned,
        tasks_completed: completed_this_sprint,
        tasks_failed: failed_this_sprint,
    })
}

fn sync_target_branch_state(
    repo_root: &Path,
    target_branch: &str,
    team_name: &str,
    config: &Config,
) -> Result<(), String> {
    if branch_is_checked_out(repo_root, target_branch)? {
        return Ok(());
    }

    let target_worktree = worktree::create_target_branch_worktree_in(repo_root, target_branch)?;

    let tasks_path = Path::new(&config.files_tasks);
    if tasks_path.is_relative() {
        let src = target_worktree.join(tasks_path);
        let dst = repo_root.join(tasks_path);
        copy_if_exists(&src, &dst)?;
    }

    let sprint_rel = Path::new(team::SWARM_HUG_DIR)
        .join(team_name)
        .join(team::SPRINT_HISTORY_FILE);
    let src = target_worktree.join(&sprint_rel);
    let dst = repo_root.join(&sprint_rel);
    copy_if_exists(&src, &dst)?;

    Ok(())
}

fn branch_is_checked_out(repo_root: &Path, target_branch: &str) -> Result<bool, String> {
    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(|e| format!("git rev-parse failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git rev-parse failed: {}", stderr.trim()));
    }

    let current = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(current == target_branch)
}

fn copy_if_exists(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {}", parent.display(), e))?;
    }

    fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| format!("failed to copy {} to {}: {}", src.display(), dst.display(), e))
}

/// Run post-sprint review to identify follow-up tasks.
#[allow(clippy::too_many_arguments)]
fn run_post_sprint_review(
    config: &Config,
    engine: &dyn engine::Engine,
    feature_worktree: &Path,
    sprint_branch: &str,
    sprint_start_commit: &str,
    task_list: &TaskList,
    team_name: &str,
    sprint_number: usize,
    worktree_tasks_path: &Path,
) -> Result<(), String> {
    // Get git log from sprint start to now
    let git_log = get_git_log_range_in(feature_worktree, sprint_start_commit, "HEAD")?;

    // If no changes, skip review
    if git_log.trim().is_empty() {
        println!("  Post-sprint review: skipped (no git changes detected)");
        return Ok(());
    }

    // Construct worktree-relative chat.md path for follow-up tasks commit
    let worktree_chat_path = feature_worktree
        .join(".swarm-hug")
        .join(team_name)
        .join("chat.md");
    let worktree_chat_str = worktree_chat_path.to_str().unwrap_or("");

    // Get current tasks content
    let tasks_content = task_list.to_string();

    if let Err(e) = chat::write_message(
        &config.files_chat,
        "ScrumMaster",
        "Post-mortem started",
    ) {
        eprintln!("warning: failed to write chat: {}", e);
    }

    // Run the review
    let log_dir = Path::new(&config.files_log_dir);
    match planning::run_sprint_review(engine, &tasks_content, &git_log, log_dir) {
        Ok(follow_ups) => {
            let start_number = task_list.max_task_number().saturating_add(1);
            let formatted_follow_ups = planning::format_follow_up_tasks(start_number, &follow_ups);

            if formatted_follow_ups.is_empty() {
                println!("  Post-sprint review: no follow-up tasks needed");
            } else {
                println!(
                    "  Post-sprint review: {} follow-up task(s) identified",
                    formatted_follow_ups.len()
                );

                // Append follow-up tasks to TASKS.md in worktree
                let mut current_content = fs::read_to_string(worktree_tasks_path)
                    .unwrap_or_default();

                // Ensure newline before appending
                if !current_content.ends_with('\n') {
                    current_content.push('\n');
                }

                // Add follow-up tasks
                current_content.push_str("\n## Follow-up tasks (from sprint review)\n");
                for task in &formatted_follow_ups {
                    current_content.push_str(task);
                    current_content.push('\n');
                    println!("    {}", task);
                }

                fs::write(worktree_tasks_path, current_content)
                    .map_err(|e| format!("failed to write follow-up tasks: {}", e))?;

                // Write to chat
                let msg = format!(
                    "Sprint review added {} follow-up task(s)",
                    formatted_follow_ups.len()
                );
                if let Err(e) = chat::write_message(worktree_chat_str, "ScrumMaster", &msg) {
                    eprintln!("  warning: failed to write chat: {}", e);
                }

                // Commit follow-up tasks so next planning phase sees them
                let commit_msg =
                    format!("{} Sprint {}: follow-up tasks from review", team_name, sprint_number);
                let tasks_path_str = worktree_tasks_path.to_str().unwrap_or("");
                if let Ok(true) = commit_files_in_worktree_on_branch(
                    feature_worktree,
                    sprint_branch,
                    &[tasks_path_str, worktree_chat_str],
                    &commit_msg,
                ) {
                    println!("  Committed follow-up tasks to git.");
                }
            }
        }
        Err(e) => {
            eprintln!("  warning: post-sprint review failed: {}", e);
        }
    }

    Ok(())
}

fn write_merge_failure_chat(chat_path: &str, agent_name: &str, detail: &str) -> std::io::Result<()> {
    let msg = format!("Merge failed for {}: {}", agent_name, detail);
    chat::write_message(chat_path, "ScrumMaster", &msg)
}

/// Commit an agent's work in their worktree.
/// Each agent makes one commit per task (enforces one task = one commit rule).
fn commit_agent_work(
    worktree_path: &Path,
    agent_name: &str,
    task_description: &str,
) -> Result<(), String> {
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
    use super::{chat, sync_target_branch_state, write_merge_failure_chat, SprintResult};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::NamedTempFile;

    use swarm::config::Config;
    use swarm::{team, worktree};

    fn run_git_in(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git command");
        assert!(
            output.status.success(),
            "git -C {} {:?} failed\nstdout:\n{}\nstderr:\n{}",
            dir.display(),
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(repo_root: &Path) {
        run_git_in(repo_root, &["init"]);
        run_git_in(repo_root, &["config", "user.name", "Swarm Test"]);
        run_git_in(repo_root, &["config", "user.email", "swarm-test@example.com"]);
        fs::write(repo_root.join("README.md"), "init").expect("write readme");
        run_git_in(repo_root, &["add", "."]);
        run_git_in(repo_root, &["commit", "-m", "init"]);
        run_git_in(repo_root, &["branch", "-M", "main"]);
    }

    #[test]
    fn test_sprint_result_all_failed_true() {
        let result = SprintResult {
            tasks_assigned: 3,
            tasks_completed: 0,
            tasks_failed: 3,
        };
        assert!(result.all_failed());
    }

    #[test]
    fn test_sprint_result_all_failed_false_with_success() {
        let result = SprintResult {
            tasks_assigned: 3,
            tasks_completed: 1,
            tasks_failed: 2,
        };
        assert!(!result.all_failed());
    }

    #[test]
    fn test_sprint_result_all_failed_false_no_tasks() {
        let result = SprintResult {
            tasks_assigned: 0,
            tasks_completed: 0,
            tasks_failed: 0,
        };
        assert!(!result.all_failed());
    }

    #[test]
    fn test_sprint_result_all_failed_false_all_success() {
        let result = SprintResult {
            tasks_assigned: 2,
            tasks_completed: 2,
            tasks_failed: 0,
        };
        assert!(!result.all_failed());
    }

    #[test]
    fn test_write_merge_failure_chat() {
        let temp = NamedTempFile::new().expect("temp chat file");
        write_merge_failure_chat(
            temp.path().to_str().expect("temp path"),
            "Aaron",
            "conflicts in file.txt",
        )
        .expect("write merge failure chat");

        let content = fs::read_to_string(temp.path()).expect("read chat");
        let line = content.lines().next().expect("chat line");
        let (_, agent, message) = chat::parse_line(line).expect("parse chat line");
        assert_eq!(agent, "ScrumMaster");
        assert_eq!(message, "Merge failed for Aaron: conflicts in file.txt");
    }

    #[test]
    fn test_sync_target_branch_state_refreshes_tasks_and_history() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        let team_name = "greenfield";
        let team_dir = repo_root.join(".swarm-hug").join(team_name);
        fs::create_dir_all(&team_dir).expect("create team dir");
        let tasks_path = team_dir.join("tasks.md");
        let history_path = team_dir.join("sprint-history.json");
        fs::write(&tasks_path, "# Tasks\n\n- [ ] Task one\n").expect("write tasks");
        fs::write(
            &history_path,
            r#"{"team": "greenfield", "total_sprints": 0}"#,
        )
        .expect("write history");
        run_git_in(&repo_root, &["add", "."]);
        run_git_in(&repo_root, &["commit", "-m", "init state"]);
        run_git_in(&repo_root, &["checkout", "-b", "dev"]);

        let target_worktree =
            worktree::create_target_branch_worktree_in(&repo_root, "main")
                .expect("create target worktree");
        let target_team_dir = target_worktree.join(".swarm-hug").join(team_name);
        fs::create_dir_all(&target_team_dir).expect("create target team dir");
        fs::write(
            target_team_dir.join("tasks.md"),
            "# Tasks\n\n- [x] Task one\n",
        )
        .expect("write updated tasks");
        fs::write(
            target_team_dir.join("sprint-history.json"),
            r#"{"team": "greenfield", "total_sprints": 1}"#,
        )
        .expect("write updated history");
        run_git_in(&target_worktree, &["add", ".swarm-hug"]);
        run_git_in(&target_worktree, &["commit", "-m", "complete task"]);

        let before = fs::read_to_string(&tasks_path).expect("read tasks before");
        assert!(before.contains("[ ]"));

        let mut config = Config::default();
        config.project = Some(team_name.to_string());
        config.target_branch = Some("main".to_string());
        config.files_tasks = format!(".swarm-hug/{}/tasks.md", team_name);

        sync_target_branch_state(&repo_root, "main", team_name, &config)
            .expect("sync target branch state");

        let after = fs::read_to_string(&tasks_path).expect("read tasks after");
        assert!(after.contains("[x]"));
        let history = team::SprintHistory::load_from(&history_path).expect("load history");
        assert_eq!(history.total_sprints, 1);
    }
}
