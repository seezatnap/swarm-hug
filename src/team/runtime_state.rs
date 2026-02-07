use std::path::{Path, PathBuf};

use super::{SPRINT_HISTORY_FILE, SWARM_HUG_DIR, TEAM_STATE_FILE};

/// Runtime state paths for a swarm run.
///
/// For split source/target branch runs, state is namespaced under:
/// `.swarm-hug/<team>/runs/<sanitized-target-branch>/`
///
/// For single-variation runs (source == target), legacy paths are preserved:
/// `.swarm-hug/<team>/`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStatePaths {
    team_name: String,
    root: PathBuf,
    namespaced: bool,
}

impl RuntimeStatePaths {
    /// Build runtime paths for the given team and branch configuration.
    pub fn for_branches(team_name: &str, source_branch: &str, target_branch: &str) -> Self {
        let base = PathBuf::from(SWARM_HUG_DIR).join(team_name);
        let source = source_branch.trim();
        let target = target_branch.trim();

        let namespaced = !source.is_empty() && !target.is_empty() && source != target;
        let root = if namespaced {
            base.join("runs").join(sanitize_target_branch_component(target))
        } else {
            base
        };

        Self {
            team_name: team_name.to_string(),
            root,
            namespaced,
        }
    }

    /// Root runtime state directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether this run uses the split-branch runtime namespace.
    pub fn is_namespaced(&self) -> bool {
        self.namespaced
    }

    /// Runtime tasks path used for sprint planning and assignment state.
    pub fn tasks_path(&self) -> PathBuf {
        self.root.join("tasks.md")
    }

    /// Runtime sprint history path.
    pub fn sprint_history_path(&self) -> PathBuf {
        self.root.join(SPRINT_HISTORY_FILE)
    }

    /// Runtime team state path.
    pub fn team_state_path(&self) -> PathBuf {
        self.root.join(TEAM_STATE_FILE)
    }

    /// Canonical team root in branch state (`.swarm-hug/<team>`).
    pub fn branch_root(&self) -> PathBuf {
        PathBuf::from(SWARM_HUG_DIR).join(&self.team_name)
    }

    /// Canonical tasks path in branch state.
    pub fn branch_tasks_path(&self) -> PathBuf {
        self.branch_root().join("tasks.md")
    }

    /// Canonical sprint history path in branch state.
    pub fn branch_sprint_history_path(&self) -> PathBuf {
        self.branch_root().join(SPRINT_HISTORY_FILE)
    }

    /// Canonical team state path in branch state.
    pub fn branch_team_state_path(&self) -> PathBuf {
        self.branch_root().join(TEAM_STATE_FILE)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_variation_uses_legacy_paths() {
        let paths = RuntimeStatePaths::for_branches("alpha", "main", "main");
        assert!(!paths.is_namespaced());
        assert_eq!(paths.root(), Path::new(".swarm-hug/alpha"));
        assert_eq!(paths.tasks_path(), PathBuf::from(".swarm-hug/alpha/tasks.md"));
        assert_eq!(
            paths.sprint_history_path(),
            PathBuf::from(".swarm-hug/alpha/sprint-history.json")
        );
        assert_eq!(
            paths.team_state_path(),
            PathBuf::from(".swarm-hug/alpha/team-state.json")
        );
    }

    #[test]
    fn split_variation_uses_target_branch_namespace() {
        let paths = RuntimeStatePaths::for_branches("alpha", "main", "feature/try-1");
        assert!(paths.is_namespaced());
        assert_eq!(
            paths.root(),
            Path::new(".swarm-hug/alpha/runs/feature%2Ftry-1")
        );
        assert_eq!(
            paths.tasks_path(),
            PathBuf::from(".swarm-hug/alpha/runs/feature%2Ftry-1/tasks.md")
        );
    }

    #[test]
    fn split_variation_encodes_special_branch_characters() {
        let paths =
            RuntimeStatePaths::for_branches("alpha", "main", "release/v1.0@staging");
        assert_eq!(
            paths.root(),
            Path::new(".swarm-hug/alpha/runs/release%2Fv1.0%40staging")
        );
    }

    #[test]
    fn branch_paths_remain_canonical() {
        let paths = RuntimeStatePaths::for_branches("beta", "dev", "feature/x");
        assert_eq!(paths.branch_root(), PathBuf::from(".swarm-hug/beta"));
        assert_eq!(
            paths.branch_sprint_history_path(),
            PathBuf::from(".swarm-hug/beta/sprint-history.json")
        );
        assert_eq!(
            paths.branch_team_state_path(),
            PathBuf::from(".swarm-hug/beta/team-state.json")
        );
    }
}
