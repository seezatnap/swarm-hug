use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::create::worktree_is_registered;
use super::git::{
    ensure_head, git_repo_root, prune_stale_worktree_registrations, reconcile_worktree_registration,
    repair_worktree_links,
};

/// Returns the shared worktrees root for target branch operations.
///
/// Path: `./.swarm-hug/.shared/worktrees` (relative to the repo root).
pub fn shared_worktrees_root(repo_root: &Path) -> PathBuf {
    repo_root
        .join(".swarm-hug")
        .join(".shared")
        .join("worktrees")
}

/// Ensure the shared worktrees root exists before target worktree operations.
pub fn ensure_shared_worktrees_root(repo_root: &Path) -> Result<PathBuf, String> {
    let root = shared_worktrees_root(repo_root);
    fs::create_dir_all(&root).map_err(|e| {
        format!(
            "failed to create shared worktrees dir {}: {}",
            root.display(),
            e
        )
    })?;
    Ok(root)
}

/// Find the worktree path for the target branch, if any.
pub fn find_target_branch_worktree(target_branch: &str) -> Result<Option<PathBuf>, String> {
    let repo_root = git_repo_root()?;
    find_target_branch_worktree_in(&repo_root, target_branch)
}

/// Find the worktree path for the target branch in the specified repo, if any.
pub fn find_target_branch_worktree_in(
    repo_root: &Path,
    target_branch: &str,
) -> Result<Option<PathBuf>, String> {
    let target = normalize_target_branch(target_branch)?;
    find_target_branch_worktree_in_normalized(repo_root, target)
}

fn normalize_target_branch(target_branch: &str) -> Result<&str, String> {
    let target = target_branch.trim();
    if target.is_empty() {
        return Err("target branch name is empty".to_string());
    }
    let target = target.strip_prefix("refs/heads/").unwrap_or(target);
    if target.is_empty() {
        return Err("target branch name is empty".to_string());
    }
    Ok(target)
}

#[derive(Debug, Clone)]
struct WorktreeRegistration {
    path: PathBuf,
    branch: Option<String>,
}

fn resolve_worktree_path(repo_root: &Path, worktree_path: &str) -> PathBuf {
    let candidate = PathBuf::from(worktree_path);
    if candidate.is_absolute() {
        candidate
    } else {
        repo_root.join(candidate)
    }
}

fn parse_worktree_registrations(
    porcelain_output: &str,
    repo_root: &Path,
) -> Vec<WorktreeRegistration> {
    let mut registrations = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;

    for line in porcelain_output.lines().chain(std::iter::once("")) {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(path) = current_path.take() {
                registrations.push(WorktreeRegistration {
                    path,
                    branch: current_branch.take(),
                });
            }
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                current_path = Some(resolve_worktree_path(repo_root, trimmed));
            }
            current_branch = None;
        } else if let Some(branch_ref) = line.strip_prefix("branch refs/heads/") {
            let trimmed = branch_ref.trim();
            if current_path.is_some() && !trimmed.is_empty() {
                current_branch = Some(trimmed.to_string());
            }
        } else if line.is_empty() {
            if let Some(path) = current_path.take() {
                registrations.push(WorktreeRegistration {
                    path,
                    branch: current_branch.take(),
                });
            }
            current_branch = None;
        }
    }

    registrations
}

fn worktree_paths_match(repo_root: &Path, expected: &Path, registered: &Path) -> bool {
    let expected_abs = if expected.is_absolute() {
        expected.to_path_buf()
    } else {
        repo_root.join(expected)
    };
    let registered_abs = if registered.is_absolute() {
        registered.to_path_buf()
    } else {
        repo_root.join(registered)
    };

    if expected_abs == registered_abs {
        return true;
    }

    match (expected_abs.canonicalize(), registered_abs.canonicalize()) {
        (Ok(expected), Ok(registered)) => expected == registered,
        _ => false,
    }
}

fn find_worktree_registration_for_path(
    repo_root: &Path,
    expected_path: &Path,
) -> Result<Option<WorktreeRegistration>, String> {
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
    let registrations = parse_worktree_registrations(&stdout, repo_root);
    Ok(registrations
        .into_iter()
        .find(|reg| worktree_paths_match(repo_root, expected_path, &reg.path)))
}

fn recover_stale_registration(
    repo_root: &Path,
    path: &Path,
    expected_branch: &str,
) -> Result<(), String> {
    let Some(registration) = find_worktree_registration_for_path(repo_root, path)? else {
        return Ok(());
    };

    if registration.branch.as_deref() == Some(expected_branch) {
        return Ok(());
    }

    let registered_path = registration.path.to_string_lossy().to_string();
    let remove_output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "remove", "--force", &registered_path])
        .output()
        .map_err(|e| format!("failed to run git worktree remove: {}", e))?;

    if !remove_output.status.success() {
        let prune_output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["worktree", "prune", "--expire", "now"])
            .output()
            .map_err(|e| format!("failed to run git worktree prune: {}", e))?;
        if !prune_output.status.success() {
            let remove_stderr = String::from_utf8_lossy(&remove_output.stderr);
            let prune_stderr = String::from_utf8_lossy(&prune_output.stderr);
            return Err(format!(
                "failed to force-remove stale worktree registration '{}': remove failed ({}) and prune failed ({})",
                registration.path.display(),
                remove_stderr.trim(),
                prune_stderr.trim()
            ));
        }
    }

    if worktree_is_registered(repo_root, path)? {
        let found = find_worktree_registration_for_path(repo_root, path)?
            .and_then(|reg| reg.branch)
            .unwrap_or_else(|| "unknown".to_string());
        return Err(format!(
            "stale worktree path '{}' is still registered to '{}' (expected '{}')",
            path.display(),
            found,
            expected_branch
        ));
    }

    Ok(())
}

/// Validate that the target branch worktree (if any) is under the shared root.
///
/// Returns the worktree path when it exists under the shared root, Ok(None) if
/// no worktree exists, and Err if the worktree exists outside the shared root.
pub fn validate_target_branch_worktree(target_branch: &str) -> Result<Option<PathBuf>, String> {
    let repo_root = git_repo_root()?;
    validate_target_branch_worktree_in(&repo_root, target_branch)
}

/// Validate that the target branch worktree (if any) is under the shared root.
pub fn validate_target_branch_worktree_in(
    repo_root: &Path,
    target_branch: &str,
) -> Result<Option<PathBuf>, String> {
    let target = normalize_target_branch(target_branch)?;
    prune_stale_worktree_registrations(repo_root)?;
    let shared_root = ensure_shared_worktrees_root(repo_root)?;
    let existing = find_target_branch_worktree_in_normalized(repo_root, target)?;

    if let Some(path) = existing {
        if path_is_under_root(&path, &shared_root) {
            return Ok(Some(path));
        }
        if is_repo_root_worktree(repo_root, &path) {
            return Ok(Some(path));
        }
        return Err(format!(
            "target branch '{}' already has a worktree at '{}' outside shared worktrees root '{}'",
            target,
            path.display(),
            shared_root.display()
        ));
    }

    Ok(None)
}

/// Create (or reuse) the target branch worktree under the shared root.
///
/// If an existing worktree for the target branch is already under the shared root,
/// it is reused. If a worktree exists elsewhere, this errors. If no worktree exists,
/// a new one is created at `./.swarm-hug/.shared/worktrees/<sanitized-target>`.
pub fn create_target_branch_worktree(target_branch: &str) -> Result<PathBuf, String> {
    let repo_root = git_repo_root()?;
    create_target_branch_worktree_in(&repo_root, target_branch)
}

/// Create (or reuse) the target branch worktree under the shared root in the specified repo.
pub fn create_target_branch_worktree_in(
    repo_root: &Path,
    target_branch: &str,
) -> Result<PathBuf, String> {
    let target = normalize_target_branch(target_branch)?;

    if let Some(existing) = validate_target_branch_worktree_in(repo_root, target)? {
        if !is_repo_root_worktree(repo_root, &existing) {
            repair_worktree_links(repo_root, &existing).map_err(|e| {
                format!(
                    "git worktree repair failed for {}: {}",
                    existing.display(),
                    e
                )
            })?;
        }
        return Ok(existing);
    }

    let shared_root = ensure_shared_worktrees_root(repo_root)?;
    ensure_head(repo_root)?;

    let sanitized = sanitize_target_branch_component(target);
    let path = shared_root.join(&sanitized);
    let path_str = path.to_string_lossy().to_string();

    reconcile_worktree_registration(repo_root, &path, target)?;

    if worktree_is_registered(repo_root, &path)? {
        return Err(format!(
            "worktree path '{}' is already registered for another branch",
            path.display()
        ));
    }

    if path.exists() {
        fs::remove_dir_all(&path).map_err(|e| {
            format!(
                "failed to remove stale worktree dir {}: {}",
                path.display(),
                e
            )
        })?;
    }

    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo_root)
        .args(["worktree", "add", "--relative-paths"]);

    if branch_exists(repo_root, target)? {
        cmd.arg(&path_str).arg(target);
    } else {
        cmd.args(["-b", target, &path_str, "HEAD"]);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("failed to run git worktree add: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git worktree add failed for {}: {}",
            path.display(),
            stderr.trim()
        ));
    }

    repair_worktree_links(repo_root, &path)
        .map_err(|e| format!("git worktree repair failed for {}: {}", path.display(), e))?;

    Ok(path)
}

/// Parse `git worktree list --porcelain` output to find the worktree path
/// for a specific target branch.
fn parse_target_worktree_path(
    porcelain_output: &str,
    target_branch: &str,
    repo_root: &Path,
) -> Option<PathBuf> {
    let mut current_path: Option<&str> = None;

    for line in porcelain_output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.trim());
        } else if let Some(branch_ref) = line.strip_prefix("branch refs/heads/") {
            if branch_ref.trim() == target_branch {
                if let Some(path) = current_path {
                    let candidate = PathBuf::from(path);
                    return Some(if candidate.is_absolute() {
                        candidate
                    } else {
                        repo_root.join(candidate)
                    });
                }
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    None
}

fn find_target_branch_worktree_in_normalized(
    repo_root: &Path,
    target_branch: &str,
) -> Result<Option<PathBuf>, String> {
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
    Ok(parse_target_worktree_path(
        &stdout,
        target_branch,
        repo_root,
    ))
}

fn sanitize_target_branch_component(target_branch: &str) -> String {
    let mut sanitized = String::with_capacity(target_branch.len());
    for byte in target_branch.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            sanitized.push(ch);
        } else {
            sanitized.push('%');
            sanitized.push(hex_char(byte >> 4));
            sanitized.push(hex_char(byte & 0x0f));
        }
    }
    if sanitized.is_empty() {
        "target".to_string()
    } else {
        sanitized
    }
}

fn hex_char(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + (nibble - 10)) as char,
        _ => '0',
    }
}

fn branch_exists(repo_root: &Path, branch: &str) -> Result<bool, String> {
    let ref_name = format!("refs/heads/{}", branch);
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["show-ref", "--verify", "--quiet", &ref_name])
        .output()
        .map_err(|e| format!("failed to run git show-ref: {}", e))?;

    if output.status.success() {
        return Ok(true);
    }
    match output.status.code() {
        Some(1) => Ok(false),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("git show-ref failed: {}", stderr.trim()))
        }
    }
}

fn path_is_under_root(path: &Path, root: &Path) -> bool {
    let root_canonical = match root.canonicalize() {
        Ok(root) => root,
        Err(_) => return false,
    };
    let path_canonical = match path.canonicalize() {
        Ok(path) => path,
        Err(_) => return false,
    };

    path_canonical.starts_with(&root_canonical)
}

fn is_repo_root_worktree(repo_root: &Path, path: &Path) -> bool {
    let repo_canonical = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let path_canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    repo_canonical == path_canonical
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn test_shared_worktrees_root_path() {
        let temp = TempDir::new().expect("temp dir");
        let root = shared_worktrees_root(temp.path());
        let expected = temp
            .path()
            .join(".swarm-hug")
            .join(".shared")
            .join("worktrees");
        assert_eq!(root, expected);
    }

    #[test]
    fn test_ensure_shared_worktrees_root_creates_dir() {
        let temp = TempDir::new().expect("temp dir");
        let root = ensure_shared_worktrees_root(temp.path()).expect("create shared root");
        assert!(root.exists(), "shared worktrees root should exist");
        assert!(root.is_dir(), "shared worktrees root should be a directory");
    }

    #[test]
    fn test_parse_target_worktree_path_finds_match() {
        let porcelain = "\\
worktree /repo
HEAD abc123
branch refs/heads/main

worktree /repo/.swarm-hug/.shared/worktrees/develop
HEAD def456
branch refs/heads/develop

";
        let repo_root = Path::new("/repo");
        let result =
            parse_target_worktree_path(porcelain, "develop", repo_root).expect("path found");
        assert_eq!(
            result,
            PathBuf::from("/repo/.swarm-hug/.shared/worktrees/develop")
        );
    }

    #[test]
    fn test_parse_target_worktree_path_resolves_relative() {
        let porcelain = "\\
worktree .swarm-hug/.shared/worktrees/main
HEAD abc123
branch refs/heads/main

";
        let repo_root = Path::new("/repo");
        let result = parse_target_worktree_path(porcelain, "main", repo_root).expect("path found");
        assert_eq!(
            result,
            PathBuf::from("/repo/.swarm-hug/.shared/worktrees/main")
        );
    }

    #[test]
    fn test_parse_target_worktree_path_no_match() {
        let porcelain = "\\
worktree /repo
HEAD abc123
branch refs/heads/main

";
        let repo_root = Path::new("/repo");
        let result = parse_target_worktree_path(porcelain, "develop", repo_root);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_worktree_registrations_tracks_path_and_branch() {
        let porcelain = "\\
worktree /repo
HEAD abc123
branch refs/heads/main

worktree .swarm-hug/.shared/worktrees/target
HEAD def456
branch refs/heads/target-branch

worktree /repo/.swarm-hug/.shared/worktrees/detached
HEAD 123abc
detached

";
        let regs = parse_worktree_registrations(porcelain, Path::new("/repo"));
        assert_eq!(regs.len(), 3);
        assert_eq!(regs[0].path, PathBuf::from("/repo"));
        assert_eq!(regs[0].branch.as_deref(), Some("main"));
        assert_eq!(
            regs[1].path,
            PathBuf::from("/repo/.swarm-hug/.shared/worktrees/target")
        );
        assert_eq!(regs[1].branch.as_deref(), Some("target-branch"));
        assert_eq!(
            regs[2].path,
            PathBuf::from("/repo/.swarm-hug/.shared/worktrees/detached")
        );
        assert!(regs[2].branch.is_none());
    }

    #[test]
    fn test_path_is_under_root_true_for_child() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path().join("root");
        let child = root.join("branch");
        std::fs::create_dir_all(&child).expect("create child");

        assert!(path_is_under_root(&child, &root));
    }

    #[test]
    fn test_path_is_under_root_false_for_sibling() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path().join("root");
        let sibling = temp.path().join("other");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&sibling).expect("create sibling");

        assert!(!path_is_under_root(&sibling, &root));
    }

    #[test]
    fn test_path_is_under_root_false_for_parent_escape() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");
        let escaped = root.join("..").join("outside");

        assert!(!path_is_under_root(&escaped, &root));
    }

    #[test]
    fn test_normalize_target_branch_strips_refs_heads() {
        let normalized = normalize_target_branch("refs/heads/main").expect("normalize branch");
        assert_eq!(normalized, "main");
    }

    #[test]
    fn test_normalize_target_branch_rejects_empty_ref() {
        let err = normalize_target_branch("refs/heads/").expect_err("should error");
        assert_eq!(err, "target branch name is empty");
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("failed to run git command");
        assert!(
            output.status.success(),
            "git -C {} {:?} failed\nstdout:\n{}\nstderr:\n{}",
            repo.display(),
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(repo: &Path) {
        run_git(repo, &["init"]);
        run_git(repo, &["config", "user.name", "Swarm Test"]);
        run_git(repo, &["config", "user.email", "swarm-test@example.com"]);
        std::fs::write(repo.join("README.md"), "init").expect("write README");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "init"]);
    }

    fn git_stdout(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("failed to run git command");
        assert!(
            output.status.success(),
            "git -C {} {:?} failed\nstdout:\n{}\nstderr:\n{}",
            repo.display(),
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn rev_parse(repo: &Path, rev: &str) -> String {
        git_stdout(repo, &["rev-parse", rev])
    }

    fn make_worktree_gitdir_relative(repo: &Path, worktree_path: &Path) {
        let worktrees_dir = repo.join(".git").join("worktrees");
        let expected_gitdir = worktree_path.join(".git");
        let expected_canonical = expected_gitdir
            .canonicalize()
            .unwrap_or_else(|_| expected_gitdir.clone());
        let mut updated = false;

        let entries = std::fs::read_dir(&worktrees_dir).expect("read worktrees dir");
        for entry in entries {
            let entry = entry.expect("worktree entry");
            let gitdir_path = entry.path().join("gitdir");
            let gitdir_contents = std::fs::read_to_string(&gitdir_path).expect("read gitdir");
            let gitdir_trimmed = gitdir_contents.trim();
            let gitdir_path_buf = PathBuf::from(gitdir_trimmed);
            let gitdir_resolved = if gitdir_path_buf.is_absolute() {
                gitdir_path_buf
            } else {
                repo.join(gitdir_path_buf)
            };
            let gitdir_canonical = gitdir_resolved
                .canonicalize()
                .unwrap_or_else(|_| gitdir_resolved.clone());
            if gitdir_canonical == expected_canonical {
                let base = gitdir_path
                    .parent()
                    .expect("gitdir parent")
                    .canonicalize()
                    .unwrap_or_else(|_| gitdir_path.parent().unwrap().to_path_buf());
                let relative = path_relative_to(&expected_canonical, &base);
                std::fs::write(&gitdir_path, relative.to_string_lossy().as_ref())
                    .expect("write gitdir");
                updated = true;
                break;
            }
        }

        assert!(updated, "worktree gitdir entry not found");
    }

    fn canonical_path(path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    fn path_relative_to(target: &Path, base: &Path) -> PathBuf {
        let target_components: Vec<_> = target.components().collect();
        let base_components: Vec<_> = base.components().collect();
        let mut shared = 0;

        while shared < target_components.len()
            && shared < base_components.len()
            && target_components[shared] == base_components[shared]
        {
            shared += 1;
        }

        let mut relative = PathBuf::new();
        for _ in shared..base_components.len() {
            relative.push("..");
        }
        for component in &target_components[shared..] {
            relative.push(component.as_os_str());
        }

        if relative.as_os_str().is_empty() {
            relative.push(".");
        }

        relative
    }

    #[test]
    fn test_validate_target_branch_worktree_reuses_shared_root() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);

        let shared_root = ensure_shared_worktrees_root(repo).expect("create shared root");
        let worktree_path = shared_root.join("target-branch");
        run_git(
            repo,
            &[
                "worktree",
                "add",
                worktree_path.to_str().expect("worktree path"),
                "target-branch",
            ],
        );

        let result = validate_target_branch_worktree_in(repo, "target-branch")
            .expect("validate target worktree");
        let expected = canonical_path(&worktree_path);
        let actual = result.as_ref().map(|path| canonical_path(path));
        assert_eq!(actual, Some(expected));
    }

    #[test]
    fn test_validate_target_branch_worktree_allows_repo_root() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);

        let current = git_stdout(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        let result =
            validate_target_branch_worktree_in(repo, &current).expect("validate target worktree");
        let expected = canonical_path(repo);
        let actual = result.as_ref().map(|path| canonical_path(path));
        assert_eq!(actual, Some(expected));
    }

    #[test]
    fn test_validate_target_branch_worktree_errors_outside_shared_root() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);

        let outside_dir = TempDir::new().expect("outside dir");
        let outside_path = outside_dir.path().join("target-branch");
        run_git(
            repo,
            &[
                "worktree",
                "add",
                outside_path.to_str().expect("outside path"),
                "target-branch",
            ],
        );

        let err = validate_target_branch_worktree_in(repo, "target-branch")
            .expect_err("should error for outside worktree");
        assert!(
            err.contains("outside shared worktrees root"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_validate_target_branch_worktree_none_when_absent() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);

        let result = validate_target_branch_worktree_in(repo, "target-branch")
            .expect("validate target worktree");
        assert!(result.is_none());
    }

    #[test]
    fn test_sanitize_target_branch_component_encodes_special_chars() {
        let sanitized = sanitize_target_branch_component("release/v1.0@foo");
        assert_eq!(sanitized, "release%2Fv1.0%40foo");
    }

    #[test]
    fn test_create_target_branch_worktree_creates_under_shared_root() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);

        let path = create_target_branch_worktree_in(repo, "target-branch")
            .expect("create target branch worktree");
        let shared_root = ensure_shared_worktrees_root(repo).expect("shared root");

        assert!(path.exists(), "worktree path should exist");
        assert!(path.starts_with(&shared_root));

        let head = git_stdout(&path, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(head, "target-branch");
    }

    #[test]
    fn test_create_target_branch_worktree_reuses_repo_root() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);

        let current = git_stdout(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        let path =
            create_target_branch_worktree_in(repo, &current).expect("reuse repo root worktree");
        assert_eq!(canonical_path(&path), canonical_path(repo));
    }

    #[test]
    fn test_create_target_branch_worktree_sanitizes_path() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "release/v1"]);

        let path = create_target_branch_worktree_in(repo, "release/v1").expect("create worktree");
        let shared_root = ensure_shared_worktrees_root(repo).expect("shared root");
        let expected = shared_root.join("release%2Fv1");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_create_target_branch_worktree_creates_missing_branch_at_head() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);

        let head_rev = rev_parse(repo, "HEAD");
        create_target_branch_worktree_in(repo, "new-target").expect("create worktree");

        let target_rev = rev_parse(repo, "new-target");
        assert_eq!(head_rev, target_rev);
    }

    #[test]
    fn test_create_target_branch_worktree_reconciles_registered_relative_path() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);
        run_git(repo, &["branch", "other"]);

        let shared_root = ensure_shared_worktrees_root(repo).expect("shared root");
        let worktree_path = shared_root.join("target-branch");
        run_git(
            repo,
            &[
                "worktree",
                "add",
                worktree_path.to_str().expect("worktree path"),
                "other",
            ],
        );

        make_worktree_gitdir_relative(repo, &worktree_path);

        let created = create_target_branch_worktree_in(repo, "target-branch")
            .expect("should reconcile mismatched registered path");
        assert_eq!(canonical_path(&created), canonical_path(&worktree_path));
        assert!(worktree_path.exists(), "worktree should exist after reconcile");

        let resolved = find_target_branch_worktree_in(repo, "target-branch")
            .expect("find target worktree")
            .expect("target worktree should exist");
        assert_eq!(canonical_path(&resolved), canonical_path(&worktree_path));

        let head = git_stdout(&worktree_path, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(head, "target-branch");
    }

    #[test]
    fn test_create_target_branch_worktree_preserves_dirty_mismatch() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);
        run_git(repo, &["branch", "other"]);

        let shared_root = ensure_shared_worktrees_root(repo).expect("shared root");
        let worktree_path = shared_root.join("target-branch");
        run_git(
            repo,
            &[
                "worktree",
                "add",
                worktree_path.to_str().expect("worktree path"),
                "other",
            ],
        );

        std::fs::write(worktree_path.join("dirty.txt"), "dirty").expect("write dirty file");

        let err = create_target_branch_worktree_in(repo, "target-branch")
            .expect_err("should not replace active dirty worktree");
        assert!(
            err.contains("uncommitted changes"),
            "unexpected error: {}",
            err
        );
        assert!(worktree_path.exists(), "dirty worktree should be preserved");
        let head = git_stdout(&worktree_path, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(head, "other");
        assert!(
            worktree_path.join("dirty.txt").exists(),
            "dirty file should remain in preserved worktree"
        );
    }

    #[test]
    fn test_create_target_branch_worktree_recovers_missing_stale_registration() {
        let temp = TempDir::new().expect("temp dir");
        let repo = temp.path();
        init_repo(repo);
        run_git(repo, &["branch", "target-branch"]);

        let shared_root = ensure_shared_worktrees_root(repo).expect("shared root");
        let worktree_path = shared_root.join("target-branch");
        run_git(
            repo,
            &[
                "worktree",
                "add",
                worktree_path.to_str().expect("worktree path"),
                "target-branch",
            ],
        );
        std::fs::remove_dir_all(&worktree_path).expect("remove worktree dir");

        let created = create_target_branch_worktree_in(repo, "target-branch")
            .expect("should recreate from stale registration");
        assert_eq!(canonical_path(&created), canonical_path(&worktree_path));
        assert!(created.exists(), "recreated worktree should exist");
        let head = git_stdout(&created, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(head, "target-branch");
    }
}
