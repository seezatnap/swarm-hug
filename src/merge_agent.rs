use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::EngineType;
use crate::engine::{self, Engine, EngineResult};
use crate::prompt;
use crate::worktree;

/// Generate the merge agent prompt for feature-to-target branch merges.
pub fn generate_merge_agent_prompt(
    feature_branch: &str,
    target_branch: &str,
    target_worktree_path: &Path,
) -> Result<String, String> {
    let feature = normalize_branch("feature", feature_branch)?;
    let target = normalize_branch("target", target_branch)?;
    let target_worktree = target_worktree_path.to_string_lossy().to_string();

    let mut vars = HashMap::new();
    vars.insert("feature_branch", feature);
    vars.insert("target_branch", target);
    vars.insert("target_worktree_path", target_worktree);
    vars.insert("co_author", engine::coauthor_line());

    prompt::load_and_render("merge_agent", &vars)
}

/// Run the merge agent to merge a feature branch into the target branch.
///
/// Returns the engine result so callers can inspect success and output.
pub fn run_merge_agent(
    engine: &dyn Engine,
    feature_branch: &str,
    target_branch: &str,
    repo_root: &Path,
) -> Result<EngineResult, String> {
    if engine.engine_type() == EngineType::Stub {
        let message = format!(
            "Stub merge agent: {} -> {}",
            feature_branch.trim(),
            target_branch.trim()
        );
        return Ok(EngineResult::success(message));
    }

    let main_repo = main_worktree_root(repo_root)?;
    let target_worktree_path =
        worktree::create_target_branch_worktree_in(&main_repo, target_branch)?;
    let prompt = generate_merge_agent_prompt(feature_branch, target_branch, &target_worktree_path)?;

    Ok(engine.execute(
        "MergeAgent",
        &prompt,
        &target_worktree_path,
        0,
        None,
    ))
}

/// Run the merge agent inside an existing target worktree.
///
/// This avoids creating a new target worktree (useful when the target branch
/// is already checked out elsewhere, e.g. sprint worktrees).
pub fn run_merge_agent_in_worktree(
    engine: &dyn Engine,
    feature_branch: &str,
    target_branch: &str,
    target_worktree_path: &Path,
) -> Result<EngineResult, String> {
    if engine.engine_type() == EngineType::Stub {
        let message = format!(
            "Stub merge agent: {} -> {}",
            feature_branch.trim(),
            target_branch.trim()
        );
        return Ok(EngineResult::success(message));
    }

    let prompt = generate_merge_agent_prompt(feature_branch, target_branch, target_worktree_path)?;
    Ok(engine.execute(
        "MergeAgent",
        &prompt,
        target_worktree_path,
        0,
        None,
    ))
}

/// Ensure the feature branch is merged into the target branch after merge agent runs.
///
/// In stub mode, performs a deterministic git merge so tests can validate behavior.
/// When the feature and target branches differ, additionally verifies that the
/// target branch tip has two parents (a true merge commit). If the tip has only
/// one parent, returns a specific squash-merge diagnostic.
pub fn ensure_feature_merged(
    engine: &dyn Engine,
    feature_branch: &str,
    target_branch: &str,
    repo_root: &Path,
) -> Result<(), String> {
    let feature = normalize_branch("feature", feature_branch)?;
    let target = normalize_branch("target", target_branch)?;
    let main_repo = main_worktree_root(repo_root)?;

    if engine.engine_type() == EngineType::Stub {
        stub_merge_feature_branch(&main_repo, &feature, &target)?;
    }

    if is_branch_merged(&main_repo, &feature, &target)? {
        // When feature and target are different branches, verify 2-parent merge commit
        if feature != target {
            let parent_count = commit_parent_count(&main_repo, &target)?;
            if parent_count < 2 {
                return Err(format!(
                    "squash-merge detected: tip of '{}' has {} parent(s), expected 2-parent merge commit",
                    target, parent_count
                ));
            }
        }
        Ok(())
    } else {
        Err(format!(
            "feature branch '{}' is not merged into '{}'",
            feature, target
        ))
    }
}

/// Verify the merge, retrying `run_merge_agent` once on initial verification failure.
///
/// 1. Calls `ensure_feature_merged` to check if the feature branch is already merged.
/// 2. If verification fails, re-runs `run_merge_agent` exactly once.
/// 3. Calls `ensure_feature_merged` a second time.
/// 4. If the second verification also fails, returns a fatal error with no further retries.
pub fn run_merge_agent_with_retry(
    engine: &dyn Engine,
    feature_branch: &str,
    target_branch: &str,
    repo_root: &Path,
) -> Result<(), String> {
    verify_with_retry(
        || ensure_feature_merged(engine, feature_branch, target_branch, repo_root),
        || run_merge_agent(engine, feature_branch, target_branch, repo_root),
    )
}

/// Core retry loop: verify, and if verification fails, run the merge agent once
/// then re-verify. Returns `Ok(())` on success, or a fatal error after the
/// second verification failure.
///
/// Extracted for testability — the public API is `run_merge_agent_with_retry`.
fn verify_with_retry<V, R>(mut verify: V, retry: R) -> Result<(), String>
where
    V: FnMut() -> Result<(), String>,
    R: FnOnce() -> Result<EngineResult, String>,
{
    // First verification attempt
    match verify() {
        Ok(()) => return Ok(()),
        Err(first_err) => {
            // Retry: re-run the merge agent once
            let retry_result = retry()?;
            if !retry_result.success {
                let detail = retry_result
                    .error
                    .unwrap_or_else(|| "merge agent retry failed".to_string());
                return Err(format!(
                    "merge agent retry failed after initial verification error '{}': {}",
                    first_err, detail
                ));
            }

            // Second verification attempt — fatal on failure
            verify().map_err(|second_err| {
                format!(
                    "merge verification failed after retry (initial: '{}', retry: '{}')",
                    first_err, second_err
                )
            })
        }
    }
}

/// Prepare the main repo working tree for a merge by cleaning known paths.
///
/// This resets tracked files and removes untracked files that would block the merge.
pub fn prepare_merge_workspace(repo_root: &Path, paths: &[PathBuf]) -> Result<(), String> {
    let main_repo = main_worktree_root(repo_root)?;

    for path in paths {
        let (relative, absolute) = normalize_path(&main_repo, path);
        if relative.is_empty() {
            continue;
        }

        if is_tracked(&main_repo, &relative)? {
            reset_tracked_path(&main_repo, &relative)?;
        } else if absolute.is_file() && should_remove_untracked(&absolute) {
            std::fs::remove_file(&absolute)
                .map_err(|e| format!("failed to remove {}: {}", absolute.display(), e))?;
        }
    }

    Ok(())
}

fn normalize_branch(label: &str, branch: &str) -> Result<String, String> {
    let trimmed = branch.trim();
    if trimmed.is_empty() {
        return Err(format!("{} branch name is empty", label));
    }
    Ok(trimmed.to_string())
}

fn normalize_path(main_repo: &Path, path: &Path) -> (String, PathBuf) {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        main_repo.join(path)
    };

    let relative = absolute
        .strip_prefix(main_repo)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string());

    (relative, absolute)
}

fn should_remove_untracked(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("sprint-history.json") | Some("team-state.json")
    )
}

fn main_worktree_root(repo_root: &Path) -> Result<PathBuf, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(|e| format!("failed to run git worktree list: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree list failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                continue;
            }
            let candidate = PathBuf::from(trimmed);
            return Ok(if candidate.is_absolute() {
                candidate
            } else {
                repo_root.join(candidate)
            });
        }
    }

    Err("no worktree entries found".to_string())
}

fn commit_parent_count(repo_root: &Path, branch: &str) -> Result<usize, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-list", "--parents", "-1", branch])
        .output()
        .map_err(|e| format!("failed to run git rev-list: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git rev-list failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "commit_hash parent1 parent2 ..."
    // Number of parents = number of space-separated tokens - 1
    let count = stdout.trim().split_whitespace().count();
    Ok(if count > 0 { count - 1 } else { 0 })
}

fn is_branch_merged(repo_root: &Path, feature: &str, target: &str) -> Result<bool, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["merge-base", "--is-ancestor", feature, target])
        .output()
        .map_err(|e| format!("failed to run git merge-base: {}", e))?;

    if output.status.success() {
        return Ok(true);
    }

    match output.status.code() {
        Some(1) => Ok(false),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("git merge-base failed: {}", stderr.trim()))
        }
    }
}

fn is_tracked(repo_root: &Path, path: &str) -> Result<bool, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["ls-files", "--error-unmatch", path])
        .output()
        .map_err(|e| format!("failed to run git ls-files: {}", e))?;

    if output.status.success() {
        Ok(true)
    } else if output.status.code() == Some(1) {
        Ok(false)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git ls-files failed: {}", stderr.trim()))
    }
}

fn reset_tracked_path(repo_root: &Path, path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["checkout", "--", path])
        .output()
        .map_err(|e| format!("failed to run git checkout: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git checkout failed: {}", stderr.trim()))
    }
}

fn stub_merge_feature_branch(
    repo_root: &Path,
    feature_branch: &str,
    target_branch: &str,
) -> Result<(), String> {
    let checkout = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["checkout", target_branch])
        .output()
        .map_err(|e| format!("failed to run git checkout: {}", e))?;

    if !checkout.status.success() {
        let stderr = String::from_utf8_lossy(&checkout.stderr);
        return Err(format!("git checkout failed: {}", stderr.trim()));
    }

    let merge = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["merge", "--no-ff", feature_branch])
        .env("GIT_AUTHOR_NAME", "Swarm ScrumMaster")
        .env("GIT_AUTHOR_EMAIL", "scrummaster@swarm.local")
        .env("GIT_COMMITTER_NAME", "Swarm ScrumMaster")
        .env("GIT_COMMITTER_EMAIL", "scrummaster@swarm.local")
        .output()
        .map_err(|e| format!("failed to run git merge: {}", e))?;

    if merge.status.success() {
        return Ok(());
    }

    let conflicts = merge_conflicts(repo_root).unwrap_or_default();
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["merge", "--abort"])
        .output();

    if !conflicts.is_empty() {
        return Err(format!("merge conflicts: {}", conflicts.join(", ")));
    }

    let stderr = String::from_utf8_lossy(&merge.stderr);
    let err = stderr.trim();
    if err.is_empty() {
        Err("git merge failed".to_string())
    } else {
        Err(format!("git merge failed: {}", err))
    }
}

fn merge_conflicts(repo_root: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["diff", "--name-only", "--diff-filter=U"])
        .output()
        .map_err(|e| format!("failed to run git diff: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use crate::engine::StubEngine;
    use crate::testutil::with_temp_cwd;

    #[test]
    fn test_generate_merge_agent_prompt_renders_vars() {
        with_temp_cwd(|| {
            fs::create_dir_all(".swarm-hug").unwrap();
            fs::write(".swarm-hug/email.txt", "dev@example.com").unwrap();

            let prompt = generate_merge_agent_prompt(
                "feature-1",
                "main",
                Path::new("/tmp/target-worktree"),
            )
            .unwrap();
            assert!(prompt.contains("feature-1"));
            assert!(prompt.contains("main"));
            assert!(prompt.contains("/tmp/target-worktree"));
            assert!(prompt.contains("Co-Authored-By: dev <dev@example.com>"));
            assert!(!prompt.contains("{{feature_branch}}"));
            assert!(!prompt.contains("{{target_branch}}"));
        });
    }

    #[test]
    fn test_generate_merge_agent_prompt_rejects_empty_branch() {
        let path = Path::new("/tmp/target-worktree");
        assert!(generate_merge_agent_prompt("", "main", path).is_err());
        assert!(generate_merge_agent_prompt("feature", " ", path).is_err());
    }

    #[test]
    fn test_run_merge_agent_stub() {
        with_temp_cwd(|| {
            init_repo();
            let engine = StubEngine::new("loop");
            let result = run_merge_agent(&engine, "feature-x", "main", Path::new("."))
                .expect("run merge agent");
            assert!(result.success);
            assert!(result.output.contains("Stub merge agent"));
        });
    }

    fn run_git(args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .output()
            .expect("git command");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo() {
        run_git(&["init"]);
        run_git(&["config", "user.name", "Swarm Test"]);
        run_git(&["config", "user.email", "swarm-test@example.com"]);
        fs::write("README.md", "init").expect("write readme");
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "init"]);
        run_git(&["branch", "-M", "master"]);
    }

    fn commit_on_branch(branch: &str, filename: &str) {
        run_git(&["checkout", "-B", branch]);
        fs::write(filename, branch).expect("write file");
        run_git(&["add", "."]);
        run_git(&["commit", "-m", &format!("commit {}", branch)]);
    }

    fn is_merged(feature: &str, target: &str) -> bool {
        let output = Command::new("git")
            .args(["merge-base", "--is-ancestor", feature, target])
            .output()
            .expect("git merge-base");
        output.status.success()
    }

    #[test]
    fn test_ensure_feature_merged_stub_merges_branch() {
        with_temp_cwd(|| {
            init_repo();
            commit_on_branch("feature-1", "feature.txt");

            let engine = StubEngine::new("loop");
            ensure_feature_merged(&engine, "feature-1", "master", Path::new("."))
                .expect("ensure merge");

            assert!(is_merged("feature-1", "master"));
        });
    }

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

        fn engine_type(&self) -> EngineType {
            EngineType::Claude
        }
    }

    #[test]
    fn test_ensure_feature_merged_non_stub_requires_merge() {
        with_temp_cwd(|| {
            init_repo();
            commit_on_branch("feature-2", "feature2.txt");

            let engine = NoopEngine;
            let err = ensure_feature_merged(&engine, "feature-2", "master", Path::new("."))
                .expect_err("should detect missing merge");
            assert!(err.contains("not merged"));
            assert!(!is_merged("feature-2", "master"));
        });
    }

    #[test]
    fn test_merge_agent_prompt_preflight_aborts_only_stale_merges() {
        let template = crate::prompt::get_embedded("merge_agent").unwrap();

        // Step 0 must explicitly scope abort to pre-existing/stale merges
        assert!(
            template.contains("PRE-EXISTING stale merges"),
            "Step 0 must label abort as for pre-existing stale merges only"
        );
        assert!(
            template.contains("you have NOT yet run step 3"),
            "Step 0 must condition abort on step 3 not having run"
        );
    }

    #[test]
    fn test_merge_agent_prompt_forbids_abort_after_step_3() {
        let template = crate::prompt::get_embedded("merge_agent").unwrap();

        // After step 3, the prompt must warn never to abort
        assert!(
            template.contains("MERGE_HEAD is YOUR merge state"),
            "Prompt must warn that MERGE_HEAD belongs to this run after step 3"
        );
        assert!(
            template.contains("Do NOT run `git merge --abort` at this stage"),
            "Prompt must forbid abort during conflict resolution (Phase C)"
        );
    }

    #[test]
    fn test_merge_agent_prompt_preflight_runs_once() {
        let template = crate::prompt::get_embedded("merge_agent").unwrap();

        // Preflight must be labeled as run-once
        assert!(
            template.contains("run ONCE before starting the merge"),
            "Preflight phase must be labeled as run-once"
        );
        assert!(
            template.contains("you must NEVER\nreturn to this step"),
            "Prompt must forbid returning to step 0 after step 3"
        );
    }

    #[test]
    fn test_prompt_contains_critical_rules_section() {
        let prompt = prompt::embedded::MERGE_AGENT;
        assert!(
            prompt.contains("## Critical Rules"),
            "merge agent prompt must contain a Critical Rules section"
        );
    }

    #[test]
    fn test_prompt_bans_squash_merge() {
        let prompt = prompt::embedded::MERGE_AGENT;
        assert!(
            prompt.contains("git merge --squash") && prompt.contains("Banned"),
            "prompt must explicitly ban git merge --squash"
        );
    }

    #[test]
    fn test_prompt_bans_cherry_pick() {
        let prompt = prompt::embedded::MERGE_AGENT;
        assert!(
            prompt.contains("git cherry-pick") && prompt.contains("Banned"),
            "prompt must explicitly ban git cherry-pick"
        );
    }

    #[test]
    fn test_prompt_bans_diff_apply() {
        let prompt = prompt::embedded::MERGE_AGENT;
        assert!(
            prompt.contains("git diff") && prompt.contains("git apply") && prompt.contains("Banned"),
            "prompt must explicitly ban git diff | git apply"
        );
    }

    #[test]
    fn test_prompt_bans_rebase() {
        let prompt = prompt::embedded::MERGE_AGENT;
        assert!(
            prompt.contains("git rebase") && prompt.contains("Banned"),
            "prompt must explicitly ban git rebase"
        );
    }

    #[test]
    fn test_prompt_requires_no_ff_for_conflicts() {
        let prompt = prompt::embedded::MERGE_AGENT;
        assert!(
            prompt.contains("git merge --no-ff"),
            "prompt must require git merge --no-ff as the only permitted strategy"
        );
        assert!(
            prompt.contains("ONLY permitted merge strategy"),
            "prompt must state --no-ff is the ONLY permitted strategy"
        );
    }

    #[test]
    fn test_prompt_critical_rules_survive_rendering() {
        with_temp_cwd(|| {
            fs::create_dir_all(".swarm-hug").unwrap();
            fs::write(".swarm-hug/email.txt", "dev@example.com").unwrap();

            let rendered = generate_merge_agent_prompt(
                "feature-1",
                "main",
                Path::new("/tmp/target-worktree"),
            )
            .unwrap();
            assert!(
                rendered.contains("## Critical Rules"),
                "Critical Rules section must survive template rendering"
            );
            assert!(
                rendered.contains("git merge --squash"),
                "squash ban must survive rendering"
            );
            assert!(
                rendered.contains("git cherry-pick"),
                "cherry-pick ban must survive rendering"
            );
            assert!(
                rendered.contains("git rebase"),
                "rebase ban must survive rendering"
            );
        });
    }

    #[test]
    fn test_prepare_merge_workspace_resets_and_cleans() {
        with_temp_cwd(|| {
            init_repo();
            run_git(&["checkout", "-B", "master"]);
            std::fs::create_dir_all(".swarm-hug/alpha").expect("create team dir");
            fs::write(".swarm-hug/alpha/tasks.md", "task one\n").expect("write tasks");
            run_git(&["add", ".swarm-hug/alpha/tasks.md"]);
            run_git(&["commit", "-m", "add tasks"]);

            fs::write(".swarm-hug/alpha/team-state.json", "{\"team\":\"alpha\"}")
                .expect("write team state");
            fs::write(".swarm-hug/alpha/sprint-history.json", "{\"sprint\":1}")
                .expect("write sprint history");
            fs::write(".swarm-hug/alpha/tasks.md", "task one\nchanged\n")
                .expect("modify tasks");

            let paths = vec![
                PathBuf::from(".swarm-hug/alpha/tasks.md"),
                PathBuf::from(".swarm-hug/alpha/team-state.json"),
                PathBuf::from(".swarm-hug/alpha/sprint-history.json"),
            ];

            prepare_merge_workspace(Path::new("."), &paths).expect("prepare workspace");

            let tasks = fs::read_to_string(".swarm-hug/alpha/tasks.md").expect("read tasks");
            assert_eq!(tasks, "task one\n");
            assert!(!Path::new(".swarm-hug/alpha/team-state.json").exists());
            assert!(!Path::new(".swarm-hug/alpha/sprint-history.json").exists());
        });
    }

    // --- Tests for verify_with_retry (runner retry behavior) ---

    use std::cell::Cell;

    #[test]
    fn test_verify_with_retry_succeeds_on_first_verification() {
        // When the first verification passes, no retry should occur.
        let retry_called = Cell::new(false);

        let result = verify_with_retry(
            || Ok(()),
            || {
                retry_called.set(true);
                Ok(EngineResult::success("should not run"))
            },
        );

        assert!(result.is_ok());
        assert!(
            !retry_called.get(),
            "retry should not be called when first verification succeeds"
        );
    }

    #[test]
    fn test_verify_with_retry_succeeds_on_second_attempt() {
        // First verification fails, merge agent retries (succeeds),
        // second verification passes.
        let call_count = Cell::new(0u32);
        let retry_called = Cell::new(false);

        let result = verify_with_retry(
            || {
                let n = call_count.get();
                call_count.set(n + 1);
                if n == 0 {
                    Err("not merged yet".to_string())
                } else {
                    Ok(())
                }
            },
            || {
                retry_called.set(true);
                Ok(EngineResult::success("merge agent retry output"))
            },
        );

        assert!(result.is_ok());
        assert!(retry_called.get(), "retry should be called after first verification failure");
        assert_eq!(call_count.get(), 2, "verify should be called exactly twice");
    }

    #[test]
    fn test_verify_with_retry_fatal_after_second_failure() {
        // Both verifications fail — should return a fatal error with no extra retries.
        let verify_count = Cell::new(0u32);
        let retry_count = Cell::new(0u32);

        let result = verify_with_retry(
            || {
                let n = verify_count.get();
                verify_count.set(n + 1);
                Err(format!("verify failed attempt {}", n + 1))
            },
            || {
                let n = retry_count.get();
                retry_count.set(n + 1);
                Ok(EngineResult::success("retry succeeded but merge still bad"))
            },
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("merge verification failed after retry"),
            "error should indicate retry exhaustion, got: {}",
            err
        );
        assert!(
            err.contains("verify failed attempt 1"),
            "error should contain initial failure detail, got: {}",
            err
        );
        assert!(
            err.contains("verify failed attempt 2"),
            "error should contain retry failure detail, got: {}",
            err
        );
        assert_eq!(verify_count.get(), 2, "verify should be called exactly twice");
        assert_eq!(retry_count.get(), 1, "retry should be called exactly once");
    }

    #[test]
    fn test_verify_with_retry_no_extra_retries_after_second_failure() {
        // Ensure there are no additional retry or verification attempts beyond
        // the one retry + one re-verification.
        let verify_count = Cell::new(0u32);
        let retry_count = Cell::new(0u32);

        let _ = verify_with_retry(
            || {
                verify_count.set(verify_count.get() + 1);
                Err("always fails".to_string())
            },
            || {
                retry_count.set(retry_count.get() + 1);
                Ok(EngineResult::success("retry ran"))
            },
        );

        assert_eq!(
            verify_count.get(),
            2,
            "verify must be called exactly 2 times (initial + after retry), not more"
        );
        assert_eq!(
            retry_count.get(),
            1,
            "retry must be called exactly 1 time, not more"
        );
    }

    #[test]
    fn test_verify_with_retry_retry_engine_failure_returns_error() {
        // If the retry merge agent itself fails (returns !success), should
        // return an error without a second verification.
        let verify_count = Cell::new(0u32);

        let result = verify_with_retry(
            || {
                verify_count.set(verify_count.get() + 1);
                Err("initial verification failed".to_string())
            },
            || Ok(EngineResult::failure("engine crashed", 1)),
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("merge agent retry failed"),
            "error should indicate retry failure, got: {}",
            err
        );
        assert!(
            err.contains("engine crashed"),
            "error should contain engine failure detail, got: {}",
            err
        );
        assert_eq!(
            verify_count.get(),
            1,
            "verify should only be called once when retry engine fails"
        );
    }

    #[test]
    fn test_verify_with_retry_retry_execution_error_propagates() {
        // If the retry function returns Err (execution error, not engine
        // failure), it should propagate directly.
        let verify_count = Cell::new(0u32);

        let result = verify_with_retry(
            || {
                verify_count.set(verify_count.get() + 1);
                Err("initial verification failed".to_string())
            },
            || Err("failed to spawn merge agent".to_string()),
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err, "failed to spawn merge agent");
        assert_eq!(
            verify_count.get(),
            1,
            "verify should only be called once when retry execution fails"
        );
    }

    #[test]
    fn test_run_merge_agent_with_retry_stub_already_merged() {
        // Integration test: when the feature is already merged, no retry
        // should occur.
        with_temp_cwd(|| {
            init_repo();
            commit_on_branch("feature-retry", "retry.txt");
            run_git(&["checkout", "master"]);
            run_git(&["merge", "--no-ff", "feature-retry"]);

            assert!(is_merged("feature-retry", "master"));

            let engine = StubEngine::new("loop");
            run_merge_agent_with_retry(&engine, "feature-retry", "master", Path::new("."))
                .expect("already merged should succeed without retry");
        });
    }

    #[test]
    fn test_ensure_feature_merged_stub_creates_two_parent_commit() {
        with_temp_cwd(|| {
            init_repo();
            commit_on_branch("feature-pc", "feature_pc.txt");

            let engine = StubEngine::new("loop");
            ensure_feature_merged(&engine, "feature-pc", "master", Path::new("."))
                .expect("ensure merge with parent-count check");

            // Verify the merge commit on master has 2 parents
            let output = Command::new("git")
                .args(["rev-list", "--parents", "-1", "master"])
                .output()
                .expect("git rev-list");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let parent_count = stdout.trim().split_whitespace().count() - 1;
            assert_eq!(parent_count, 2, "merge commit should have 2 parents");
        });
    }

    #[test]
    fn test_run_merge_agent_with_retry_stub_merges_on_first_verify() {
        // Integration test: stub engine performs the merge during
        // ensure_feature_merged, so it succeeds on the first verification.
        with_temp_cwd(|| {
            init_repo();
            commit_on_branch("feature-stub-retry", "stub-retry.txt");

            let engine = StubEngine::new("loop");
            run_merge_agent_with_retry(
                &engine,
                "feature-stub-retry",
                "master",
                Path::new("."),
            )
            .expect("stub should merge and verify on first attempt");

            assert!(is_merged("feature-stub-retry", "master"));
        });
    }

    // --- Tests for ensure_feature_merged parent-count enforcement (#7) ---

    #[test]
    fn test_ensure_feature_merged_detects_squash_merge() {
        // When ancestry check passes but the target tip is a single-parent
        // commit (squash-merge), ensure_feature_merged should return an error
        // with a specific "squash-merge detected" diagnostic.
        with_temp_cwd(|| {
            init_repo();
            commit_on_branch("feature-squash", "feature_squash.txt");
            run_git(&["checkout", "master"]);

            // Perform a proper merge first so ancestry check passes
            run_git(&["merge", "--no-ff", "feature-squash", "-m", "real merge"]);
            assert!(is_merged("feature-squash", "master"));

            // Now add a single-parent commit on top so the tip is not a merge commit,
            // simulating what happens after a squash-merge.
            fs::write("extra.txt", "extra").expect("write extra");
            run_git(&["add", "."]);
            run_git(&["commit", "-m", "squash-like commit"]);

            // Ancestry still passes (feature-squash is an ancestor of master)
            assert!(is_merged("feature-squash", "master"));

            // But ensure_feature_merged should detect the single-parent tip
            let engine = NoopEngine;
            let err = ensure_feature_merged(&engine, "feature-squash", "master", Path::new("."))
                .expect_err("should detect squash-merge");
            assert!(
                err.contains("squash-merge detected"),
                "error should mention squash-merge, got: {}",
                err
            );
            assert!(
                err.contains("1 parent(s)"),
                "error should report parent count, got: {}",
                err
            );
        });
    }

    #[test]
    fn test_ensure_feature_merged_detects_single_parent_tip() {
        with_temp_cwd(|| {
            init_repo();
            // Create a feature branch, then fast-forward master to it.
            // The feature IS an ancestor of master (ancestry passes), but
            // the tip of master has only 1 parent (no merge commit).
            commit_on_branch("feature-ff", "feature_ff.txt");
            // Fast-forward master to feature-ff tip
            run_git(&["checkout", "master"]);
            run_git(&["merge", "--ff-only", "feature-ff"]);

            // Now master tip == feature-ff tip, 1 parent commit.
            // ensure_feature_merged should detect this as squash-merge.
            let engine = NoopEngine;
            let err = ensure_feature_merged(&engine, "feature-ff", "master", Path::new("."))
                .expect_err("should detect single-parent tip");
            assert!(
                err.contains("squash-merge detected"),
                "expected squash-merge error, got: {}",
                err
            );
            assert!(
                err.contains("1 parent"),
                "expected parent count in error, got: {}",
                err
            );
        });
    }

    #[test]
    fn test_ensure_feature_merged_accepts_two_parent_merge() {
        // A proper --no-ff merge produces a 2-parent commit; ensure_feature_merged
        // should accept it without error.
        with_temp_cwd(|| {
            init_repo();
            commit_on_branch("feature-proper", "feature_proper.txt");
            run_git(&["checkout", "master"]);

            // Perform a proper --no-ff merge (creates 2-parent commit)
            run_git(&["merge", "--no-ff", "feature-proper", "-m", "proper merge"]);

            let engine = NoopEngine;
            ensure_feature_merged(&engine, "feature-proper", "master", Path::new("."))
                .expect("two-parent merge should pass validation");
        });
    }

    #[test]
    fn test_ensure_feature_merged_squash_detection_with_noop_engine() {
        with_temp_cwd(|| {
            init_repo();
            commit_on_branch("feature-sq2", "feature_sq2.txt");

            // Manually merge with --squash then commit, so content is included
            // but there's only 1 parent. However, with --squash, git merge-base
            // --is-ancestor will NOT consider feature-sq2 as ancestor of master.
            // So the "not merged" error fires first, not the squash detection.
            run_git(&["checkout", "master"]);
            run_git(&["merge", "--squash", "feature-sq2"]);
            run_git(&["commit", "-m", "squash"]);

            let engine = NoopEngine;
            let err = ensure_feature_merged(&engine, "feature-sq2", "master", Path::new("."))
                .expect_err("should fail");
            // The ancestry check fails first for actual squash merges
            assert!(err.contains("not merged"));
        });
    }

    #[test]
    fn test_ensure_feature_merged_same_branch_skips_parent_check() {
        // When feature == target, the parent count check should be skipped
        // entirely. This covers the case where an agent merges into its own
        // branch (e.g. same-branch verification).
        with_temp_cwd(|| {
            init_repo();

            // When feature == target, stub engine will attempt the merge (no-op
            // since same branch). The parent check is skipped because the
            // branches are identical.
            let engine = StubEngine::new("loop");
            ensure_feature_merged(&engine, "master", "master", Path::new("."))
                .expect("same-branch should pass without parent check");
        });
    }

    #[test]
    fn test_commit_parent_count_single_parent() {
        with_temp_cwd(|| {
            init_repo();
            // Initial commit on master has 0 parents (root commit), let's add one more
            fs::write("file2.txt", "content").expect("write file");
            run_git(&["add", "."]);
            run_git(&["commit", "-m", "second commit"]);

            let count = commit_parent_count(Path::new("."), "master").expect("parent count");
            assert_eq!(count, 1, "regular commit should have 1 parent");
        });
    }

    #[test]
    fn test_commit_parent_count_merge_commit() {
        with_temp_cwd(|| {
            init_repo();
            commit_on_branch("feature-cnt", "feature_cnt.txt");
            run_git(&["checkout", "master"]);
            run_git(&["merge", "--no-ff", "feature-cnt"]);

            let count = commit_parent_count(Path::new("."), "master").expect("parent count");
            assert_eq!(count, 2, "merge commit should have 2 parents");
        });
    }

    #[test]
    fn test_prompt_contains_critical_rules() {
        with_temp_cwd(|| {
            fs::create_dir_all(".swarm-hug").unwrap();
            fs::write(".swarm-hug/email.txt", "dev@example.com").unwrap();

            let prompt = generate_merge_agent_prompt(
                "feature-1",
                "main",
                Path::new("/tmp/target-worktree"),
            )
            .unwrap();

            // Verify Critical Rules section exists with banned commands
            assert!(prompt.contains("Critical Rules"), "prompt should have Critical Rules section");
            assert!(prompt.contains("--squash"), "prompt should ban --squash");
            assert!(prompt.contains("cherry-pick"), "prompt should ban cherry-pick");
            assert!(prompt.contains("git rebase"), "prompt should ban rebase");
            assert!(prompt.contains("MERGE_HEAD"), "prompt should mention MERGE_HEAD guard");
            assert!(prompt.contains("rev-parse HEAD^2"), "prompt should require post-commit verification");
        });
    }

    #[test]
    fn test_prompt_contains_merge_head_recovery() {
        with_temp_cwd(|| {
            fs::create_dir_all(".swarm-hug").unwrap();
            fs::write(".swarm-hug/email.txt", "dev@example.com").unwrap();

            let prompt = generate_merge_agent_prompt(
                "feature-1",
                "main",
                Path::new("/tmp/target-worktree"),
            )
            .unwrap();

            assert!(prompt.contains("MERGE_HEAD Recovery"), "prompt should have recovery section");
            assert!(prompt.contains("Re-initiate the merge"), "prompt should describe recovery steps");
        });
    }

    #[test]
    fn test_prompt_preflight_only_aborts_stale_merges() {
        with_temp_cwd(|| {
            fs::create_dir_all(".swarm-hug").unwrap();
            fs::write(".swarm-hug/email.txt", "dev@example.com").unwrap();

            let prompt = generate_merge_agent_prompt(
                "feature-1",
                "main",
                Path::new("/tmp/target-worktree"),
            )
            .unwrap();

            assert!(prompt.contains("PRE-EXISTING stale merges"), "preflight should clarify stale-only abort");
            assert!(prompt.contains("do NOT loop back here and abort your own merge"), "preflight should warn against self-abort");
        });
    }
}
