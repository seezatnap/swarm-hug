use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use swarm::agent;
use swarm::agent::INITIALS;
use swarm::chat;
use swarm::color::{self, emoji};
use swarm::config::{Config, EngineType};
use swarm::engine;
use swarm::heartbeat;
use swarm::lifecycle::LifecycleTracker;
use swarm::log::{self, AgentLogger, NamedLogger};
use swarm::merge_agent;
use swarm::planning;
use swarm::run_context::RunContext;
use swarm::shutdown;
use swarm::task::TaskList;
use swarm::team;
use swarm::worktree::{self, Worktree};

use crate::git::{
    commit_files_in_worktree_on_branch, commit_sprint_completion, commit_task_assignments,
    create_pull_request, get_commit_log_between, get_current_commit_in, get_git_log_range_in,
    get_short_commit_for_ref_in, git_repo_root, push_branch_to_remote, PullRequestCreateResult,
};
use crate::output::{print_sprint_start_banner, print_team_status_banner};
use crate::project::project_name_for_config;

type TaskResult = (char, String, bool, Option<String>, Option<Duration>);

#[derive(Debug, Clone)]
struct MergeFailureInfo {
    initial: char,
    agent_name: String,
    branch: String,
    worktree_path: String,
    log_path: String,
    detail: String,
    skip_cleanup: bool,
}

fn split_cleanup_initials(
    initials: &[char],
    merge_failures: &[MergeFailureInfo],
) -> (Vec<char>, Vec<char>) {
    let mut cleanup = Vec::new();
    let mut skipped = Vec::new();

    for initial in initials {
        let failed = merge_failures
            .iter()
            .any(|failure| failure.initial == *initial && failure.skip_cleanup);
        if failed {
            skipped.push(*initial);
        } else {
            cleanup.push(*initial);
        }
    }

    (cleanup, skipped)
}

struct PreserveOutcome {
    path: PathBuf,
    allow_recreate: bool,
    error: Option<String>,
}

fn preserve_failed_worktree(
    repo_root: &Path,
    worktrees_dir: &Path,
    worktree_path: &Path,
    branch: &str,
    task_index: usize,
) -> PreserveOutcome {
    let mut outcome = PreserveOutcome {
        path: worktree_path.to_path_buf(),
        allow_recreate: false,
        error: None,
    };

    if !worktree_path.exists() {
        outcome.error = Some(format!(
            "worktree path does not exist: {}",
            worktree_path.display()
        ));
        return outcome;
    }

    let worktrees_dir = if worktrees_dir.is_absolute() {
        worktrees_dir.to_path_buf()
    } else {
        repo_root.join(worktrees_dir)
    };
    let preserved_root = worktrees_dir.join("preserved");
    if let Err(e) = fs::create_dir_all(&preserved_root) {
        outcome.error = Some(format!(
            "failed to create preserved worktrees dir {}: {}",
            preserved_root.display(),
            e
        ));
        return outcome;
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut preserved_path =
        preserved_root.join(format!("{}-preserved-{}-{}", branch, task_index + 1, ts));
    if preserved_path.exists() {
        preserved_path = preserved_root.join(format!(
            "{}-preserved-{}-{}-{}",
            branch,
            task_index + 1,
            ts,
            process::id()
        ));
    }

    let worktree_path_str = worktree_path.to_string_lossy().to_string();
    let preserved_path_str = preserved_path.to_string_lossy().to_string();

    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "move", &worktree_path_str, &preserved_path_str])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            outcome.path = preserved_path;
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            outcome.error = Some(format!("git worktree move failed: {}", stderr.trim()));
            return outcome;
        }
        Err(e) => {
            outcome.error = Some(format!("failed to run git worktree move: {}", e));
            return outcome;
        }
    }

    let output = process::Command::new("git")
        .arg("-C")
        .arg(&outcome.path)
        .args(["checkout", "--detach"])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            outcome.allow_recreate = true;
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            outcome.error = Some(format!(
                "git checkout --detach failed in preserved worktree: {}",
                stderr.trim()
            ));
        }
        Err(e) => {
            outcome.error = Some(format!("failed to detach preserved worktree: {}", e));
        }
    }

    outcome
}

fn create_branch_at_commit(repo_root: &Path, branch: &str, commit: &str) -> Result<(), String> {
    if branch.trim().is_empty() {
        return Err("branch name is empty".to_string());
    }
    if commit.trim().is_empty() {
        return Err("commit hash is empty".to_string());
    }

    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["branch", branch, commit])
        .output()
        .map_err(|e| format!("failed to run git branch: {}", e))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("already exists") {
        Ok(())
    } else {
        Err(format!("git branch failed: {}", stderr.trim()))
    }
}

fn create_sprint_worktree_in(
    worktrees_dir: &Path,
    sprint_branch: &str,
    source_branch: &str,
) -> Result<PathBuf, String> {
    worktree::create_feature_worktree_in(worktrees_dir, sprint_branch, source_branch)
        .map_err(|e| format!("failed to create feature worktree: {}", e))
}

fn target_contains_source_tip(
    repo_root: &Path,
    source_branch: &str,
    target_branch: &str,
) -> Result<bool, String> {
    let source = source_branch.trim();
    if source.is_empty() {
        return Err("source branch name is empty".to_string());
    }
    let target = target_branch.trim();
    if target.is_empty() {
        return Err("target branch name is empty".to_string());
    }

    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["merge-base", "--is-ancestor", source, target])
        .output()
        .map_err(|e| format!("failed to run git merge-base: {}", e))?;

    if output.status.success() {
        Ok(true)
    } else if output.status.code() == Some(1) {
        Ok(false)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git merge-base failed: {}", stderr.trim()))
    }
}

fn resolve_sprint_base_branch(
    repo_root: &Path,
    source_branch: &str,
    target_branch: &str,
) -> Result<String, String> {
    let source = source_branch.trim();
    if source.is_empty() {
        return Err("source branch name is empty".to_string());
    }
    let target = target_branch.trim();
    if target.is_empty() {
        return Err("target branch name is empty".to_string());
    }

    if source == target {
        return Ok(source.to_string());
    }

    if target_contains_source_tip(repo_root, source, target)? {
        Ok(target.to_string())
    } else {
        Ok(source.to_string())
    }
}

fn engine_team_dir(team_name: &str, fallback_tasks_path: &str) -> String {
    let trimmed = team_name.trim();
    if trimmed.is_empty() {
        return Path::new(fallback_tasks_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
    }

    Path::new(team::SWARM_HUG_DIR)
        .join(trimmed)
        .to_string_lossy()
        .to_string()
}

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

/// Retry the merge agent once after an initial `ensure_feature_merged` failure.
///
/// Re-prepares the workspace, re-runs the merge agent, and re-checks merge status.
/// Returns `Ok(())` if the retry succeeds, or an error combining both attempts' context.
#[cfg(test)]
fn retry_merge_agent(
    engine: &dyn swarm::engine::Engine,
    sprint_branch: &str,
    target_branch: &str,
    feature_worktree_path: &Path,
    merge_cleanup_paths: &[PathBuf],
    first_err: &str,
    merge_logger: &log::NamedLogger,
) -> Result<(), String> {
    // Re-prepare workspace for the retry attempt.
    if let Err(e) = merge_agent::prepare_merge_workspace(feature_worktree_path, merge_cleanup_paths)
    {
        let _ = merge_logger.log(&format!("Retry prepare workspace failed: {}", e));
        return Err(format!(
            "merge agent failed: attempt 1: {}; retry prepare failed: {}",
            first_err, e
        ));
    }
    let _ = merge_logger.log("Retry: workspace re-prepared");

    let retry_result =
        merge_agent::run_merge_agent(engine, sprint_branch, target_branch, feature_worktree_path)
            .map_err(|e| {
            let _ = merge_logger.log(&format!("Retry merge agent execution failed: {}", e));
            format!(
                "merge agent failed: attempt 1: {}; retry execution failed: {}",
                first_err, e
            )
        })?;

    if !retry_result.output.is_empty() {
        let output_preview = if retry_result.output.len() > 1000 {
            format!(
                "{}... [truncated, {} bytes total]",
                &retry_result.output[..1000],
                retry_result.output.len()
            )
        } else {
            retry_result.output.clone()
        };
        let _ = merge_logger.log(&format!("Retry engine output:\n{}", output_preview));
    }
    let _ = merge_logger.log(&format!(
        "Retry engine result: {} (exit_code={})",
        if retry_result.success {
            "success"
        } else {
            "failure"
        },
        retry_result.exit_code
    ));
    if let Some(err) = retry_result.error.as_deref() {
        let _ = merge_logger.log(&format!("Retry engine error: {}", err));
    }

    if !retry_result.success {
        let detail = retry_result
            .error
            .unwrap_or_else(|| "unknown error".to_string());
        let _ = merge_logger.log(&format!("Retry merge agent not successful: {}", detail));
        return Err(format!(
            "merge agent failed: attempt 1: {}; retry failed: {}",
            first_err, detail
        ));
    }

    // Re-check merge status after retry.
    if let Err(retry_err) = merge_agent::ensure_feature_merged(
        engine,
        sprint_branch,
        target_branch,
        feature_worktree_path,
    ) {
        let _ = merge_logger.log(&format!(
            "Merge verification failed (attempt 2): {}",
            retry_err
        ));
        return Err(format!(
            "merge agent failed after retry: attempt 1: {}; attempt 2: {}",
            first_err, retry_err
        ));
    }
    let _ = merge_logger.log("Merge verification succeeded on retry (attempt 2)");
    Ok(())
}

const DEFAULT_PR_BODY: &str = "Auto-generated by swarm sprint.";

fn default_pr_title(target_branch: &str) -> String {
    format!("[swarm] {}", target_branch)
}

fn build_pr_metadata_prompt(source_branch: &str, target_branch: &str, commit_log: &str) -> String {
    let commit_log = if commit_log.trim().is_empty() {
        "(no commits found in range)".to_string()
    } else {
        commit_log.trim().to_string()
    };

    format!(
        "Generate a GitHub pull request title and description.\n\
Return only a JSON object with exactly two string keys: \"title\" and \"body\".\n\
No markdown, no code fences, and no extra text.\n\n\
Source branch: {}\n\
Target branch: {}\n\n\
Commit log (`git log --oneline {}..{}`):\n{}\n",
        source_branch, target_branch, source_branch, target_branch, commit_log
    )
}

fn skip_json_whitespace(input: &str, mut index: usize) -> usize {
    while index < input.len() {
        let Some(ch) = input[index..].chars().next() else {
            break;
        };
        if ch.is_whitespace() {
            index += ch.len_utf8();
        } else {
            break;
        }
    }
    index
}

fn parse_json_string_at(input: &str, start_index: usize) -> Option<(String, usize)> {
    if input.as_bytes().get(start_index) != Some(&b'"') {
        return None;
    }

    let mut decoded = String::new();
    let mut index = start_index + 1;

    while index < input.len() {
        let ch = input[index..].chars().next()?;
        match ch {
            '"' => return Some((decoded, index + 1)),
            '\\' => {
                index += 1;
                let escaped = input[index..].chars().next()?;
                match escaped {
                    '"' => decoded.push('"'),
                    '\\' => decoded.push('\\'),
                    '/' => decoded.push('/'),
                    'b' => decoded.push('\u{0008}'),
                    'f' => decoded.push('\u{000C}'),
                    'n' => decoded.push('\n'),
                    'r' => decoded.push('\r'),
                    't' => decoded.push('\t'),
                    'u' => {
                        let hex_start = index + 1;
                        let hex_end = hex_start + 4;
                        if hex_end > input.len() {
                            return None;
                        }
                        let codepoint = u32::from_str_radix(&input[hex_start..hex_end], 16).ok()?;
                        let value = char::from_u32(codepoint)?;
                        decoded.push(value);
                        index = hex_end;
                        continue;
                    }
                    _ => return None,
                }
                index += escaped.len_utf8();
            }
            _ => {
                decoded.push(ch);
                index += ch.len_utf8();
            }
        }
    }

    None
}

fn parse_json_string_field(json: &str, key: &str) -> Option<String> {
    let key_pattern = format!("\"{}\"", key);
    let mut search_start = 0;

    while search_start < json.len() {
        let relative = json[search_start..].find(&key_pattern)?;
        let key_start = search_start + relative;
        let mut value_start = key_start + key_pattern.len();
        value_start = skip_json_whitespace(json, value_start);
        if json.as_bytes().get(value_start) != Some(&b':') {
            search_start = key_start + key_pattern.len();
            continue;
        }

        value_start += 1;
        value_start = skip_json_whitespace(json, value_start);
        if let Some((value, _)) = parse_json_string_at(json, value_start) {
            return Some(value);
        }

        search_start = key_start + key_pattern.len();
    }

    None
}

fn find_matching_json_object_end(output: &str, start_index: usize) -> Option<usize> {
    if output[start_index..].chars().next()? != '{' {
        return None;
    }

    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in output[start_index..].char_indices() {
        let index = start_index + offset;
        if in_string {
            if escaped {
                escaped = false;
            } else {
                match ch {
                    '\\' => escaped = true,
                    '"' => in_string = false,
                    _ => {}
                }
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn parse_pr_metadata_from_engine_output(output: &str) -> Option<(String, String)> {
    for (start_index, ch) in output.char_indices() {
        if ch != '{' {
            continue;
        }

        let Some(end_index) = find_matching_json_object_end(output, start_index) else {
            continue;
        };
        let candidate = &output[start_index..=end_index];
        let Some(title) = parse_json_string_field(candidate, "title") else {
            continue;
        };
        let Some(body) = parse_json_string_field(candidate, "body") else {
            continue;
        };
        if !title.trim().is_empty() && !body.trim().is_empty() {
            return Some((title, body));
        }
    }

    None
}

fn truncate_for_log_chars(input: &str, max_chars: usize) -> String {
    let mut preview: String = input.chars().take(max_chars).collect();
    let total_chars = input.chars().count();
    if total_chars > max_chars {
        preview.push_str(&format!("... [truncated, {} chars total]", total_chars));
    }
    preview
}

#[allow(clippy::too_many_arguments)]
fn generate_pr_title_and_body(
    engine: &dyn engine::Engine,
    repo_root: &Path,
    working_dir: &Path,
    session_sprint_number: usize,
    team_dir: Option<&str>,
    source_branch: &str,
    target_branch: &str,
    merge_logger: &NamedLogger,
) -> (String, String) {
    let commit_log = match get_commit_log_between(repo_root, source_branch, target_branch) {
        Ok(log) => log,
        Err(e) => {
            let _ = merge_logger.log(&format!("Failed to load commit log for PR metadata: {}", e));
            String::new()
        }
    };
    let prompt = build_pr_metadata_prompt(source_branch, target_branch, &commit_log);
    let pr_result = engine.execute(
        "ScrumMaster",
        &prompt,
        working_dir,
        session_sprint_number,
        team_dir,
    );

    if !pr_result.success {
        let detail = pr_result
            .error
            .unwrap_or_else(|| "unknown error".to_string());
        let _ = merge_logger.log(&format!(
            "PR metadata generation failed; using defaults: {}",
            detail
        ));
        return (default_pr_title(target_branch), DEFAULT_PR_BODY.to_string());
    }

    if let Some((title, body)) = parse_pr_metadata_from_engine_output(&pr_result.output) {
        (title, body)
    } else {
        let output_preview = truncate_for_log_chars(&pr_result.output, 400);
        let _ = merge_logger.log(&format!(
            "PR metadata parse failed; using defaults. output_preview='{}'",
            output_preview
        ));
        (default_pr_title(target_branch), DEFAULT_PR_BODY.to_string())
    }
}

fn report_pull_request_creation(
    result: PullRequestCreateResult,
    merge_logger: &NamedLogger,
    chat_file: &str,
) {
    match result {
        PullRequestCreateResult::Created {
            url,
            stdout,
            stderr,
        } => {
            let url = url.unwrap_or_else(|| "(no URL returned)".to_string());
            println!("  PR: created {}", url);
            let _ = merge_logger.log(&format!("PR created: {}", url));
            if !stdout.trim().is_empty() {
                let _ = merge_logger.log(&format!("PR create stdout: {}", stdout.trim()));
            }
            if !stderr.trim().is_empty() {
                let _ = merge_logger.log(&format!("PR create stderr: {}", stderr.trim()));
            }
            if let Err(e) =
                chat::write_message(chat_file, "ScrumMaster", &format!("PR: created {}", url))
            {
                eprintln!("  warning: failed to write PR creation to chat: {}", e);
            }
        }
        PullRequestCreateResult::Skipped { reason } => {
            eprintln!(
                "  warning: failed to create pull request (continuing): {}",
                reason
            );
            let _ = merge_logger.log(&format!("PR creation skipped: {}", reason));
            if let Err(e) = chat::write_message(
                chat_file,
                "ScrumMaster",
                &format!("PR: skipped ({})", reason),
            ) {
                eprintln!("  warning: failed to write PR skip to chat: {}", e);
            }
        }
        PullRequestCreateResult::Failed {
            stdout,
            stderr,
            exit_code,
        } => {
            eprintln!("  warning: failed to create pull request (continuing)");
            let exit = exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let _ = merge_logger.log(&format!(
                "PR creation failed: exit_code={} stdout='{}' stderr='{}'",
                exit,
                stdout.trim(),
                stderr.trim()
            ));
            if let Err(e) = chat::write_message(
                chat_file,
                "ScrumMaster",
                "PR: failed to create (continuing)",
            ) {
                eprintln!("  warning: failed to write PR failure to chat: {}", e);
            }
        }
    }
}

fn should_push_target_branch(
    target_branch_explicit: bool,
    sprint_branch: &str,
    target_branch: &str,
    shutdown_requested: bool,
) -> bool {
    push_skip_reason(
        target_branch_explicit,
        sprint_branch,
        target_branch,
        shutdown_requested,
    )
    .is_none()
}

/// Run a single sprint.
///
/// The `session_sprint_number` is the sprint number within this run session (1, 2, 3...).
/// The historical sprint number (used in commits) is loaded from sprint-history.json.
pub(crate) fn run_sprint(
    config: &Config,
    session_sprint_number: usize,
    run_instance: &str,
) -> Result<SprintResult, String> {
    // Resolve runtime state namespace and determine sprint number (peek, don't write yet).
    let team_name = project_name_for_config(config);
    let source_branch = config
        .source_branch
        .as_deref()
        .ok_or_else(|| "source branch not configured".to_string())?;
    let target_branch = config
        .target_branch
        .as_deref()
        .ok_or_else(|| "target branch not configured".to_string())?;
    let repo_root = git_repo_root()?;
    let runtime_paths =
        team::RuntimeStatePaths::for_branches(&team_name, source_branch, target_branch);

    // Start each `swarm run` invocation with a fresh runtime namespace for the
    // target branch to avoid stale cache/state artifacts across reruns.
    if session_sprint_number == 1 && runtime_paths.is_namespaced() {
        reset_runtime_namespace_for_new_run(&repo_root, &runtime_paths)?;
    }

    // Validate that source branch exists before proceeding.
    // This gives a clear error when a non-existent source branch is specified.
    ensure_branch_exists(&repo_root, source_branch)?;

    sync_target_branch_state(
        &repo_root,
        source_branch,
        target_branch,
        &team_name,
        config,
        &runtime_paths,
    )?;

    // Load tasks from runtime-scoped state.
    let runtime_tasks_path = runtime_paths.tasks_path();
    let runtime_history_path = runtime_paths.sprint_history_path();
    let runtime_state_path = runtime_paths.team_state_path();

    let content = fs::read_to_string(&runtime_tasks_path)
        .map_err(|e| format!("failed to read {}: {}", runtime_tasks_path.display(), e))?;
    let mut task_list = TaskList::parse(&content);

    let mut sprint_history = team::SprintHistory::load_from(&runtime_history_path)?;
    if sprint_history.team_name == "unknown" {
        sprint_history.team_name = team_name.clone();
    }
    let historical_sprint = sprint_history.peek_next_sprint();
    let formatted_team = sprint_history.formatted_team_name();

    // Unassign any incomplete tasks from previous sprints so they can be reassigned fresh.
    // Keep this in-memory to avoid dirtying the target branch worktree.
    task_list.unassign_all();

    // Determine how many agents to spawn
    let assignable = task_list.assignable_count();
    if assignable == 0 {
        return Ok(SprintResult {
            tasks_assigned: 0,
            tasks_completed: 0,
            tasks_failed: 0,
        });
    }

    let tasks_per_agent = config.agents_tasks_per_agent;
    let agents_needed = assignable.div_ceil(tasks_per_agent);
    let agent_cap = agents_needed.min(config.agents_max_count);
    // With project-namespaced worktrees, all agents are available for any project
    let initials: Vec<char> = INITIALS.iter().take(agent_cap).copied().collect();
    if initials.is_empty() {
        println!("No agents available.");
        return Ok(SprintResult {
            tasks_assigned: 0,
            tasks_completed: 0,
            tasks_failed: 0,
        });
    }
    let agent_count = initials.len();

    // Assign tasks via LLM planning (with fallback to algorithmic)
    let engine = engine::create_engine(
        config.effective_engine(),
        &config.files_log_dir,
        config.agent_timeout_secs,
    );
    let log_dir = Path::new(&config.files_log_dir);

    if let Err(e) =
        chat::write_message(&config.files_chat, "ScrumMaster", "Sprint planning started")
    {
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
        return Ok(SprintResult {
            tasks_assigned: 0,
            tasks_completed: 0,
            tasks_failed: 0,
        });
    }

    // Create run context for namespaced artifacts (worktrees, branches)
    // This is created early so the sprint branch uses the run hash
    let run_ctx = RunContext::new_for_run(
        &team_name,
        target_branch,
        run_instance,
        historical_sprint as u32,
    );

    // Log run hash at sprint start for visibility
    println!(
        "{} {} Sprint {} (runtime {}, run {}): starting",
        emoji::SPRINT,
        color::info(&formatted_team),
        color::number(historical_sprint),
        color::info(run_ctx.runtime_id()),
        color::info(run_ctx.hash())
    );

    // Compute sprint branch name using run context (includes run hash)
    let sprint_branch = run_ctx.sprint_branch();
    let sprint_base_branch = resolve_sprint_base_branch(&repo_root, source_branch, target_branch)?;
    let worktrees_dir = Path::new(&config.files_worktrees_dir);

    let base_commit = get_short_commit_for_ref_in(&repo_root, &sprint_base_branch)
        .or_else(|| get_short_commit_for_ref_in(&repo_root, "HEAD"))
        .unwrap_or_else(|| "unknown".to_string());
    if let Err(e) = chat::write_message(
        &config.files_chat,
        "ScrumMaster",
        &format!(
            "Creating worktree {} from {} ({})",
            sprint_branch, sprint_base_branch, base_commit
        ),
    ) {
        eprintln!("warning: failed to write chat: {}", e);
    }

    // Create sprint branch/worktree FIRST, before any file writes
    // This ensures all sprint setup files are written to the sprint worktree,
    // not the target branch (main/master)

    // Clean up any existing feature worktree from a failed previous sprint.
    // This ensures we start fresh from the source branch for this run.
    if let Err(e) = worktree::cleanup_feature_worktree(worktrees_dir, &sprint_branch, true) {
        // Log but don't fail - the worktree might not exist
        eprintln!("  note: pre-sprint feature worktree cleanup: {}", e);
    }

    let feature_worktree_path =
        create_sprint_worktree_in(worktrees_dir, &sprint_branch, &sprint_base_branch)?;

    // Print sprint start banner (after worktree creation to ensure we have a valid sprint)
    print_sprint_start_banner(&formatted_team, historical_sprint);

    // Construct the sprint worktree swarm directory path.
    let worktree_swarm_dir = feature_worktree_path
        .join(team::SWARM_HUG_DIR)
        .join(&team_name);

    // Ensure worktree swarm dir exists.
    let worktree_tasks_path = worktree_swarm_dir.join("tasks.md");
    fs::create_dir_all(&worktree_swarm_dir)
        .map_err(|e| format!("failed to create worktree swarm dir: {}", e))?;
    update_runtime_feature_branch(&runtime_state_path, &team_name, Some(&sprint_branch))?;

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

    // Persist runtime-scoped planning tasks for this target branch.
    if runtime_paths.is_namespaced() {
        persist_runtime_tasks_file(&worktree_tasks_path, &runtime_tasks_path)?;
    }

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
    let assignments_ref: Vec<(char, &str)> =
        assignments.iter().map(|(i, d)| (*i, d.as_str())).collect();
    chat::write_sprint_plan(&config.files_chat, historical_sprint, &assignments_ref)
        .map_err(|e| format!("failed to write chat: {}", e))?;

    // Commit assignment changes to git so worktrees can see them.
    commit_task_assignments(
        &feature_worktree_path,
        &sprint_branch,
        worktree_tasks_path.to_str().unwrap_or(""),
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
        worktree::create_worktrees_in(worktrees_dir, &assignments, &sprint_branch, &run_ctx)
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
    let merge_failures: Arc<Mutex<Vec<MergeFailureInfo>>> = Arc::new(Mutex::new(Vec::new()));

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

    // Always pass canonical team directory to engines. Runtime tasks may be
    // namespaced under runs/<target>, but prompt-derived
    // team-state/worktree paths should resolve from .swarm-hug/<team>.
    let team_dir = Some(engine_team_dir(&team_name, &config.files_tasks));

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
        let merge_failures = Arc::clone(&merge_failures);
        let run_ctx = run_ctx.clone();
        let repo_root = repo_root.clone();
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
                let engine_type_str = selected_engine_type.as_str();
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
                if let Err(e) = logger.log(&format!(
                    "Assigned task: {} [engine: {}]",
                    description, engine_type_str
                )) {
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
                if let Err(e) = chat::write_message(
                    &chat_path,
                    agent_name,
                    &format!("Starting: {} [engine: {}]", description, engine_type_str),
                ) {
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
                    if let Err(e) = logger.log(&format!(
                        "Engine error: {} (exit code: {})",
                        err, result.exit_code
                    )) {
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

                    if let Err(e) = logger.log(&format!(
                        "Task completed: {} [engine: {}]",
                        description, engine_type_str
                    )) {
                        eprintln!("warning: failed to write log: {}", e);
                    }

                    if let Err(e) = chat::write_message(
                        &chat_path,
                        agent_name,
                        &format!("Completed: {}", description),
                    ) {
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
                    let mut merge_result = {
                        let _guard = worktree_lock.lock().unwrap();
                        worktree::merge_agent_branch_in_with_ctx(
                            &feature_worktree_path,
                            &run_ctx,
                            initial,
                            Some(&sprint_branch),
                        )
                    };
                    let mut recreate_context: Option<(String, String)> = None;
                    if matches!(merge_result, worktree::MergeResult::NoBranch) {
                        let expected_branch = run_ctx.agent_branch(initial);
                        let head_commit = get_current_commit_in(&working_dir);
                        let head_short = get_short_commit_for_ref_in(&working_dir, "HEAD")
                            .unwrap_or_else(|| "unknown".to_string());
                        recreate_context = Some((expected_branch.clone(), head_short.clone()));
                        if let Some(commit) = head_commit {
                            if let Err(e) = logger.log(&format!(
                                "Missing branch {}. Recreating from HEAD {}...",
                                expected_branch, head_short
                            )) {
                                eprintln!("warning: failed to write log: {}", e);
                            }
                            let recreate_result = {
                                let _guard = worktree_lock.lock().unwrap();
                                create_branch_at_commit(
                                    &feature_worktree_path,
                                    &expected_branch,
                                    &commit,
                                )
                            };
                            match recreate_result {
                                Ok(()) => {
                                    let retry_result = {
                                        let _guard = worktree_lock.lock().unwrap();
                                        worktree::merge_agent_branch_in_with_ctx(
                                            &feature_worktree_path,
                                            &run_ctx,
                                            initial,
                                            Some(&sprint_branch),
                                        )
                                    };
                                    merge_result = retry_result;
                                }
                                Err(err) => {
                                    let detail = format!(
                                        "agent branch '{}' not found (HEAD {}) and recreate failed: {}",
                                        expected_branch, head_short, err
                                    );
                                    merge_result = worktree::MergeResult::Error(detail);
                                }
                            }
                        } else {
                            let detail = format!(
                                "agent branch '{}' not found and HEAD commit unavailable",
                                expected_branch
                            );
                            merge_result = worktree::MergeResult::Error(detail);
                        }
                    }
                    if let (Some((branch, head_short)), worktree::MergeResult::NoBranch) =
                        (&recreate_context, &merge_result)
                    {
                        merge_result = worktree::MergeResult::Error(format!(
                            "agent branch '{}' still missing after recreate (HEAD {})",
                            branch, head_short
                        ));
                    }

                    if matches!(merge_result, worktree::MergeResult::Conflict(_))
                        && engine.engine_type() != EngineType::Stub
                    {
                        let conflict_detail = match &merge_result {
                            worktree::MergeResult::Conflict(files) => {
                                if files.is_empty() {
                                    "conflicts detected".to_string()
                                } else {
                                    format!("conflicts in {}", files.join(", "))
                                }
                            }
                            _ => "conflicts detected".to_string(),
                        };
                        let agent_branch = run_ctx.agent_branch(initial);
                        if let Err(e) = logger.log("Merge conflict detected; invoking merge agent")
                        {
                            eprintln!("warning: failed to write log: {}", e);
                        }
                        let conflict_msg = format!(
                            "Merge conflict for {} detected. Invoking merge agent.",
                            agent_name
                        );
                        if let Err(e) =
                            chat::write_message(&chat_path, "ScrumMaster", &conflict_msg)
                        {
                            eprintln!("warning: failed to write chat: {}", e);
                        }

                        let merge_attempt = {
                            let _guard = worktree_lock.lock().unwrap();
                            merge_agent::run_merge_agent_in_worktree(
                                engine.as_ref(),
                                &agent_branch,
                                &sprint_branch,
                                &feature_worktree_path,
                            )
                        };

                        match merge_attempt {
                            Ok(result) => {
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
                                    if let Err(e) = logger
                                        .log(&format!("Merge agent output:\n{}", output_preview))
                                    {
                                        eprintln!("warning: failed to write log: {}", e);
                                    }
                                }
                                if let Some(err) = result.error.as_deref() {
                                    if let Err(e) =
                                        logger.log(&format!("Merge agent error: {}", err))
                                    {
                                        eprintln!("warning: failed to write log: {}", e);
                                    }
                                }

                                if result.success {
                                    match merge_agent::ensure_feature_merged(
                                        engine.as_ref(),
                                        &agent_branch,
                                        &sprint_branch,
                                        &feature_worktree_path,
                                    ) {
                                        Ok(()) => {
                                            merge_result = worktree::MergeResult::Success;
                                            if let Err(e) =
                                                logger.log("Merge agent resolved conflicts")
                                            {
                                                eprintln!("warning: failed to write log: {}", e);
                                            }
                                            let resolved_msg = format!(
                                                "Merge conflicts resolved for {}.",
                                                agent_name
                                            );
                                            if let Err(e) = chat::write_message(
                                                &chat_path,
                                                "ScrumMaster",
                                                &resolved_msg,
                                            ) {
                                                eprintln!("warning: failed to write chat: {}", e);
                                            }
                                        }
                                        Err(e) => {
                                            merge_result = worktree::MergeResult::Error(format!(
                                                "merge agent failed after {}: {}",
                                                conflict_detail, e
                                            ));
                                        }
                                    }
                                } else {
                                    let err = result
                                        .error
                                        .unwrap_or_else(|| "merge agent failed".to_string());
                                    merge_result = worktree::MergeResult::Error(format!(
                                        "merge agent failed after {}: {}",
                                        conflict_detail, err
                                    ));
                                }
                            }
                            Err(e) => {
                                merge_result = worktree::MergeResult::Error(format!(
                                    "merge agent failed after {}: {}",
                                    conflict_detail, e
                                ));
                            }
                        }
                    }

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
                            let expected_branch = run_ctx.agent_branch(initial);
                            merge_error_detail =
                                Some(format!("agent branch not found: {}", expected_branch));
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
                            worktree::cleanup_agent_worktree(
                                &worktrees_dir,
                                initial,
                                true,
                                &run_ctx,
                            )
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

                    let mut preserve_outcome = PreserveOutcome {
                        path: working_dir.clone(),
                        allow_recreate: true,
                        error: None,
                    };

                    if let Some(detail) = merge_error_detail.as_ref() {
                        if let Err(e) = logger.log(&format!("Merge failed: {}", detail)) {
                            eprintln!("warning: failed to write log: {}", e);
                        }
                        if let Err(e) = write_merge_failure_chat(&chat_path, agent_name, detail) {
                            eprintln!("warning: failed to write chat: {}", e);
                        }
                        let branch = run_ctx.agent_branch(initial);
                        let log_path = log::log_file_path(Path::new(&log_dir), initial)
                            .display()
                            .to_string();

                        preserve_outcome = {
                            let _guard = worktree_lock.lock().unwrap();
                            preserve_failed_worktree(
                                &repo_root,
                                &worktrees_dir,
                                &working_dir,
                                &branch,
                                task_index,
                            )
                        };

                        if let Some(err) = preserve_outcome.error.as_ref() {
                            if let Err(e) = logger.log(&format!("Preserve failed: {}", err)) {
                                eprintln!("warning: failed to write log: {}", e);
                            }
                        }

                        let preserve_msg = if let Some(err) = preserve_outcome.error.as_ref() {
                            format!(
                                "Preserving {} worktree at {} (branch {}, log {}). Unable to prepare a fresh worktree from sprint head: {}. Remaining tasks will be skipped.",
                                agent_name,
                                preserve_outcome.path.display(),
                                branch,
                                log_path,
                                err
                            )
                        } else {
                            format!(
                                "Preserving {} worktree at {} (branch {}, log {}). Continuing with a fresh worktree from sprint head for remaining tasks.",
                                agent_name,
                                preserve_outcome.path.display(),
                                branch,
                                log_path
                            )
                        };
                        if let Err(e) = logger.log(&preserve_msg) {
                            eprintln!("warning: failed to write log: {}", e);
                        }
                        if let Err(e) =
                            chat::write_message(&chat_path, "ScrumMaster", &preserve_msg)
                        {
                            eprintln!("warning: failed to write chat: {}", e);
                        }
                        if let Ok(mut failures) = merge_failures.lock() {
                            failures.push(MergeFailureInfo {
                                initial,
                                agent_name: agent_name.to_string(),
                                branch,
                                worktree_path: preserve_outcome.path.display().to_string(),
                                log_path,
                                detail: detail.clone(),
                                skip_cleanup: preserve_outcome.error.is_some(),
                            });
                        }
                    }

                    if let Some(msg) = merge_error {
                        success = false;
                        error = Some(msg);
                        allow_recreate = preserve_outcome.allow_recreate;
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
        println!(
            "Waiting for {} agent(s) to finish current work...",
            total_agents
        );
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
            if *success {
                duration.as_ref().copied()
            } else {
                None
            }
        })
        .collect();

    let completion = reconcile_sprint_tasks_from_git(
        &feature_worktree_path,
        &sprint_start_commit,
        &assignments,
        &results,
        engine.engine_type() == EngineType::Stub,
        &mut task_list,
    )?;
    let completed_this_sprint = completion.completed;
    let failed_this_sprint = completion.failed;

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

    let merge_failures_snapshot = merge_failures
        .lock()
        .map(|failures| failures.clone())
        .unwrap_or_default();
    let (cleanup_initials, skipped_initials) =
        split_cleanup_initials(&assigned_initials, &merge_failures_snapshot);

    if !merge_failures_snapshot.is_empty() {
        if !skipped_initials.is_empty() {
            println!(
                "  Post-sprint cleanup: skipping {} agent worktree(s) due to merge failures",
                skipped_initials.len()
            );
        }
        for failure in &merge_failures_snapshot {
            println!(
                "  Merge failure preserved: {} ({}) branch {} at {}",
                failure.agent_name, failure.initial, failure.branch, failure.worktree_path
            );
            println!(
                "  Merge failure detail: {} (log: {})",
                failure.detail, failure.log_path
            );
        }
    }

    // Clean up worktrees after sprint completes
    // This ensures worktrees are recreated fresh from the feature branch on the next sprint
    let cleanup_summary = worktree::cleanup_agent_worktrees(
        worktrees_dir,
        &cleanup_initials,
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

    // Persist final runtime-scoped planning tasks (including follow-up tasks if added).
    if runtime_paths.is_namespaced() {
        persist_runtime_tasks_file(&worktree_tasks_path, &runtime_tasks_path)?;
    }

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

    let mut sprint_state_committed = false;

    // Merge sprint branch into target branch via merge agent.
    if shutdown::requested() {
        println!("  Skipping merge agent due to shutdown.");
    } else if sprint_branch == target_branch {
        println!("  Skipping merge agent: feature branch matches target branch.");
        sprint_state_committed = true;
    } else {
        let merge_logger = NamedLogger::new(
            Path::new(&config.files_log_dir),
            "MergeAgent",
            "merge-agent.log",
        );
        println!(
            "  Merge agent: starting ({} -> {})",
            sprint_branch, target_branch
        );
        let merge_msg = format!(
            "Merge agent: starting ({} -> {})",
            sprint_branch, target_branch
        );
        if let Err(e) = chat::write_message(&config.files_chat, "ScrumMaster", &merge_msg) {
            eprintln!("  warning: failed to write merge start to chat: {}", e);
        }
        if let Err(e) = merge_logger.log(&format!(
            "Starting merge: {} -> {}",
            sprint_branch, target_branch
        )) {
            eprintln!("  warning: failed to write merge log: {}", e);
        }
        let merge_engine = engine.engine_type().as_str();
        if let Err(e) = merge_logger.log(&format!("Engine: {}", merge_engine)) {
            eprintln!("  warning: failed to write merge log: {}", e);
        }
        let merge_cleanup_paths = vec![worktree_tasks_path.clone()];
        if let Err(e) =
            merge_agent::prepare_merge_workspace(&feature_worktree_path, &merge_cleanup_paths)
        {
            let _ = merge_logger.log(&format!("Prepare workspace failed: {}", e));
            return Err(format!("merge agent failed: {}", e));
        }
        if let Err(e) = merge_logger.log("Workspace prepared") {
            eprintln!("  warning: failed to write merge log: {}", e);
        }
        let merge_result = merge_agent::run_merge_agent(
            engine.as_ref(),
            &sprint_branch,
            target_branch,
            &feature_worktree_path,
        )
        .map_err(|e| {
            let _ = merge_logger.log(&format!("Merge agent execution failed: {}", e));
            format!("merge agent failed: {}", e)
        })?;
        if !merge_result.output.is_empty() {
            let output_preview = if merge_result.output.len() > 1000 {
                format!(
                    "{}... [truncated, {} bytes total]",
                    &merge_result.output[..1000],
                    merge_result.output.len()
                )
            } else {
                merge_result.output.clone()
            };
            if let Err(e) = merge_logger.log(&format!("Engine output:\n{}", output_preview)) {
                eprintln!("  warning: failed to write merge log: {}", e);
            }
        }
        if let Err(e) = merge_logger.log(&format!(
            "Engine result: {} (exit_code={})",
            if merge_result.success {
                "success"
            } else {
                "failure"
            },
            merge_result.exit_code
        )) {
            eprintln!("  warning: failed to write merge log: {}", e);
        }
        if let Some(err) = merge_result.error.as_deref() {
            if let Err(e) = merge_logger.log(&format!("Engine error: {}", err)) {
                eprintln!("  warning: failed to write merge log: {}", e);
            }
        }
        if merge_result.success {
            if let Err(e) = merge_agent::run_merge_agent_with_retry(
                engine.as_ref(),
                &sprint_branch,
                target_branch,
                &feature_worktree_path,
            ) {
                let _ = merge_logger.log(&format!("Merge verification failed (with retry): {}", e));
                return Err(format!("merge agent failed: {}", e));
            }
            println!("  Merge agent: completed");
            if let Err(e) =
                chat::write_message(&config.files_chat, "ScrumMaster", "Merge agent: completed")
            {
                eprintln!("  warning: failed to write merge complete to chat: {}", e);
            }
            if let Err(e) = merge_logger.log("Merge completed") {
                eprintln!("  warning: failed to write merge log: {}", e);
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
                            let _ = merge_logger.log("Stub merge failed: feature branch not found");
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
                            let _ = merge_logger.log(&format!("Stub merge conflict: {}", detail));
                            return Err(format!("merge agent failed: {}", detail));
                        }
                        worktree::MergeResult::Error(e) => {
                            let _ = merge_logger.log(&format!("Stub merge error: {}", e));
                            return Err(format!("merge agent failed: {}", e));
                        }
                    }
                } else {
                    let _ = merge_logger.log("Merge agent did not merge feature into target");
                    return Err(format!(
                        "merge agent did not merge '{}' into '{}'",
                        sprint_branch, target_branch
                    ));
                }
            }

            if merged_ok {
                let mut push_succeeded = false;
                let skip_reason = push_skip_reason(
                    config.target_branch_explicit,
                    &sprint_branch,
                    target_branch,
                    shutdown::requested(),
                );
                if let Some(reason) = skip_reason {
                    let push_msg = format!("Push: skipped ({})", reason);
                    println!("  {}", push_msg);
                    let _ = merge_logger.log(&push_msg);
                    if let Err(e) = write_push_outcome_chat(&config.files_chat, &push_msg) {
                        eprintln!("  warning: failed to write push status to chat: {}", e);
                    }
                } else if should_push_target_branch(
                    config.target_branch_explicit,
                    &sprint_branch,
                    target_branch,
                    shutdown::requested(),
                ) {
                    let push_result = push_branch_to_remote(&repo_root, target_branch);
                    if push_result.success {
                        push_succeeded = true;
                        let push_msg = format!("Push: pushed '{}' to origin", target_branch);
                        println!("  {}", push_msg);
                        let _ = merge_logger.log(&format!("Push succeeded: {}", target_branch));
                        if let Err(e) = write_push_outcome_chat(&config.files_chat, &push_msg) {
                            eprintln!("  warning: failed to write push status to chat: {}", e);
                        }
                    } else {
                        eprintln!(
                            "  warning: failed to push '{}' to origin (continuing)",
                            target_branch
                        );
                        let push_msg = format!(
                            "Push: failed to push '{}' to origin (continuing)",
                            target_branch
                        );
                        let error = push_result.error.as_deref().unwrap_or("unknown error");
                        let stdout = push_result.stdout.trim();
                        let stderr = push_result.stderr.trim();
                        let _ = merge_logger.log(&format!(
                            "Push failed for '{}': error='{}' exit_code={:?} stdout='{}' stderr='{}'",
                            target_branch, error, push_result.exit_code, stdout, stderr
                        ));
                        if let Err(e) = write_push_outcome_chat(&config.files_chat, &push_msg) {
                            eprintln!("  warning: failed to write push status to chat: {}", e);
                        }
                    }
                }

                if push_succeeded {
                    let pr_team_dir = engine_team_dir(&team_name, &config.files_tasks);
                    let (pr_title, pr_body) = generate_pr_title_and_body(
                        engine.as_ref(),
                        &repo_root,
                        &feature_worktree_path,
                        session_sprint_number,
                        Some(pr_team_dir.as_str()),
                        source_branch,
                        target_branch,
                        &merge_logger,
                    );
                    let _ = merge_logger.log(&format!(
                        "PR metadata prepared: title='{}' body_chars={}",
                        pr_title,
                        pr_body.len()
                    ));
                    let pr_result =
                        create_pull_request(&pr_title, &pr_body, source_branch, target_branch);
                    report_pull_request_creation(pr_result, &merge_logger, &config.files_chat);
                }

                if let Err(e) =
                    worktree::cleanup_feature_worktree(worktrees_dir, &sprint_branch, true)
                {
                    eprintln!("  warning: feature worktree cleanup failed: {}", e);
                    let _ = merge_logger.log(&format!("Feature cleanup failed: {}", e));
                } else {
                    println!("  Feature cleanup: removed '{}'", sprint_branch);
                    let _ =
                        merge_logger.log(&format!("Feature cleanup: removed '{}'", sprint_branch));
                }
                sprint_state_committed = true;
            }
        } else {
            let detail = merge_result
                .error
                .unwrap_or_else(|| "unknown error".to_string());
            println!("  Merge agent: failed");
            if let Err(e) = chat::write_message(
                &config.files_chat,
                "ScrumMaster",
                &format!("Merge agent: failed ({})", detail),
            ) {
                eprintln!("  warning: failed to write merge failure to chat: {}", e);
            }
            let _ = merge_logger.log(&format!("Merge failed: {}", detail));
            return Err(format!("merge agent failed: {}", detail));
        }
    }

    if sprint_state_committed {
        finalize_runtime_state_after_sprint(
            &runtime_history_path,
            &runtime_state_path,
            &team_name,
        )?;
    }

    Ok(SprintResult {
        tasks_assigned: assigned,
        tasks_completed: completed_this_sprint,
        tasks_failed: failed_this_sprint,
    })
}

fn reset_runtime_namespace_for_new_run(
    repo_root: &Path,
    runtime_paths: &team::RuntimeStatePaths,
) -> Result<(), String> {
    if !runtime_paths.is_namespaced() {
        return Ok(());
    }

    let runtime_root = repo_root.join(runtime_paths.root());
    if !runtime_root.exists() {
        return Ok(());
    }

    fs::remove_dir_all(&runtime_root).map_err(|e| {
        format!(
            "failed to reset runtime state {}: {}",
            runtime_root.display(),
            e
        )
    })
}

fn sync_target_branch_state(
    repo_root: &Path,
    source_branch: &str,
    target_branch: &str,
    team_name: &str,
    config: &Config,
    runtime_paths: &team::RuntimeStatePaths,
) -> Result<(), String> {
    // Runtime state is scoped under `.swarm-hug/<team>/runs/<target>/`.
    // Bootstrap tasks from target branch once; keep history/state local to
    // runtime namespace to avoid branch-tracked state conflicts.
    if runtime_paths.is_namespaced() {
        let runtime_tasks = repo_root.join(runtime_paths.tasks_path());
        let runtime_history = repo_root.join(runtime_paths.sprint_history_path());
        let runtime_state = repo_root.join(runtime_paths.team_state_path());

        if !runtime_tasks.exists() {
            let branch_tasks_rel = runtime_paths.branch_tasks_path();
            let configured_tasks_rel = Path::new(&config.files_tasks);

            if branch_is_checked_out(repo_root, target_branch)? {
                let src_tasks = repo_root.join(&branch_tasks_rel);
                copy_if_missing(&src_tasks, &runtime_tasks)?;
                if !runtime_tasks.exists() && configured_tasks_rel.is_relative() {
                    copy_if_missing(&repo_root.join(configured_tasks_rel), &runtime_tasks)?;
                }
            } else {
                let target_worktree_preexisting =
                    worktree::find_target_branch_worktree_in(repo_root, target_branch)?;
                let target_worktree =
                    worktree::create_target_branch_worktree_in(repo_root, target_branch)?;
                let src_tasks = target_worktree.join(&branch_tasks_rel);
                copy_if_missing(&src_tasks, &runtime_tasks)?;
                if !runtime_tasks.exists() && configured_tasks_rel.is_relative() {
                    copy_if_missing(&target_worktree.join(configured_tasks_rel), &runtime_tasks)?;
                }
                if target_worktree_preexisting.is_none() {
                    remove_worktree_path(repo_root, &target_worktree)?;
                }
            }
        }

        if !runtime_history.exists() {
            let mut history = team::SprintHistory::load_from(&runtime_history)?;
            if history.team_name == "unknown" {
                history.team_name = team_name.to_string();
            }
            history.save()?;
        }

        if !runtime_state.exists() {
            let mut state = team::TeamState::load_from(&runtime_state)?;
            state.team_name = team_name.to_string();
            state.clear_feature_branch();
            state.save()?;
        }

        return Ok(());
    }

    // Legacy behavior: sync task list and sprint history from source_branch.
    if branch_is_checked_out(repo_root, source_branch)? {
        return Ok(());
    }

    let source_worktree = worktree::create_target_branch_worktree_in(repo_root, source_branch)?;

    let tasks_path = Path::new(&config.files_tasks);
    if tasks_path.is_relative() {
        let src = source_worktree.join(tasks_path);
        let dst = repo_root.join(tasks_path);
        copy_if_exists(&src, &dst)?;
    }

    let sprint_rel = Path::new(team::SWARM_HUG_DIR)
        .join(team_name)
        .join(team::SPRINT_HISTORY_FILE);
    let src = source_worktree.join(&sprint_rel);
    let dst = repo_root.join(&sprint_rel);
    copy_if_exists(&src, &dst)?;

    // If source and target differ, also ensure the target branch worktree exists
    // so that later merge operations have a valid target.
    if source_branch != target_branch && !branch_is_checked_out(repo_root, target_branch)? {
        worktree::create_target_branch_worktree_in(repo_root, target_branch)?;
    }

    Ok(())
}

fn ensure_branch_exists(repo_root: &Path, branch: &str) -> Result<(), String> {
    let ref_name = format!("refs/heads/{}", branch);
    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["show-ref", "--verify", "--quiet", &ref_name])
        .output()
        .map_err(|e| format!("failed to run git show-ref: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "source branch '{}' does not exist. Check the branch name and try again.",
            branch
        ))
    }
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

    fs::copy(src, dst).map(|_| ()).map_err(|e| {
        format!(
            "failed to copy {} to {}: {}",
            src.display(),
            dst.display(),
            e
        )
    })
}

fn copy_if_missing(src: &Path, dst: &Path) -> Result<(), String> {
    if dst.exists() {
        return Ok(());
    }
    copy_if_exists(src, dst)
}

fn remove_worktree_path(repo_root: &Path, worktree_path: &Path) -> Result<(), String> {
    let path_str = worktree_path.to_string_lossy().to_string();
    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "remove", "--force", &path_str])
        .output()
        .map_err(|e| format!("failed to run git worktree remove: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "git worktree remove failed for {}: {}",
            worktree_path.display(),
            stderr.trim()
        ))
    }
}

fn persist_runtime_tasks_file(
    worktree_tasks_path: &Path,
    runtime_tasks_path: &Path,
) -> Result<(), String> {
    copy_if_exists(worktree_tasks_path, runtime_tasks_path)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SprintCompletionSummary {
    completed: usize,
    failed: usize,
}

#[derive(Debug, Default)]
struct SprintCommitEvidence {
    subject_counts: std::collections::HashMap<String, usize>,
    merge_counts_by_initial: std::collections::HashMap<char, usize>,
    authored_counts_by_initial: std::collections::HashMap<char, usize>,
    has_any_changes: bool,
}

fn parse_agent_initial_from_email(author_email: &str) -> Option<char> {
    let normalized = author_email.trim().to_ascii_lowercase();
    let local = normalized.strip_suffix("@swarm.local")?;
    let suffix = local.strip_prefix("agent-")?;
    if suffix.len() != 1 {
        return None;
    }

    let initial = suffix.chars().next()?.to_ascii_uppercase();
    if agent::is_valid_initial(initial) {
        Some(initial)
    } else {
        None
    }
}

fn collect_sprint_commit_evidence_in_range(
    repo_dir: &Path,
    from: &str,
    to: &str,
) -> Result<SprintCommitEvidence, String> {
    let range = format!("{}..{}", from.trim(), to.trim());
    let log_output = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["log", "--format=%s%x1f%ae%x1f%P", &range])
        .output()
        .map_err(|e| format!("failed to run git log for sprint reconciliation: {}", e))?;

    if !log_output.status.success() {
        let stderr = String::from_utf8_lossy(&log_output.stderr);
        return Err(format!(
            "git log failed for sprint reconciliation: {}",
            stderr.trim()
        ));
    }

    let mut evidence = SprintCommitEvidence::default();
    let stdout = String::from_utf8_lossy(&log_output.stdout);
    for line in stdout.lines() {
        let mut parts = line.splitn(3, '\x1f');
        let subject = parts.next().unwrap_or("").trim();
        let author_email = parts.next().unwrap_or("").trim();
        let parents = parts.next().unwrap_or("").trim();

        if subject.is_empty() {
            continue;
        }

        *evidence
            .subject_counts
            .entry(subject.to_string())
            .or_insert(0) += 1;

        let Some(initial) = parse_agent_initial_from_email(author_email) else {
            continue;
        };

        let parent_count = parents.split_whitespace().count();
        let is_merge = parent_count > 1 || subject.starts_with("Merge ");
        let counter = if is_merge {
            &mut evidence.merge_counts_by_initial
        } else {
            &mut evidence.authored_counts_by_initial
        };
        *counter.entry(initial).or_insert(0) += 1;
    }

    let diff_output = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["diff", "--quiet", &range])
        .output()
        .map_err(|e| format!("failed to run git diff for sprint reconciliation: {}", e))?;

    evidence.has_any_changes = if diff_output.status.success() {
        false
    } else if diff_output.status.code() == Some(1) {
        true
    } else {
        let stderr = String::from_utf8_lossy(&diff_output.stderr);
        return Err(format!(
            "git diff failed for sprint reconciliation: {}",
            stderr.trim()
        ));
    };

    Ok(evidence)
}

fn reconcile_sprint_tasks_from_git(
    feature_worktree_path: &Path,
    sprint_start_commit: &str,
    assignments: &[(char, String)],
    results: &[TaskResult],
    allow_success_fallback: bool,
    task_list: &mut TaskList,
) -> Result<SprintCompletionSummary, String> {
    if assignments.is_empty() {
        return Ok(SprintCompletionSummary::default());
    }

    let mut evidence = collect_sprint_commit_evidence_in_range(
        feature_worktree_path,
        sprint_start_commit,
        "HEAD",
    )?;
    let mut success_counts_by_assignment: std::collections::HashMap<(char, String), usize> =
        std::collections::HashMap::new();
    let mut success_counts_by_initial: std::collections::HashMap<char, usize> =
        std::collections::HashMap::new();
    for (initial, description, success, _error, _duration) in results {
        if *success {
            *success_counts_by_assignment
                .entry((*initial, description.clone()))
                .or_insert(0) += 1;
            *success_counts_by_initial.entry(*initial).or_insert(0) += 1;
        }
    }

    let mut assigned_count_by_initial: std::collections::HashMap<char, usize> =
        std::collections::HashMap::new();
    for (initial, _description) in assignments {
        *assigned_count_by_initial.entry(*initial).or_insert(0) += 1;
    }

    let mut exact_match_counts_by_assignment: std::collections::HashMap<(char, String), usize> =
        std::collections::HashMap::new();
    let mut exact_match_counts_by_initial: std::collections::HashMap<char, usize> =
        std::collections::HashMap::new();
    for (initial, description) in assignments {
        let agent_name = agent::name_from_initial(*initial).unwrap_or("Unknown");
        let expected_subject = format!("{}: {}", agent_name, description);
        if let Some(count) = evidence.subject_counts.get_mut(&expected_subject) {
            if *count > 0 {
                *count -= 1;
                *exact_match_counts_by_assignment
                    .entry((*initial, description.clone()))
                    .or_insert(0) += 1;
                *exact_match_counts_by_initial.entry(*initial).or_insert(0) += 1;
            }
        }
    }

    let mut completion_quota_by_initial: std::collections::HashMap<char, usize> =
        std::collections::HashMap::new();
    for (initial, assigned_count) in &assigned_count_by_initial {
        let exact_count = exact_match_counts_by_initial
            .get(initial)
            .copied()
            .unwrap_or(0);
        let merge_count = evidence
            .merge_counts_by_initial
            .get(initial)
            .copied()
            .unwrap_or(0);
        let authored_count = evidence
            .authored_counts_by_initial
            .get(initial)
            .copied()
            .unwrap_or(0);
        let mut quota = exact_count.max(merge_count).max(authored_count);
        if quota == 0 && (allow_success_fallback || evidence.has_any_changes) {
            quota = success_counts_by_initial.get(initial).copied().unwrap_or(0);
        }
        completion_quota_by_initial.insert(*initial, quota.min(*assigned_count));
    }

    let mut completion_decisions = vec![false; assignments.len()];

    // First pass: exact subject matches (task commit messages preserved).
    for (index, (initial, description)) in assignments.iter().enumerate() {
        let Some(remaining_quota) = completion_quota_by_initial.get_mut(initial) else {
            continue;
        };
        if *remaining_quota == 0 {
            continue;
        }

        let key = (*initial, description.clone());
        if let Some(count) = exact_match_counts_by_assignment.get_mut(&key) {
            if *count > 0 {
                completion_decisions[index] = true;
                *count -= 1;
                *remaining_quota -= 1;
            }
        }
    }

    // Second pass: tasks that executed successfully this sprint.
    for (index, (initial, description)) in assignments.iter().enumerate() {
        if completion_decisions[index] {
            continue;
        }
        let Some(remaining_quota) = completion_quota_by_initial.get_mut(initial) else {
            continue;
        };
        if *remaining_quota == 0 {
            continue;
        }

        let key = (*initial, description.clone());
        if let Some(count) = success_counts_by_assignment.get_mut(&key) {
            if *count > 0 {
                completion_decisions[index] = true;
                *count -= 1;
                *remaining_quota -= 1;
            }
        }
    }

    // Final pass: consume remaining git-derived quota in assignment order.
    for (index, (initial, _description)) in assignments.iter().enumerate() {
        if completion_decisions[index] {
            continue;
        }
        let Some(remaining_quota) = completion_quota_by_initial.get_mut(initial) else {
            continue;
        };
        if *remaining_quota > 0 {
            completion_decisions[index] = true;
            *remaining_quota -= 1;
        }
    }

    let mut completed = 0usize;
    for (index, (initial, description)) in assignments.iter().enumerate() {
        let task_completed = completion_decisions[index];

        for task in &mut task_list.tasks {
            if let swarm::task::TaskStatus::Assigned(assigned_initial) = task.status {
                if assigned_initial == *initial && task.description == *description {
                    if task_completed {
                        task.complete(*initial);
                        completed += 1;
                    } else {
                        task.unassign();
                    }
                    break;
                }
            }
        }
    }

    Ok(SprintCompletionSummary {
        completed,
        failed: assignments.len().saturating_sub(completed),
    })
}

fn update_runtime_feature_branch(
    runtime_state_path: &Path,
    team_name: &str,
    feature_branch: Option<&str>,
) -> Result<(), String> {
    let mut state = team::TeamState::load_from(runtime_state_path)?;
    if state.team_name == "unknown" || state.team_name.trim().is_empty() {
        state.team_name = team_name.to_string();
    }
    match feature_branch {
        Some(branch) if !branch.trim().is_empty() => state.set_feature_branch(branch)?,
        _ => state.clear_feature_branch(),
    }
    state.save()
}

fn finalize_runtime_state_after_sprint(
    runtime_history_path: &Path,
    runtime_state_path: &Path,
    team_name: &str,
) -> Result<(), String> {
    let mut history = team::SprintHistory::load_from(runtime_history_path)?;
    if history.team_name == "unknown" {
        history.team_name = team_name.to_string();
    }
    history.increment();
    history.save()?;
    update_runtime_feature_branch(runtime_state_path, team_name, None)?;
    Ok(())
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

    if let Err(e) = chat::write_message(&config.files_chat, "ScrumMaster", "Post-mortem started") {
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
                let mut current_content =
                    fs::read_to_string(worktree_tasks_path).unwrap_or_default();

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
                let commit_msg = format!(
                    "{} Sprint {}: follow-up tasks from review",
                    team_name, sprint_number
                );
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

fn write_merge_failure_chat(
    chat_path: &str,
    agent_name: &str,
    detail: &str,
) -> std::io::Result<()> {
    let msg = format!("Merge failed for {}: {}", agent_name, detail);
    chat::write_message(chat_path, "ScrumMaster", &msg)
}

fn write_push_outcome_chat(chat_path: &str, detail: &str) -> std::io::Result<()> {
    chat::write_message(chat_path, "ScrumMaster", detail)
}

fn push_skip_reason(
    target_branch_explicit: bool,
    sprint_branch: &str,
    target_branch: &str,
    shutdown_requested: bool,
) -> Option<&'static str> {
    if !target_branch_explicit {
        Some("target branch was not explicitly provided")
    } else if shutdown_requested {
        Some("shutdown requested")
    } else if sprint_branch == target_branch {
        Some("feature branch matches target branch")
    } else {
        None
    }
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
        .env(
            "GIT_COMMITTER_EMAIL",
            format!("agent-{}@swarm.local", initial),
        )
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
    use super::{
        build_pr_metadata_prompt, chat, create_branch_at_commit, create_sprint_worktree_in,
        default_pr_title, engine_team_dir, ensure_branch_exists, generate_pr_title_and_body,
        parse_pr_metadata_from_engine_output, preserve_failed_worktree, push_skip_reason,
        reconcile_sprint_tasks_from_git, report_pull_request_creation,
        reset_runtime_namespace_for_new_run, resolve_sprint_base_branch, retry_merge_agent,
        should_push_target_branch, split_cleanup_initials, sync_target_branch_state,
        write_merge_failure_chat, write_push_outcome_chat, MergeFailureInfo, SprintResult,
        TaskResult, DEFAULT_PR_BODY,
    };
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use tempfile::NamedTempFile;

    use crate::git::PullRequestCreateResult;
    use crate::testutil::with_temp_cwd;
    use swarm::config::Config;
    use swarm::engine::{Engine, EngineResult};
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
        run_git_in(
            repo_root,
            &["config", "user.email", "swarm-test@example.com"],
        );
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
    fn test_engine_team_dir_uses_canonical_team_root() {
        let path = engine_team_dir("greenfield", ".swarm-hug/greenfield/runs/main/tasks.md");
        assert_eq!(path, ".swarm-hug/greenfield");
    }

    #[test]
    fn test_engine_team_dir_falls_back_to_tasks_parent_when_team_empty() {
        let path = engine_team_dir("", ".swarm-hug/greenfield/runs/main/tasks.md");
        assert_eq!(path, ".swarm-hug/greenfield/runs/main");
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
    fn test_should_push_target_branch_skips_when_sprint_branch_matches_target() {
        let should_push = should_push_target_branch(true, "feature-1", "feature-1", false);
        assert!(
            !should_push,
            "push should be skipped when sprint branch already matches target branch"
        );
    }

    #[test]
    fn test_should_push_target_branch_skips_when_shutdown_requested() {
        let should_push =
            should_push_target_branch(true, "alpha-sprint-1-abc123", "feature-1", true);
        assert!(
            !should_push,
            "push should be skipped when shutdown has been requested"
        );
    }

    #[test]
    fn test_should_push_target_branch_skips_when_target_not_explicit() {
        let should_push =
            should_push_target_branch(false, "alpha-sprint-1-abc123", "feature-1", false);
        assert!(
            !should_push,
            "push should be skipped when --target-branch was not explicitly provided"
        );
    }

    struct CapturingEngine {
        success: bool,
        output: String,
        error: Option<String>,
        captured_prompt: Arc<Mutex<Option<String>>>,
    }

    impl CapturingEngine {
        fn success(output: &str, captured_prompt: Arc<Mutex<Option<String>>>) -> Self {
            Self {
                success: true,
                output: output.to_string(),
                error: None,
                captured_prompt,
            }
        }

        fn failure(error: &str, captured_prompt: Arc<Mutex<Option<String>>>) -> Self {
            Self {
                success: false,
                output: String::new(),
                error: Some(error.to_string()),
                captured_prompt,
            }
        }
    }

    impl Engine for CapturingEngine {
        fn execute(
            &self,
            _agent_name: &str,
            task_description: &str,
            _working_dir: &Path,
            _turn_number: usize,
            _team_dir: Option<&str>,
        ) -> EngineResult {
            if let Ok(mut guard) = self.captured_prompt.lock() {
                *guard = Some(task_description.to_string());
            }
            if self.success {
                EngineResult::success(self.output.clone())
            } else {
                EngineResult::failure(
                    self.error
                        .clone()
                        .unwrap_or_else(|| "engine failed".to_string()),
                    1,
                )
            }
        }

        fn engine_type(&self) -> swarm::config::EngineType {
            swarm::config::EngineType::Claude
        }
    }

    #[test]
    fn test_build_pr_metadata_prompt_includes_range_and_log() {
        let prompt =
            build_pr_metadata_prompt("source-branch", "target-branch", "abc123 target commit");
        assert!(prompt.contains("source-branch"));
        assert!(prompt.contains("target-branch"));
        assert!(prompt.contains("git log --oneline source-branch..target-branch"));
        assert!(prompt.contains("abc123 target commit"));
        assert!(prompt.contains("\"title\""));
        assert!(prompt.contains("\"body\""));
    }

    #[test]
    fn test_parse_pr_metadata_from_engine_output_parses_plain_json() {
        let parsed = parse_pr_metadata_from_engine_output(
            r#"{"title":"[swarm] target-branch","body":"Autogenerated body"}"#,
        )
        .expect("parse PR metadata");
        assert_eq!(parsed.0, "[swarm] target-branch");
        assert_eq!(parsed.1, "Autogenerated body");
    }

    #[test]
    fn test_parse_pr_metadata_from_engine_output_parses_json_with_extra_text() {
        let output = r#"Result:
```json
{"title":"Release prep","body":"Includes sprint changes."}
```"#;
        let parsed =
            parse_pr_metadata_from_engine_output(output).expect("parse metadata from mixed output");
        assert_eq!(parsed.0, "Release prep");
        assert_eq!(parsed.1, "Includes sprint changes.");
    }

    #[test]
    fn test_generate_pr_title_and_body_falls_back_on_parse_failure() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        run_git_in(&repo_root, &["checkout", "-b", "source-branch"]);
        run_git_in(&repo_root, &["checkout", "-b", "target-branch"]);

        let captured_prompt = Arc::new(Mutex::new(None));
        let non_ascii_output = "你".repeat(500);
        let engine = CapturingEngine::success(&non_ascii_output, Arc::clone(&captured_prompt));
        let log_dir = repo_root.join("logs");
        fs::create_dir_all(&log_dir).expect("create logs dir");
        let merge_logger = swarm::log::NamedLogger::new(&log_dir, "MergeAgent", "merge-agent.log");

        let (title, body) = generate_pr_title_and_body(
            &engine,
            &repo_root,
            &repo_root,
            1,
            None,
            "source-branch",
            "target-branch",
            &merge_logger,
        );
        assert_eq!(title, default_pr_title("target-branch"));
        assert_eq!(body, DEFAULT_PR_BODY);

        let log_content = fs::read_to_string(merge_logger.path).expect("read merge log");
        assert!(log_content.contains("PR metadata parse failed; using defaults. output_preview='"));
        assert!(log_content.contains("[truncated, 500 chars total]"));
    }

    #[test]
    fn test_generate_pr_title_and_body_prompt_contains_commit_log_between_branches() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        run_git_in(&repo_root, &["checkout", "-b", "source-branch"]);
        fs::write(repo_root.join("source.txt"), "source").expect("write source file");
        run_git_in(&repo_root, &["add", "."]);
        run_git_in(&repo_root, &["commit", "-m", "source commit"]);

        run_git_in(&repo_root, &["checkout", "-b", "target-branch"]);
        fs::write(repo_root.join("target.txt"), "target").expect("write target file");
        run_git_in(&repo_root, &["add", "."]);
        run_git_in(&repo_root, &["commit", "-m", "target commit"]);

        let captured_prompt = Arc::new(Mutex::new(None));
        let engine = CapturingEngine::success(
            r#"{"title":"PR title","body":"PR body"}"#,
            Arc::clone(&captured_prompt),
        );
        let log_dir = repo_root.join("logs");
        fs::create_dir_all(&log_dir).expect("create logs dir");
        let merge_logger = swarm::log::NamedLogger::new(&log_dir, "MergeAgent", "merge-agent.log");

        let (title, body) = generate_pr_title_and_body(
            &engine,
            &repo_root,
            &repo_root,
            2,
            Some(".swarm-hug/alpha"),
            "source-branch",
            "target-branch",
            &merge_logger,
        );
        assert_eq!(title, "PR title");
        assert_eq!(body, "PR body");

        let prompt = captured_prompt
            .lock()
            .expect("prompt mutex")
            .clone()
            .expect("captured prompt");
        assert!(
            prompt.contains("git log --oneline source-branch..target-branch"),
            "prompt should include commit range, got: {}",
            prompt
        );
        assert!(
            prompt.contains("target commit"),
            "prompt should include oneline commit log output, got: {}",
            prompt
        );
        assert!(
            !prompt.contains("source commit"),
            "prompt should use source..target range and exclude source-only commits, got: {}",
            prompt
        );
    }

    #[test]
    fn test_generate_pr_title_and_body_falls_back_on_engine_failure() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        run_git_in(&repo_root, &["checkout", "-b", "source-branch"]);
        run_git_in(&repo_root, &["checkout", "-b", "target-branch"]);

        let captured_prompt = Arc::new(Mutex::new(None));
        let engine = CapturingEngine::failure("engine unavailable", Arc::clone(&captured_prompt));
        let log_dir = repo_root.join("logs");
        fs::create_dir_all(&log_dir).expect("create logs dir");
        let merge_logger = swarm::log::NamedLogger::new(&log_dir, "MergeAgent", "merge-agent.log");

        let (title, body) = generate_pr_title_and_body(
            &engine,
            &repo_root,
            &repo_root,
            3,
            None,
            "source-branch",
            "target-branch",
            &merge_logger,
        );
        assert_eq!(title, default_pr_title("target-branch"));
        assert_eq!(body, DEFAULT_PR_BODY);
    }

    #[test]
    fn test_report_pull_request_creation_logs_success_url() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let log_dir = temp.path().join("logs");
        fs::create_dir_all(&log_dir).expect("create log dir");
        let merge_logger = swarm::log::NamedLogger::new(&log_dir, "MergeAgent", "merge-agent.log");
        let chat_file = temp.path().join("chat.md");

        report_pull_request_creation(
            PullRequestCreateResult::Created {
                url: Some("https://github.com/example/repo/pull/42".to_string()),
                stdout: String::new(),
                stderr: String::new(),
            },
            &merge_logger,
            chat_file.to_str().expect("chat path"),
        );

        let log_content = fs::read_to_string(merge_logger.path).expect("read merge log");
        assert!(log_content.contains("PR created: https://github.com/example/repo/pull/42"));

        let chat_content = fs::read_to_string(&chat_file).expect("read chat file");
        assert!(chat_content.contains("PR: created https://github.com/example/repo/pull/42"));
    }

    #[test]
    fn test_report_pull_request_creation_logs_skip_warning() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let log_dir = temp.path().join("logs");
        fs::create_dir_all(&log_dir).expect("create log dir");
        let merge_logger = swarm::log::NamedLogger::new(&log_dir, "MergeAgent", "merge-agent.log");
        let chat_file = temp.path().join("chat.md");

        report_pull_request_creation(
            PullRequestCreateResult::Skipped {
                reason: "skipping PR creation: 'gh' was not found on PATH".to_string(),
            },
            &merge_logger,
            chat_file.to_str().expect("chat path"),
        );

        let log_content = fs::read_to_string(merge_logger.path).expect("read merge log");
        assert!(log_content.contains("PR creation skipped"));
        assert!(log_content.contains("gh"));

        let chat_content = fs::read_to_string(&chat_file).expect("read chat file");
        assert!(chat_content.contains("PR: skipped"));
    }

    #[test]
    fn test_report_pull_request_creation_logs_failure_and_continues() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let log_dir = temp.path().join("logs");
        fs::create_dir_all(&log_dir).expect("create log dir");
        let merge_logger = swarm::log::NamedLogger::new(&log_dir, "MergeAgent", "merge-agent.log");
        let chat_file = temp.path().join("chat.md");

        report_pull_request_creation(
            PullRequestCreateResult::Failed {
                stdout: String::new(),
                stderr: "authentication failed".to_string(),
                exit_code: Some(1),
            },
            &merge_logger,
            chat_file.to_str().expect("chat path"),
        );

        let log_content = fs::read_to_string(merge_logger.path).expect("read merge log");
        assert!(log_content.contains("PR creation failed: exit_code=1"));
        assert!(log_content.contains("authentication failed"));

        let chat_content = fs::read_to_string(&chat_file).expect("read chat file");
        assert!(chat_content.contains("PR: failed to create (continuing)"));
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
    fn test_write_push_outcome_chat() {
        let temp = NamedTempFile::new().expect("temp chat file");
        write_push_outcome_chat(
            temp.path().to_str().expect("temp path"),
            "Push: pushed 'release' to origin",
        )
        .expect("write push chat");

        let content = fs::read_to_string(temp.path()).expect("read chat");
        let line = content.lines().next().expect("chat line");
        let (_, agent, message) = chat::parse_line(line).expect("parse chat line");
        assert_eq!(agent, "ScrumMaster");
        assert_eq!(message, "Push: pushed 'release' to origin");
    }

    #[test]
    fn test_push_skip_reason_when_target_not_explicit() {
        let reason = push_skip_reason(false, "sprint-1", "main", false);
        assert_eq!(reason, Some("target branch was not explicitly provided"));
    }

    #[test]
    fn test_push_skip_reason_when_shutdown_requested() {
        let reason = push_skip_reason(true, "sprint-1", "release", true);
        assert_eq!(reason, Some("shutdown requested"));
    }

    #[test]
    fn test_push_skip_reason_when_feature_matches_target() {
        let reason = push_skip_reason(true, "release", "release", false);
        assert_eq!(reason, Some("feature branch matches target branch"));
    }

    #[test]
    fn test_push_skip_reason_none_when_push_is_applicable() {
        let reason = push_skip_reason(true, "sprint-1", "release", false);
        assert_eq!(reason, None);
    }

    #[test]
    fn test_split_cleanup_initials_skips_merge_failures() {
        let failures = vec![MergeFailureInfo {
            initial: 'A',
            agent_name: "Aaron".to_string(),
            branch: "branch-a".to_string(),
            worktree_path: "/tmp/wt-a".to_string(),
            log_path: "/tmp/agent-A.log".to_string(),
            detail: "conflict".to_string(),
            skip_cleanup: true,
        }];
        let (cleanup, skipped) = split_cleanup_initials(&['A', 'B', 'C'], &failures);
        assert_eq!(cleanup, vec!['B', 'C']);
        assert_eq!(skipped, vec!['A']);
    }

    #[test]
    fn test_split_cleanup_initials_allows_cleanup_when_skip_false() {
        let failures = vec![MergeFailureInfo {
            initial: 'A',
            agent_name: "Aaron".to_string(),
            branch: "branch-a".to_string(),
            worktree_path: "/tmp/wt-a".to_string(),
            log_path: "/tmp/agent-A.log".to_string(),
            detail: "conflict".to_string(),
            skip_cleanup: false,
        }];
        let (cleanup, skipped) = split_cleanup_initials(&['A', 'B'], &failures);
        assert_eq!(cleanup, vec!['A', 'B']);
        assert_eq!(skipped, Vec::<char>::new());
    }

    #[test]
    fn test_preserve_failed_worktree_moves_and_detaches() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        let worktrees_dir = repo_root.join("worktrees");
        fs::create_dir_all(&worktrees_dir).expect("create worktrees dir");

        let worktree_path = worktrees_dir.join("agent-A");
        let worktree_path_str = worktree_path.to_string_lossy().to_string();
        let args = [
            "worktree",
            "add",
            "-B",
            "agent-branch",
            worktree_path_str.as_str(),
            "main",
        ];
        run_git_in(&repo_root, &args);

        assert!(
            worktree_path.exists(),
            "worktree should exist before preserve"
        );

        let outcome = preserve_failed_worktree(
            &repo_root,
            Path::new("worktrees"),
            &worktree_path,
            "agent-branch",
            0,
        );

        assert!(outcome.error.is_none(), "preserve should not error");
        assert!(outcome.allow_recreate, "preserve should allow recreate");
        assert!(
            !worktree_path.exists(),
            "original worktree path should be moved"
        );
        assert!(outcome.path.exists(), "preserved worktree should exist");
        assert!(
            outcome
                .path
                .starts_with(repo_root.join("worktrees").join("preserved")),
            "preserved worktree should live under worktrees/preserved"
        );

        let head = Command::new("git")
            .arg("-C")
            .arg(&outcome.path)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .expect("rev-parse head");
        assert!(head.status.success(), "rev-parse should succeed");
        let head_ref = String::from_utf8_lossy(&head.stdout).trim().to_string();
        assert_eq!(head_ref, "HEAD", "preserved worktree should be detached");
    }

    #[test]
    fn test_preserve_failed_worktree_missing_path() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        let worktree_path = repo_root.join("worktrees").join("missing");
        let outcome = preserve_failed_worktree(
            &repo_root,
            Path::new("worktrees"),
            &worktree_path,
            "agent-branch",
            0,
        );

        assert!(
            outcome.error.is_some(),
            "preserve should error on missing path"
        );
        assert!(
            !outcome.allow_recreate,
            "missing path should not allow recreate"
        );
    }

    #[test]
    fn test_create_branch_at_commit_creates_branch() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        let output = Command::new("git")
            .arg("-C")
            .arg(&repo_root)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("rev-parse head");
        assert!(output.status.success());
        let head = String::from_utf8_lossy(&output.stdout).trim().to_string();

        create_branch_at_commit(&repo_root, "agent-branch", &head).expect("create branch");

        let verify = Command::new("git")
            .arg("-C")
            .arg(&repo_root)
            .args(["show-ref", "--verify", "--quiet", "refs/heads/agent-branch"])
            .output()
            .expect("verify branch");
        assert!(verify.status.success());
    }

    #[test]
    fn test_create_sprint_worktree_in_forks_from_source_branch() {
        with_temp_cwd(|| {
            let repo_root = std::env::current_dir().expect("current dir");
            init_repo(&repo_root);

            let source_file = repo_root.join("source-only.txt");
            let target_file = repo_root.join("target-only.txt");

            run_git_in(&repo_root, &["checkout", "-b", "source-branch"]);
            fs::write(&source_file, "source").expect("write source file");
            run_git_in(&repo_root, &["add", "."]);
            run_git_in(&repo_root, &["commit", "-m", "source commit"]);
            let source_commit = String::from_utf8_lossy(
                &Command::new("git")
                    .arg("-C")
                    .arg(&repo_root)
                    .args(["rev-parse", "HEAD"])
                    .output()
                    .expect("rev-parse source")
                    .stdout,
            )
            .trim()
            .to_string();

            run_git_in(&repo_root, &["checkout", "main"]);
            run_git_in(&repo_root, &["checkout", "-b", "target-branch"]);
            fs::write(&target_file, "target").expect("write target file");
            run_git_in(&repo_root, &["add", "."]);
            run_git_in(&repo_root, &["commit", "-m", "target commit"]);
            let target_commit = String::from_utf8_lossy(
                &Command::new("git")
                    .arg("-C")
                    .arg(&repo_root)
                    .args(["rev-parse", "HEAD"])
                    .output()
                    .expect("rev-parse target")
                    .stdout,
            )
            .trim()
            .to_string();
            assert_ne!(
                source_commit, target_commit,
                "source and target branches should diverge"
            );

            run_git_in(&repo_root, &["checkout", "main"]);
            let worktrees_dir = repo_root.join("worktrees");
            let worktree_path =
                create_sprint_worktree_in(&worktrees_dir, "alpha-sprint-1", "source-branch")
                    .expect("create sprint worktree");

            let sprint_commit = String::from_utf8_lossy(
                &Command::new("git")
                    .arg("-C")
                    .arg(&worktree_path)
                    .args(["rev-parse", "HEAD"])
                    .output()
                    .expect("rev-parse sprint")
                    .stdout,
            )
            .trim()
            .to_string();

            assert_eq!(
                sprint_commit, source_commit,
                "sprint branch should fork from source branch"
            );
            assert_ne!(
                sprint_commit, target_commit,
                "sprint branch should not fork from target branch"
            );
            assert!(
                worktree_path.join("source-only.txt").exists(),
                "sprint worktree should contain source branch file"
            );
            assert!(
                !worktree_path.join("target-only.txt").exists(),
                "sprint worktree should not contain target-only file"
            );
        });
    }

    #[test]
    fn test_resolve_sprint_base_branch_uses_source_when_target_lags_source() {
        with_temp_cwd(|| {
            let repo_root = std::env::current_dir().expect("current dir");
            init_repo(&repo_root);
            run_git_in(&repo_root, &["checkout", "-b", "feature-1"]);
            run_git_in(&repo_root, &["checkout", "main"]);

            fs::write(repo_root.join("main-only.txt"), "main-only").expect("write main-only");
            run_git_in(&repo_root, &["add", "."]);
            run_git_in(&repo_root, &["commit", "-m", "main-only commit"]);

            let base = resolve_sprint_base_branch(&repo_root, "main", "feature-1")
                .expect("resolve base branch");
            assert_eq!(
                base, "main",
                "target lacks source tip, so sprint should fork from source"
            );
        });
    }

    #[test]
    fn test_resolve_sprint_base_branch_uses_target_after_target_catches_source() {
        with_temp_cwd(|| {
            let repo_root = std::env::current_dir().expect("current dir");
            init_repo(&repo_root);
            run_git_in(&repo_root, &["checkout", "-b", "feature-1"]);
            run_git_in(&repo_root, &["checkout", "main"]);

            fs::write(repo_root.join("main-only.txt"), "main-only").expect("write main-only");
            run_git_in(&repo_root, &["add", "."]);
            run_git_in(&repo_root, &["commit", "-m", "main-only commit"]);

            run_git_in(&repo_root, &["checkout", "feature-1"]);
            run_git_in(
                &repo_root,
                &["merge", "--no-ff", "main", "-m", "sync source to target"],
            );

            let base = resolve_sprint_base_branch(&repo_root, "main", "feature-1")
                .expect("resolve base branch");
            assert_eq!(
                base, "feature-1",
                "once target contains source tip, sprint should fork from target"
            );
        });
    }

    #[test]
    fn test_resolve_sprint_base_branch_uses_source_when_source_equals_target() {
        with_temp_cwd(|| {
            let repo_root = std::env::current_dir().expect("current dir");
            init_repo(&repo_root);

            let base = resolve_sprint_base_branch(&repo_root, "main", "main")
                .expect("resolve base branch");
            assert_eq!(base, "main");
        });
    }

    #[test]
    fn test_sync_target_branch_state_refreshes_namespaced_runtime_from_target() {
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

        let target_worktree = worktree::create_target_branch_worktree_in(&repo_root, "main")
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
        let runtime_paths = team::RuntimeStatePaths::for_branches(team_name, "main", "main");

        sync_target_branch_state(
            &repo_root,
            "main",
            "main",
            team_name,
            &config,
            &runtime_paths,
        )
        .expect("sync target branch state");

        let runtime_tasks = repo_root.join(runtime_paths.tasks_path());
        let runtime_history = repo_root.join(runtime_paths.sprint_history_path());

        let after_runtime = fs::read_to_string(&runtime_tasks).expect("read runtime tasks after");
        assert!(after_runtime.contains("[x]"));
        let runtime_history_loaded =
            team::SprintHistory::load_from(&runtime_history).expect("load runtime history");
        assert_eq!(
            runtime_history_loaded.total_sprints, 0,
            "runtime history should initialize locally instead of copying branch state"
        );

        let shared_after = fs::read_to_string(&tasks_path).expect("read shared tasks after");
        assert!(
            shared_after.contains("[ ]"),
            "shared template tasks should remain unchanged in namespaced mode"
        );
        let shared_history =
            team::SprintHistory::load_from(&history_path).expect("load shared history");
        assert_eq!(shared_history.total_sprints, 0);
    }

    #[test]
    fn test_sync_target_branch_state_namespaces_when_source_equals_target() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        let team_name = "greenfield";
        let team_dir = repo_root.join(".swarm-hug").join(team_name);
        fs::create_dir_all(&team_dir).expect("create team dir");
        let tasks_path = team_dir.join("tasks.md");
        let history_path = team_dir.join("sprint-history.json");
        fs::write(&tasks_path, "# Tasks\n\n- [ ] Shared template task\n")
            .expect("write main tasks");
        fs::write(
            &history_path,
            r#"{"team": "greenfield", "total_sprints": 0}"#,
        )
        .expect("write main history");
        run_git_in(&repo_root, &["add", "."]);
        run_git_in(&repo_root, &["commit", "-m", "seed main state"]);

        run_git_in(&repo_root, &["checkout", "-b", "feature-branch"]);
        fs::write(
            &tasks_path,
            "# Tasks\n\n- [ ] Feature branch task A\n- [ ] Feature branch task B\n",
        )
        .expect("write feature tasks");
        fs::write(
            &history_path,
            r#"{"team": "greenfield", "total_sprints": 3}"#,
        )
        .expect("write feature history");
        fs::write(
            team_dir.join("team-state.json"),
            r#"{"team": "greenfield", "feature_branch": "feature-sprint-3"}"#,
        )
        .expect("write feature state");
        run_git_in(&repo_root, &["add", "."]);
        run_git_in(&repo_root, &["commit", "-m", "seed feature branch state"]);
        run_git_in(&repo_root, &["checkout", "main"]);

        let mut config = Config::default();
        config.project = Some(team_name.to_string());
        config.files_tasks = format!(".swarm-hug/{}/tasks.md", team_name);

        let runtime_paths =
            team::RuntimeStatePaths::for_branches(team_name, "feature-branch", "feature-branch");
        assert!(
            runtime_paths.is_namespaced(),
            "runtime state should be namespaced by target branch even when source == target"
        );

        sync_target_branch_state(
            &repo_root,
            "feature-branch",
            "feature-branch",
            team_name,
            &config,
            &runtime_paths,
        )
        .expect("sync namespaced runtime");

        let runtime_tasks = repo_root.join(runtime_paths.tasks_path());
        let runtime_history = repo_root.join(runtime_paths.sprint_history_path());
        let runtime_state = repo_root.join(runtime_paths.team_state_path());

        let runtime_tasks_content = fs::read_to_string(&runtime_tasks).expect("read runtime tasks");
        assert!(
            runtime_tasks_content.contains("Feature branch task A"),
            "runtime tasks should come from target/source branch"
        );
        let runtime_history_loaded =
            team::SprintHistory::load_from(&runtime_history).expect("load runtime history");
        assert_eq!(
            runtime_history_loaded.total_sprints, 0,
            "runtime history should initialize locally in namespaced mode"
        );
        assert!(
            runtime_state.exists(),
            "runtime team-state should be bootstrapped"
        );

        let shared_tasks_after = fs::read_to_string(&tasks_path).expect("read shared tasks");
        assert!(
            shared_tasks_after.contains("Shared template task"),
            "shared project tasks should remain template state in namespaced mode"
        );
    }

    #[test]
    fn test_sync_target_branch_state_bootstraps_namespaced_runtime_tasks_from_target() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        let team_name = "greenfield";
        let team_dir = repo_root.join(".swarm-hug").join(team_name);
        fs::create_dir_all(&team_dir).expect("create team dir");
        let tasks_path = team_dir.join("tasks.md");
        let history_path = team_dir.join("sprint-history.json");
        fs::write(&tasks_path, "# Tasks\n\n- [ ] Original task\n").expect("write tasks");
        fs::write(
            &history_path,
            r#"{"team": "greenfield", "total_sprints": 0}"#,
        )
        .expect("write history");
        run_git_in(&repo_root, &["add", "."]);
        run_git_in(&repo_root, &["commit", "-m", "init state"]);

        // Create a source branch with different tasks than target
        run_git_in(&repo_root, &["checkout", "-b", "source-branch"]);
        fs::write(
            &tasks_path,
            "# Tasks\n\n- [x] Original task\n- [ ] Source task\n",
        )
        .expect("write source tasks");
        fs::write(
            &history_path,
            r#"{"team": "greenfield", "total_sprints": 2}"#,
        )
        .expect("write source history");
        run_git_in(&repo_root, &["add", "."]);
        run_git_in(&repo_root, &["commit", "-m", "source updates"]);

        // Create a target branch from main with different data
        run_git_in(&repo_root, &["checkout", "main"]);
        run_git_in(&repo_root, &["checkout", "-b", "target-branch"]);
        fs::write(&tasks_path, "# Tasks\n\n- [ ] Target task\n").expect("write target tasks");
        fs::write(
            &history_path,
            r#"{"team": "greenfield", "total_sprints": 1}"#,
        )
        .expect("write target history");
        run_git_in(&repo_root, &["add", "."]);
        run_git_in(&repo_root, &["commit", "-m", "target updates"]);

        // Switch to a detached state so neither source nor target is checked out
        run_git_in(&repo_root, &["checkout", "main"]);
        run_git_in(&repo_root, &["checkout", "-b", "dev"]);

        // Reset local files to original state
        fs::write(&tasks_path, "# Tasks\n\n- [ ] Original task\n").expect("reset tasks");
        fs::write(
            &history_path,
            r#"{"team": "greenfield", "total_sprints": 0}"#,
        )
        .expect("reset history");

        let mut config = Config::default();
        config.project = Some(team_name.to_string());
        config.files_tasks = format!(".swarm-hug/{}/tasks.md", team_name);
        let runtime_paths =
            team::RuntimeStatePaths::for_branches(team_name, "source-branch", "target-branch");

        // Sync from source-branch, with target-branch as the target
        sync_target_branch_state(
            &repo_root,
            "source-branch",
            "target-branch",
            team_name,
            &config,
            &runtime_paths,
        )
        .expect("sync from source branch");

        // Runtime namespace should use target-branch tasks and source-branch history/state.
        let runtime_tasks = repo_root.join(runtime_paths.tasks_path());
        let runtime_history = repo_root.join(runtime_paths.sprint_history_path());
        let runtime_state = repo_root.join(runtime_paths.team_state_path());

        let after = fs::read_to_string(&runtime_tasks).expect("read runtime tasks after");
        assert!(
            after.contains("Target task"),
            "tasks should come from target branch, got: {}",
            after
        );
        let history = team::SprintHistory::load_from(&runtime_history).expect("load history");
        assert_eq!(
            history.total_sprints, 0,
            "runtime history should initialize locally instead of copying source branch"
        );
        assert!(
            runtime_state.exists(),
            "team-state should also be bootstrapped"
        );

        // Shared team path must remain untouched in namespaced mode.
        let shared_after = fs::read_to_string(&tasks_path).expect("read shared tasks after");
        assert!(
            shared_after.contains("Original task"),
            "shared tasks should remain unchanged in namespaced mode"
        );
    }

    #[test]
    fn test_sync_target_branch_state_preserves_existing_namespaced_runtime_state() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        let team_name = "greenfield";
        let team_dir = repo_root.join(".swarm-hug").join(team_name);
        fs::create_dir_all(&team_dir).expect("create team dir");
        let tasks_path = team_dir.join("tasks.md");
        let history_path = team_dir.join("sprint-history.json");
        let state_path = team_dir.join("team-state.json");
        fs::write(&tasks_path, "# Tasks\n\n- [ ] Shared source task\n").expect("write tasks");
        fs::write(
            &history_path,
            r#"{"team": "greenfield", "total_sprints": 1}"#,
        )
        .expect("write history");
        fs::write(
            &state_path,
            r#"{"team": "greenfield", "feature_branch": "greenfield-sprint-1"}"#,
        )
        .expect("write state");
        run_git_in(&repo_root, &["add", "."]);
        run_git_in(&repo_root, &["commit", "-m", "init state"]);
        run_git_in(&repo_root, &["checkout", "-b", "source-branch"]);

        let runtime_paths =
            team::RuntimeStatePaths::for_branches(team_name, "source-branch", "target-branch");
        let runtime_tasks = repo_root.join(runtime_paths.tasks_path());
        let runtime_history = repo_root.join(runtime_paths.sprint_history_path());
        let runtime_state = repo_root.join(runtime_paths.team_state_path());
        fs::create_dir_all(runtime_tasks.parent().expect("runtime parent"))
            .expect("create runtime dir");
        fs::write(&runtime_tasks, "# Tasks\n\n- [ ] Runtime task\n").expect("write runtime tasks");
        fs::write(
            &runtime_history,
            r#"{"team": "greenfield", "total_sprints": 9}"#,
        )
        .expect("write runtime history");
        fs::write(
            &runtime_state,
            r#"{"team": "greenfield", "feature_branch": "runtime-sprint"}"#,
        )
        .expect("write runtime state");

        let mut config = Config::default();
        config.project = Some(team_name.to_string());
        config.files_tasks = format!(".swarm-hug/{}/tasks.md", team_name);

        sync_target_branch_state(
            &repo_root,
            "source-branch",
            "target-branch",
            team_name,
            &config,
            &runtime_paths,
        )
        .expect("sync namespaced runtime");

        let tasks_after = fs::read_to_string(&runtime_tasks).expect("read runtime tasks");
        let history_after =
            team::SprintHistory::load_from(&runtime_history).expect("load runtime history");
        let state_after = fs::read_to_string(&runtime_state).expect("read runtime state");

        assert!(
            tasks_after.contains("Runtime task"),
            "runtime tasks should not be overwritten"
        );
        assert_eq!(
            history_after.total_sprints, 9,
            "runtime history should not be overwritten"
        );
        assert!(
            state_after.contains("runtime-sprint"),
            "runtime team-state should not be overwritten"
        );
    }

    #[test]
    fn test_reconcile_sprint_tasks_from_git_uses_merge_commit_evidence() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        let mut task_list =
            swarm::task::TaskList::parse("# Tasks\n\n- [A] (#1) Task one\n- [A] (#2) Task two\n");
        let assignments = vec![
            ('A', "(#1) Task one".to_string()),
            ('A', "(#2) Task two".to_string()),
        ];

        let sprint_start = String::from_utf8_lossy(
            &Command::new("git")
                .arg("-C")
                .arg(&repo_root)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("rev-parse")
                .stdout,
        )
        .trim()
        .to_string();

        for idx in 1..=2 {
            fs::write(repo_root.join(format!("work-{}.txt", idx)), "done").expect("write change");
            run_git_in(&repo_root, &["add", "."]);
            let commit_output = Command::new("git")
                .arg("-C")
                .arg(&repo_root)
                .args([
                    "commit",
                    "-m",
                    &format!("Merge greenfield-agent-aaron-abc{:02}", idx),
                ])
                .env("GIT_AUTHOR_NAME", "Agent Aaron")
                .env("GIT_AUTHOR_EMAIL", "agent-a@swarm.local")
                .env("GIT_COMMITTER_NAME", "Agent Aaron")
                .env("GIT_COMMITTER_EMAIL", "agent-a@swarm.local")
                .output()
                .expect("commit as agent");
            assert!(
                commit_output.status.success(),
                "agent commit should succeed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&commit_output.stdout),
                String::from_utf8_lossy(&commit_output.stderr)
            );
        }

        let summary = reconcile_sprint_tasks_from_git(
            &repo_root,
            &sprint_start,
            &assignments,
            &[],
            false,
            &mut task_list,
        )
        .expect("reconcile from merge evidence");
        assert_eq!(summary.completed, 2);
        assert_eq!(summary.failed, 0);
        assert!(matches!(
            task_list.tasks[0].status,
            swarm::task::TaskStatus::Completed('A')
        ));
        assert!(matches!(
            task_list.tasks[1].status,
            swarm::task::TaskStatus::Completed('A')
        ));
    }

    #[test]
    fn test_reconcile_sprint_tasks_from_git_falls_back_to_success_when_diff_exists() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        let mut task_list =
            swarm::task::TaskList::parse("# Tasks\n\n- [A] (#1) Task one\n- [B] (#2) Task two\n");
        let assignments = vec![
            ('A', "(#1) Task one".to_string()),
            ('B', "(#2) Task two".to_string()),
        ];
        let results: Vec<TaskResult> = vec![
            ('A', "(#1) Task one".to_string(), true, None, None),
            ('B', "(#2) Task two".to_string(), true, None, None),
        ];

        let sprint_start = String::from_utf8_lossy(
            &Command::new("git")
                .arg("-C")
                .arg(&repo_root)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("rev-parse")
                .stdout,
        )
        .trim()
        .to_string();

        fs::write(repo_root.join("changed.txt"), "changed").expect("write change");
        run_git_in(&repo_root, &["add", "."]);
        run_git_in(&repo_root, &["commit", "-m", "non-agent change"]);

        let summary = reconcile_sprint_tasks_from_git(
            &repo_root,
            &sprint_start,
            &assignments,
            &results,
            false,
            &mut task_list,
        )
        .expect("reconcile from diff and success fallback");
        assert_eq!(summary.completed, 2);
        assert_eq!(summary.failed, 0);
        assert!(matches!(
            task_list.tasks[0].status,
            swarm::task::TaskStatus::Completed('A')
        ));
        assert!(matches!(
            task_list.tasks[1].status,
            swarm::task::TaskStatus::Completed('B')
        ));
    }

    #[test]
    fn test_reset_runtime_namespace_for_new_run_clears_namespaced_runtime_dir() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        let runtime_paths = team::RuntimeStatePaths::for_branches("greenfield", "main", "main");
        let runtime_root = repo_root.join(runtime_paths.root());
        fs::create_dir_all(&runtime_root).expect("create runtime root");
        fs::write(runtime_root.join("tasks.md"), "# Tasks\n\n- [ ] stale\n")
            .expect("write runtime tasks");

        reset_runtime_namespace_for_new_run(&repo_root, &runtime_paths)
            .expect("reset namespaced runtime");

        assert!(
            !runtime_root.exists(),
            "namespaced runtime directory should be removed on new run"
        );
    }

    #[test]
    fn test_reset_runtime_namespace_for_new_run_is_noop_for_legacy_paths() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        let runtime_paths = team::RuntimeStatePaths::for_branches("greenfield", "main", "");
        let legacy_root = repo_root.join(runtime_paths.root());
        fs::create_dir_all(&legacy_root).expect("create legacy root");
        let legacy_file = legacy_root.join("tasks.md");
        fs::write(&legacy_file, "# Tasks\n\n- [ ] keep\n").expect("write legacy tasks");

        reset_runtime_namespace_for_new_run(&repo_root, &runtime_paths)
            .expect("legacy reset should be noop");

        assert!(
            legacy_file.exists(),
            "legacy non-namespaced state should not be removed"
        );
    }

    #[test]
    fn test_ensure_branch_exists_succeeds_for_existing_branch() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        ensure_branch_exists(&repo_root, "main").expect("main should exist");
    }

    #[test]
    fn test_ensure_branch_exists_errors_for_missing_branch() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let repo_root = temp.path().to_path_buf();
        init_repo(&repo_root);

        let err = ensure_branch_exists(&repo_root, "nonexistent-branch")
            .expect_err("should error for missing branch");
        assert!(
            err.contains("source branch 'nonexistent-branch' does not exist"),
            "error should mention branch name, got: {}",
            err
        );
    }

    /// A no-op engine that claims to be Claude but does nothing.
    /// `ensure_feature_merged` with this engine skips the stub merge path,
    /// so the branch must be actually merged for verification to pass.
    struct NoopEngine;

    impl Engine for NoopEngine {
        fn execute(
            &self,
            _agent_name: &str,
            _task_description: &str,
            _working_dir: &Path,
            _turn_number: usize,
            _team_dir: Option<&str>,
        ) -> EngineResult {
            EngineResult::success("noop")
        }

        fn engine_type(&self) -> swarm::config::EngineType {
            swarm::config::EngineType::Claude
        }
    }

    #[test]
    fn test_retry_merge_agent_succeeds_on_retry_with_stub() {
        use swarm::engine::StubEngine;
        use swarm::log::NamedLogger;

        with_temp_cwd(|| {
            let repo_root = std::env::current_dir().expect("current dir");
            init_repo(&repo_root);

            // Create a feature branch with a diverging commit.
            run_git_in(&repo_root, &["checkout", "-b", "feature-retry"]);
            fs::write(repo_root.join("feature.txt"), "feature content")
                .expect("write feature file");
            run_git_in(&repo_root, &["add", "."]);
            run_git_in(&repo_root, &["commit", "-m", "feature commit"]);
            run_git_in(&repo_root, &["checkout", "main"]);

            let log_dir = repo_root.join("logs");
            fs::create_dir_all(&log_dir).expect("create log dir");
            let merge_logger = NamedLogger::new(&log_dir, "MergeAgent", "merge-agent.log");

            // StubEngine ensure_feature_merged will perform a real git merge.
            let engine = StubEngine::new(repo_root.join("loop").to_string_lossy().to_string());

            let result = retry_merge_agent(
                &engine,
                "feature-retry",
                "main",
                &repo_root,
                &[],
                "first attempt failed: not merged",
                &merge_logger,
            );

            assert!(
                result.is_ok(),
                "retry should succeed with stub engine, got: {:?}",
                result
            );

            // Verify the log contains attempt 2 success.
            let log_content = fs::read_to_string(merge_logger.path).unwrap_or_default();
            assert!(
                log_content.contains("succeeded on retry (attempt 2)"),
                "log should record retry success, got: {}",
                log_content
            );
        });
    }

    #[test]
    fn test_retry_merge_agent_fails_on_both_attempts() {
        use swarm::log::NamedLogger;

        with_temp_cwd(|| {
            let repo_root = std::env::current_dir().expect("current dir");
            init_repo(&repo_root);

            // Create a feature branch with a diverging commit.
            run_git_in(&repo_root, &["checkout", "-b", "feature-fail"]);
            fs::write(repo_root.join("fail.txt"), "fail content").expect("write fail file");
            run_git_in(&repo_root, &["add", "."]);
            run_git_in(&repo_root, &["commit", "-m", "feature-fail commit"]);
            run_git_in(&repo_root, &["checkout", "main"]);

            let log_dir = repo_root.join("logs");
            fs::create_dir_all(&log_dir).expect("create log dir");
            let merge_logger = NamedLogger::new(&log_dir, "MergeAgent", "merge-agent.log");

            // NoopEngine does not actually merge, so ensure_feature_merged fails.
            let engine = NoopEngine;

            let result = retry_merge_agent(
                &engine,
                "feature-fail",
                "main",
                &repo_root,
                &[],
                "first attempt: branch not merged",
                &merge_logger,
            );

            assert!(result.is_err(), "retry should fail with noop engine");
            let err = result.unwrap_err();
            assert!(
                err.contains("attempt 1"),
                "error should contain attempt 1 context, got: {}",
                err
            );
            assert!(
                err.contains("attempt 2"),
                "error should contain attempt 2 context, got: {}",
                err
            );

            // Verify log contains both attempt failures.
            let log_content = fs::read_to_string(merge_logger.path).unwrap_or_default();
            assert!(
                log_content.contains("Merge verification failed (attempt 2)"),
                "log should record attempt 2 failure, got: {}",
                log_content
            );
        });
    }

    #[test]
    fn test_retry_merge_agent_preserves_first_error_context() {
        use swarm::log::NamedLogger;

        with_temp_cwd(|| {
            let repo_root = std::env::current_dir().expect("current dir");
            init_repo(&repo_root);

            run_git_in(&repo_root, &["checkout", "-b", "feature-ctx"]);
            fs::write(repo_root.join("ctx.txt"), "ctx").expect("write ctx file");
            run_git_in(&repo_root, &["add", "."]);
            run_git_in(&repo_root, &["commit", "-m", "ctx commit"]);
            run_git_in(&repo_root, &["checkout", "main"]);

            let log_dir = repo_root.join("logs");
            fs::create_dir_all(&log_dir).expect("create log dir");
            let merge_logger = NamedLogger::new(&log_dir, "MergeAgent", "merge-agent.log");

            let engine = NoopEngine;
            let first_err_msg = "squash-merge detected: single-parent commit";

            let result = retry_merge_agent(
                &engine,
                "feature-ctx",
                "main",
                &repo_root,
                &[],
                first_err_msg,
                &merge_logger,
            );

            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                err.contains(first_err_msg),
                "error should preserve the original first_err message, got: {}",
                err
            );
        });
    }
}
