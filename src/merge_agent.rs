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

    if !is_branch_merged(&main_repo, &feature, &target)? {
        return Err(format!(
            "feature branch '{}' is not merged into '{}'",
            feature, target
        ));
    }

    // When feature and target are different branches, verify the target tip
    // is a true merge commit (2 parents), not a squash-merge (1 parent).
    if feature != target {
        let parents = commit_parent_count(&main_repo, &target)?;
        if parents < 2 {
            return Err(format!(
                "squash-merge detected: tip of '{}' has {} parent(s) instead of 2; \
                 the merge agent likely used --squash or cherry-pick instead of --no-ff",
                target, parents
            ));
        }
    }

    Ok(())
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

fn commit_parent_count(repo_root: &Path, branch: &str) -> Result<usize, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-list", "--parents", "-n", "1", branch])
        .output()
        .map_err(|e| format!("failed to run git rev-list: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git rev-list failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let hashes: Vec<&str> = stdout.trim().split_whitespace().collect();
    // First hash is the commit itself; the rest are parents.
    Ok(if hashes.is_empty() { 0 } else { hashes.len() - 1 })
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

    #[test]
    fn test_ensure_feature_merged_detects_squash_merge() {
        with_temp_cwd(|| {
            init_repo();
            commit_on_branch("feature-squash", "feature_squash.txt");
            run_git(&["checkout", "master"]);

            // Do a proper merge first so ancestry check passes
            run_git(&["merge", "--no-ff", "feature-squash", "-m", "real merge"]);
            assert!(is_merged("feature-squash", "master"));

            // Now simulate the squash-merge scenario: add a single-parent commit
            // on top so the tip is not a merge commit.
            fs::write("extra.txt", "extra").expect("write extra");
            run_git(&["add", "."]);
            run_git(&["commit", "-m", "squash-like commit"]);

            // Ancestry still passes
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
    fn test_ensure_feature_merged_accepts_two_parent_merge() {
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
    fn test_ensure_feature_merged_same_branch_skips_parent_check() {
        with_temp_cwd(|| {
            init_repo();

            // When feature == target, the parent count check should be skipped.
            // Stub engine will attempt the merge (no-op since same branch).
            let engine = StubEngine::new("loop");
            ensure_feature_merged(&engine, "master", "master", Path::new("."))
                .expect("same-branch should pass without parent check");
        });
    }
}
