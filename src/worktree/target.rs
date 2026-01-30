use std::fs;
use std::path::{Path, PathBuf};

/// Returns the shared worktrees root for target branch operations.
///
/// Path: `./swarm-hub/.shared/worktrees` (relative to the repo root).
pub fn shared_worktrees_root(repo_root: &Path) -> PathBuf {
    repo_root.join("swarm-hub").join(".shared").join("worktrees")
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_shared_worktrees_root_path() {
        let temp = TempDir::new().expect("temp dir");
        let root = shared_worktrees_root(temp.path());
        let expected = temp
            .path()
            .join("swarm-hub")
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
}
