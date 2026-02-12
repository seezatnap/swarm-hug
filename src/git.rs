use std::fs;
use std::path::{Path, PathBuf};
use std::process;

pub(crate) fn git_repo_root() -> Result<PathBuf, String> {
    let output = process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("git rev-parse failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git rev-parse failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let root = stdout.trim();
    if root.is_empty() {
        return Err("git rev-parse returned empty repo root".to_string());
    }

    Ok(PathBuf::from(root))
}

fn resolve_repo_relative_path(
    path: &str,
    cwd: &Path,
    repo_root: &Path,
) -> Result<Option<(PathBuf, PathBuf)>, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let raw = Path::new(trimmed);
    let source = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    };

    if !source.exists() {
        return Ok(None);
    }

    let source = source
        .canonicalize()
        .map_err(|e| format!("failed to resolve {}: {}", source.display(), e))?;
    let repo_root = repo_root
        .canonicalize()
        .map_err(|e| format!("failed to resolve repo root: {}", e))?;

    let relative = source.strip_prefix(&repo_root).map_err(|_| {
        format!(
            "path '{}' is outside repo root '{}'",
            source.display(),
            repo_root.display()
        )
    })?;

    Ok(Some((relative.to_path_buf(), source)))
}

pub(crate) fn sync_paths_to_worktree(
    worktree_root: &Path,
    paths: &[&str],
) -> Result<Vec<String>, String> {
    let repo_root = git_repo_root()?;
    let cwd = std::env::current_dir().map_err(|e| format!("failed to get cwd: {}", e))?;
    let mut synced = Vec::new();

    // Canonicalize worktree_root for comparison
    let worktree_root_canonical = worktree_root
        .canonicalize()
        .unwrap_or_else(|_| worktree_root.to_path_buf());

    for path in paths {
        let Some((relative, source)) = resolve_repo_relative_path(path, &cwd, &repo_root)? else {
            continue;
        };

        // Check if the source file is already inside the worktree
        // If so, compute relative path from worktree root instead of repo root
        // and skip the copy (file is already in place)
        if source.starts_with(&worktree_root_canonical) {
            // File is already in the worktree - compute relative path from worktree root
            if let Ok(worktree_relative) = source.strip_prefix(&worktree_root_canonical) {
                synced.push(worktree_relative.to_string_lossy().to_string());
            }
            continue;
        }

        let dest = worktree_root.join(&relative);
        if dest != source {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create {}: {}", parent.display(), e))?;
            }
            fs::copy(&source, &dest)
                .map_err(|e| format!("failed to sync {}: {}", source.display(), e))?;
        }

        synced.push(relative.to_string_lossy().to_string());
    }

    Ok(synced)
}

pub(crate) fn commit_files_in(
    repo_dir: &Path,
    paths: &[&str],
    message: &str,
) -> Result<bool, String> {
    let existing: Vec<String> = paths
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .filter(|p| {
            let path = Path::new(p);
            let candidate = if path.is_absolute() {
                path.to_path_buf()
            } else {
                repo_dir.join(path)
            };
            candidate.exists()
        })
        .map(|p| p.to_string())
        .collect();

    if existing.is_empty() {
        return Ok(false);
    }

    let add_result = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .arg("add")
        .args(&existing)
        .output();

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
        .arg("-C")
        .arg(repo_dir)
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
        .arg("-C")
        .arg(repo_dir)
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

pub(crate) fn commit_files_in_worktree(
    worktree_root: &Path,
    paths: &[&str],
    message: &str,
) -> Result<bool, String> {
    let synced = sync_paths_to_worktree(worktree_root, paths)?;
    let synced_refs: Vec<&str> = synced.iter().map(String::as_str).collect();
    commit_files_in(worktree_root, &synced_refs, message)
}

fn ensure_branch_checked_out(repo_dir: &Path, branch: &str) -> Result<(), String> {
    let target = branch.trim();
    if target.is_empty() {
        return Err("branch name is empty".to_string());
    }

    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(|e| format!("git rev-parse failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git rev-parse failed: {}", stderr.trim()));
    }

    let current = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if current == target {
        return Ok(());
    }

    let checkout = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["checkout", target])
        .output()
        .map_err(|e| format!("git checkout failed: {}", e))?;

    if checkout.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&checkout.stderr);
        Err(format!("git checkout failed: {}", stderr.trim()))
    }
}

pub(crate) fn commit_files_in_worktree_on_branch(
    worktree_root: &Path,
    branch: &str,
    paths: &[&str],
    message: &str,
) -> Result<bool, String> {
    ensure_branch_checked_out(worktree_root, branch)?;
    commit_files_in_worktree(worktree_root, paths, message)
}

/// Commit task assignment changes to git.
///
/// # Arguments
/// * `sprint_branch` - Sprint/feature branch name to commit on
/// * `tasks_file` - Path to the team's tasks.md file
/// * `team_name` - Formatted team name for commit message (e.g., "Greenfield")
/// * `sprint_number` - The historical sprint number for this team
pub(crate) fn commit_task_assignments(
    worktree_root: &Path,
    sprint_branch: &str,
    tasks_file: &str,
    team_name: &str,
    sprint_number: usize,
) -> Result<(), String> {
    let commit_msg = format!("{} Sprint {}: task assignments", team_name, sprint_number);
    if commit_files_in_worktree_on_branch(worktree_root, sprint_branch, &[tasks_file], &commit_msg)?
    {
        println!("  Committed task assignments to git.");
    }
    Ok(())
}

/// Commit sprint completion (updated tasks).
///
/// # Arguments
/// * `sprint_branch` - Sprint/feature branch name to commit on
/// * `tasks_file` - Path to the team's tasks.md file
/// * `team_name` - Formatted team name for commit message (e.g., "Greenfield")
/// * `sprint_number` - The historical sprint number for this team
pub(crate) fn commit_sprint_completion(
    worktree_root: &Path,
    sprint_branch: &str,
    tasks_file: &str,
    team_name: &str,
    sprint_number: usize,
) -> Result<(), String> {
    let commit_msg = format!("{} Sprint {}: completed", team_name, sprint_number);
    if commit_files_in_worktree_on_branch(worktree_root, sprint_branch, &[tasks_file], &commit_msg)?
    {
        println!("  Committed sprint completion to git.");
    }
    Ok(())
}

/// Get the current git commit hash from a specific repo/worktree.
pub(crate) fn get_current_commit_in(repo_dir: &Path) -> Option<String> {
    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get the short git commit hash for a ref (branch, tag, or commit) in a repo/worktree.
pub(crate) fn get_short_commit_for_ref_in(repo_dir: &Path, git_ref: &str) -> Option<String> {
    let target = git_ref.trim();
    if target.is_empty() {
        return None;
    }

    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["rev-parse", "--short", target])
        .output()
        .ok()?;

    if output.status.success() {
        let short = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if short.is_empty() {
            None
        } else {
            Some(short)
        }
    } else {
        None
    }
}

/// Get git log between two commits (messages and stats, no diffs) for a specific repo/worktree.
pub(crate) fn get_git_log_range_in(
    repo_dir: &Path,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let range = format!("{}..{}", from, to);
    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PushBranchResult {
    pub success: bool,
    pub branch: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

impl PushBranchResult {
    fn from_output(branch: String, output: process::Output) -> Self {
        let success = output.status.success();
        let exit_code = output.status.code();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let error = if success {
            None
        } else {
            Some(format!(
                "git push failed with exit code {}",
                exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ))
        };

        Self {
            success,
            branch,
            exit_code,
            stdout,
            stderr,
            error,
        }
    }

    fn failure(branch: String, error: impl Into<String>) -> Self {
        Self {
            success: false,
            branch,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(error.into()),
        }
    }
}

pub(crate) fn push_branch_to_remote(repo_dir: &Path, target_branch: &str) -> PushBranchResult {
    let branch = target_branch.trim().to_string();
    if branch.is_empty() {
        return PushBranchResult::failure(branch, "target branch name is empty");
    }

    match process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["push", "origin", branch.as_str()])
        .output()
    {
        Ok(output) => PushBranchResult::from_output(branch, output),
        Err(e) => PushBranchResult::failure(branch, format!("failed to run git push: {}", e)),
    }
}

/// Get a one-line commit log between two refs (`source..target`) for PR metadata generation.
pub(crate) fn get_commit_log_between(
    repo_dir: &Path,
    source_branch: &str,
    target_branch: &str,
) -> Result<String, String> {
    let source = source_branch.trim();
    let target = target_branch.trim();
    if source.is_empty() || target.is_empty() {
        return Err("source and target branch names must be non-empty".to_string());
    }

    let range = format!("{}..{}", source, target);
    let output = process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["log", "--oneline", &range])
        .output()
        .map_err(|e| format!("failed to run git log --oneline: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git log --oneline failed: {}", stderr.trim()))
    }
}

/// Result of attempting to create a pull request with GitHub CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PullRequestCreateResult {
    /// Pull request command succeeded.
    Created {
        /// PR URL parsed from stdout (usually first non-empty line).
        url: Option<String>,
        /// Raw stdout from `gh pr create`.
        stdout: String,
        /// Raw stderr from `gh pr create`.
        stderr: String,
    },
    /// Pull request command ran but failed.
    Failed {
        /// Raw stdout from `gh pr create`.
        stdout: String,
        /// Raw stderr from `gh pr create`.
        stderr: String,
        /// Process exit code if available.
        exit_code: Option<i32>,
    },
    /// Pull request creation was intentionally skipped.
    Skipped {
        /// Human-readable reason for skip.
        reason: String,
    },
}

#[allow(dead_code)]
fn extract_pull_request_url(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

fn gh_probe_command_for_platform(is_windows: bool) -> &'static str {
    if is_windows {
        "where"
    } else {
        "which"
    }
}

fn gh_probe_command() -> &'static str {
    gh_probe_command_for_platform(cfg!(windows))
}

#[cfg_attr(not(test), allow(dead_code))]
fn create_pull_request_with_commands(
    title: &str,
    body: &str,
    source_branch: &str,
    target_branch: &str,
    probe_command: &str,
    gh_command: &str,
) -> PullRequestCreateResult {
    let probe_output = process::Command::new(probe_command)
        .arg(gh_command)
        .output();

    match probe_output {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let reason = if stderr.is_empty() {
                format!("'{}' not found on PATH", gh_command)
            } else {
                format!("'{}' not found on PATH: {}", gh_command, stderr)
            };
            return PullRequestCreateResult::Skipped { reason };
        }
        Err(e) => {
            return PullRequestCreateResult::Skipped {
                reason: format!("failed to check for '{}': {}", gh_command, e),
            };
        }
    }

    let output = process::Command::new(gh_command)
        .args([
            "pr",
            "create",
            "--title",
            title,
            "--body",
            body,
            "--base",
            source_branch,
            "--head",
            target_branch,
        ])
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                PullRequestCreateResult::Created {
                    url: extract_pull_request_url(&stdout),
                    stdout,
                    stderr,
                }
            } else {
                PullRequestCreateResult::Failed {
                    stdout,
                    stderr,
                    exit_code: output.status.code(),
                }
            }
        }
        Err(e) => PullRequestCreateResult::Failed {
            stdout: String::new(),
            stderr: format!("failed to run gh pr create: {}", e),
            exit_code: None,
        },
    }
}

#[allow(dead_code)]
pub(crate) fn create_pull_request(
    title: &str,
    body: &str,
    source_branch: &str,
    target_branch: &str,
) -> PullRequestCreateResult {
    create_pull_request_with_commands(
        title,
        body,
        source_branch,
        target_branch,
        gh_probe_command(),
        "gh",
    )
}
pub(crate) const MIN_GIT_VERSION: (u32, u32, u32) = (2, 48, 0);

pub(crate) fn ensure_min_git_version() -> Result<(), String> {
    let output = process::Command::new("git")
        .arg("--version")
        .output()
        .map_err(|e| format!("failed to run git --version: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git --version failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let current = parse_git_version(&stdout)
        .ok_or_else(|| format!("could not parse git version from '{}'", stdout.trim()))?;

    if version_lt(current, MIN_GIT_VERSION) {
        return Err(format!(
            "git {}.{}.{}+ required for relative worktree paths; found {}.{}.{} (please upgrade git)",
            MIN_GIT_VERSION.0,
            MIN_GIT_VERSION.1,
            MIN_GIT_VERSION.2,
            current.0,
            current.1,
            current.2
        ));
    }

    Ok(())
}

fn parse_git_version(output: &str) -> Option<(u32, u32, u32)> {
    let token = output.split_whitespace().find(|part| {
        part.chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
    })?;

    let mut nums = Vec::new();
    for part in token.split('.') {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            break;
        }
        if let Ok(value) = digits.parse() {
            nums.push(value);
        } else {
            break;
        }
    }

    if nums.len() < 2 {
        return None;
    }

    let major = nums[0];
    let minor = nums[1];
    let patch = nums.get(2).copied().unwrap_or(0);
    Some((major, minor, patch))
}

fn version_lt(current: (u32, u32, u32), min: (u32, u32, u32)) -> bool {
    current.0 < min.0
        || (current.0 == min.0 && (current.1 < min.1 || (current.1 == min.1 && current.2 < min.2)))
}

#[cfg(test)]
mod tests {
    use super::{
        create_pull_request_with_commands, ensure_branch_checked_out, get_commit_log_between,
        get_short_commit_for_ref_in, gh_probe_command_for_platform, push_branch_to_remote,
        PullRequestCreateResult,
    };
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn run_git(repo_dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_dir)
            .args(args)
            .output()
            .expect("failed to run git");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    #[test]
    fn test_ensure_branch_checked_out_switches_branch() {
        let temp = TempDir::new().expect("temp dir");
        let repo_dir = temp.path();

        run_git(repo_dir, &["init"]);
        run_git(repo_dir, &["config", "user.name", "Swarm Test"]);
        run_git(
            repo_dir,
            &["config", "user.email", "swarm-test@example.com"],
        );

        fs::write(repo_dir.join("README.md"), "hello").expect("write file");
        run_git(repo_dir, &["add", "."]);
        run_git(repo_dir, &["commit", "-m", "init"]);
        run_git(repo_dir, &["branch", "feature"]);

        ensure_branch_checked_out(repo_dir, "feature").expect("should checkout feature branch");
        let branch = run_git(repo_dir, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(branch.trim(), "feature");
    }

    #[test]
    fn test_get_short_commit_for_ref_in_returns_short_hash() {
        let temp = TempDir::new().expect("temp dir");
        let repo_dir = temp.path();

        run_git(repo_dir, &["init"]);
        run_git(repo_dir, &["config", "user.name", "Swarm Test"]);
        run_git(
            repo_dir,
            &["config", "user.email", "swarm-test@example.com"],
        );
        fs::write(repo_dir.join("README.md"), "hello").expect("write file");
        run_git(repo_dir, &["add", "."]);
        run_git(repo_dir, &["commit", "-m", "init"]);

        let full = run_git(repo_dir, &["rev-parse", "HEAD"]).trim().to_string();
        let short =
            get_short_commit_for_ref_in(repo_dir, "HEAD").expect("short commit should exist");
        assert!(!short.is_empty());
        assert!(full.starts_with(&short));
    }

    #[test]
    fn test_push_branch_to_remote_pushes_requested_branch() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path();
        let repo_dir = root.join("local");
        let remote_dir = root.join("remote.git");

        fs::create_dir_all(&repo_dir).expect("create local repo dir");
        run_git(
            root,
            &["init", "--bare", remote_dir.to_str().expect("remote path")],
        );
        run_git(&repo_dir, &["init"]);
        run_git(&repo_dir, &["config", "user.name", "Swarm Test"]);
        run_git(
            &repo_dir,
            &["config", "user.email", "swarm-test@example.com"],
        );

        fs::write(repo_dir.join("README.md"), "hello").expect("write file");
        run_git(&repo_dir, &["add", "."]);
        run_git(&repo_dir, &["commit", "-m", "init"]);

        run_git(
            &repo_dir,
            &[
                "remote",
                "add",
                "origin",
                remote_dir.to_str().expect("remote path"),
            ],
        );
        run_git(&repo_dir, &["checkout", "-b", "release"]);
        fs::write(repo_dir.join("release.txt"), "release").expect("write file");
        run_git(&repo_dir, &["add", "."]);
        run_git(&repo_dir, &["commit", "-m", "release work"]);

        run_git(&repo_dir, &["checkout", "-b", "other"]);
        fs::write(repo_dir.join("other.txt"), "other").expect("write file");
        run_git(&repo_dir, &["add", "."]);
        run_git(&repo_dir, &["commit", "-m", "other work"]);

        let result = push_branch_to_remote(&repo_dir, "release");
        assert!(
            result.success,
            "expected push success, got error: {:?}\nstderr:\n{}",
            result.error, result.stderr
        );
        assert_eq!(result.branch, "release");
        assert_eq!(result.exit_code, Some(0));
        assert!(result.error.is_none());

        let release_ref = Command::new("git")
            .arg("--git-dir")
            .arg(&remote_dir)
            .args(["show-ref", "--verify", "refs/heads/release"])
            .output()
            .expect("check release ref");
        assert!(
            release_ref.status.success(),
            "release branch missing on remote\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&release_ref.stdout),
            String::from_utf8_lossy(&release_ref.stderr)
        );

        let other_ref = Command::new("git")
            .arg("--git-dir")
            .arg(&remote_dir)
            .args(["show-ref", "--verify", "refs/heads/other"])
            .output()
            .expect("check other ref");
        assert!(
            !other_ref.status.success(),
            "unexpectedly pushed 'other' branch\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&other_ref.stdout),
            String::from_utf8_lossy(&other_ref.stderr)
        );
    }

    #[test]
    fn test_push_branch_to_remote_captures_failure_details() {
        let temp = TempDir::new().expect("temp dir");
        let repo_dir = temp.path();

        run_git(repo_dir, &["init"]);
        run_git(repo_dir, &["config", "user.name", "Swarm Test"]);
        run_git(
            repo_dir,
            &["config", "user.email", "swarm-test@example.com"],
        );
        fs::write(repo_dir.join("README.md"), "hello").expect("write file");
        run_git(repo_dir, &["add", "."]);
        run_git(repo_dir, &["commit", "-m", "init"]);

        let result = push_branch_to_remote(repo_dir, "main");
        assert!(!result.success, "push should fail without origin remote");
        assert_eq!(result.branch, "main");
        assert!(result.exit_code.is_some());
        assert!(
            !result.stderr.trim().is_empty(),
            "stderr should be captured"
        );
        assert!(
            result.error.is_some(),
            "failure should include structured error"
        );
    }

    #[test]
    fn test_push_branch_to_remote_rejects_empty_branch_name() {
        let temp = TempDir::new().expect("temp dir");
        let result = push_branch_to_remote(temp.path(), "   ");
        assert!(!result.success);
        assert_eq!(result.exit_code, None);
        assert_eq!(result.branch, "");
        assert_eq!(result.error.as_deref(), Some("target branch name is empty"));
    }

    #[test]
    fn test_get_commit_log_between_returns_oneline_log() {
        let temp = TempDir::new().expect("temp dir");
        let repo_dir = temp.path();

        run_git(repo_dir, &["init"]);
        run_git(repo_dir, &["config", "user.name", "Swarm Test"]);
        run_git(
            repo_dir,
            &["config", "user.email", "swarm-test@example.com"],
        );

        fs::write(repo_dir.join("README.md"), "init").expect("write file");
        run_git(repo_dir, &["add", "."]);
        run_git(repo_dir, &["commit", "-m", "init"]);
        run_git(repo_dir, &["branch", "-M", "main"]);

        run_git(repo_dir, &["checkout", "-b", "source-branch"]);
        fs::write(repo_dir.join("source.txt"), "source").expect("write source file");
        run_git(repo_dir, &["add", "."]);
        run_git(repo_dir, &["commit", "-m", "source commit"]);

        run_git(repo_dir, &["checkout", "-b", "target-branch"]);
        fs::write(repo_dir.join("target.txt"), "target").expect("write target file");
        run_git(repo_dir, &["add", "."]);
        run_git(repo_dir, &["commit", "-m", "target commit"]);

        let log = get_commit_log_between(repo_dir, "source-branch", "target-branch")
            .expect("get commit log");
        assert!(
            log.contains("target commit"),
            "expected target commit in oneline log, got: {}",
            log
        );
        assert!(
            !log.contains("source commit"),
            "range source..target should not include source-only commit, got: {}",
            log
        );
    }

    #[test]
    fn test_parse_git_version_accepts_standard_output() {
        let parsed = super::parse_git_version("git version 2.48.1").expect("parse version");
        assert_eq!(parsed, (2, 48, 1));
    }

    #[test]
    fn test_parse_git_version_accepts_suffix() {
        let parsed =
            super::parse_git_version("git version 2.48.0.windows.1").expect("parse version");
        assert_eq!(parsed, (2, 48, 0));
    }

    #[test]
    fn test_gh_probe_command_for_platform_uses_windows_where() {
        assert_eq!(gh_probe_command_for_platform(true), "where");
        assert_eq!(gh_probe_command_for_platform(false), "which");
    }

    #[cfg(unix)]
    fn write_executable_script(path: &Path, content: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, content).expect("write script");
        let mut perms = fs::metadata(path).expect("script metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("set script permissions");
    }

    #[test]
    #[cfg(unix)]
    fn test_create_pull_request_builds_expected_command() {
        let temp = TempDir::new().expect("temp dir");
        let which_path = temp.path().join("which-gh");
        let gh_path = temp.path().join("gh");
        let args_path = temp.path().join("gh-args.txt");

        write_executable_script(&which_path, "#!/bin/sh\necho \"$1\"\n");
        write_executable_script(
            &gh_path,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\necho \"https://github.com/example/repo/pull/42\"\n",
                args_path.display()
            ),
        );

        let result = create_pull_request_with_commands(
            "Add sprint automation",
            "Generated body",
            "source-branch",
            "target-branch",
            which_path.to_str().expect("which path"),
            gh_path.to_str().expect("gh path"),
        );

        match result {
            PullRequestCreateResult::Created {
                url,
                stdout,
                stderr,
            } => {
                assert_eq!(
                    url,
                    Some("https://github.com/example/repo/pull/42".to_string())
                );
                assert!(stdout.contains("https://github.com/example/repo/pull/42"));
                assert!(stderr.is_empty(), "unexpected stderr: {}", stderr);
            }
            other => panic!("expected Created, got {:?}", other),
        }

        let args_file = fs::read_to_string(&args_path).expect("read gh args");
        let args: Vec<&str> = args_file.lines().collect();
        assert_eq!(
            args,
            vec![
                "pr",
                "create",
                "--title",
                "Add sprint automation",
                "--body",
                "Generated body",
                "--base",
                "source-branch",
                "--head",
                "target-branch",
            ]
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_create_pull_request_supports_windows_probe_command() {
        let temp = TempDir::new().expect("temp dir");
        let where_path = temp.path().join("where-gh");
        let where_args_path = temp.path().join("where-args.txt");
        let gh_path = temp.path().join("gh");

        write_executable_script(
            &where_path,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n",
                where_args_path.display()
            ),
        );
        write_executable_script(
            &gh_path,
            "#!/bin/sh\necho \"https://github.com/example/repo/pull/99\"\n",
        );

        let result = create_pull_request_with_commands(
            "title",
            "body",
            "source",
            "target",
            where_path.to_str().expect("where path"),
            gh_path.to_str().expect("gh path"),
        );

        match result {
            PullRequestCreateResult::Created { url, .. } => {
                assert_eq!(
                    url,
                    Some("https://github.com/example/repo/pull/99".to_string())
                );
            }
            other => panic!("expected Created, got {:?}", other),
        }

        let probe_args_file = fs::read_to_string(&where_args_path).expect("read where args");
        let probe_args: Vec<&str> = probe_args_file.lines().collect();
        assert_eq!(probe_args, vec![gh_path.to_str().expect("gh path")]);
    }

    #[test]
    #[cfg(unix)]
    fn test_create_pull_request_skips_when_gh_missing() {
        let temp = TempDir::new().expect("temp dir");
        let which_path = temp.path().join("which-missing-gh");
        let gh_path = temp.path().join("gh");
        let marker_path = temp.path().join("gh-called.txt");

        write_executable_script(&which_path, "#!/bin/sh\nexit 1\n");
        write_executable_script(
            &gh_path,
            &format!("#!/bin/sh\necho called > \"{}\"\n", marker_path.display()),
        );

        let result = create_pull_request_with_commands(
            "title",
            "body",
            "source",
            "target",
            which_path.to_str().expect("which path"),
            gh_path.to_str().expect("gh path"),
        );

        match result {
            PullRequestCreateResult::Skipped { reason } => {
                assert!(
                    reason.contains("not found on PATH"),
                    "unexpected skip reason: {}",
                    reason
                );
            }
            other => panic!("expected Skipped, got {:?}", other),
        }
        assert!(!marker_path.exists(), "gh should not have been executed");
    }

    #[test]
    #[cfg(unix)]
    fn test_create_pull_request_returns_failure_data() {
        let temp = TempDir::new().expect("temp dir");
        let which_path = temp.path().join("which-gh");
        let gh_path = temp.path().join("gh-fail");

        write_executable_script(&which_path, "#!/bin/sh\necho \"$1\"\n");
        write_executable_script(
            &gh_path,
            "#!/bin/sh\necho \"validation failed\" 1>&2\nexit 1\n",
        );

        let result = create_pull_request_with_commands(
            "title",
            "body",
            "source",
            "target",
            which_path.to_str().expect("which path"),
            gh_path.to_str().expect("gh path"),
        );

        match result {
            PullRequestCreateResult::Failed {
                stdout,
                stderr,
                exit_code,
            } => {
                assert!(stdout.is_empty(), "unexpected stdout: {}", stdout);
                assert!(stderr.contains("validation failed"));
                assert_eq!(exit_code, Some(1));
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }
}
