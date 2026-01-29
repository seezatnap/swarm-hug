# Specifications: agent-hashes

# PRD: Project-Namespaced Agent Worktrees with Run Hash

## Summary
Namespace agent worktrees by project name **and run hash** to allow parallel sprints across different projects and safe restarts. Currently agent worktrees are named `agent-aaron`, which conflicts when multiple projects run concurrently or when a sprint is cancelled and restarted. Change to `{project}-agent-aaron-{hash}` format, where the hash is unique per sprint run.

## Problem Statement

With the sprint branch isolation work, multiple projects can now run sprints in parallel. However, agent worktrees use a global naming scheme:

```
worktrees/
├── agent-aaron      ← Used by which project?
├── agent-betty      ← Conflict if greenfield and payments both need Betty
└── agent-charlie
```

If project "greenfield" and project "payments" both try to use agent Aaron, they'll conflict on the same worktree path and branch name.

Additionally, if a sprint is cancelled mid-run and restarted, stale worktrees and branches from the previous run can cause conflicts or data corruption. A run-unique hash ensures each sprint execution is isolated.

## Goals
- Sprint worktrees are namespaced with a run hash: `{project}-sprint-{n}-{hash}`
- Agent worktrees are namespaced with the same run hash: `{project}-agent-{name}-{hash}`
- Agent branches are namespaced with the same run hash: `{project}-agent-{name}-{hash}`
- The hash is generated once per sprint run and shared by all artifacts of that run
- Cancelling and restarting a sprint creates a new hash (new isolated run)
- Multiple projects can run concurrently without worktree/branch conflicts
- Cleanup operations target artifacts by their run hash, ensuring precise cleanup

## Non-goals
- Changes to feature branch naming (remains `{project}-feature-{name}`)
- Persisting the hash across process restarts (each run is intentionally isolated)

## Obsoletes: assignments.toml

With project-namespaced worktrees, the `.swarm-hug/assignments.toml` file is no longer needed. Previously it tracked which agents were assigned to which projects to prevent worktree conflicts:

```toml
# .swarm-hug/assignments.toml
[agents]
A = "greenfield"
B = "greenfield"
C = "payments"
```

This was necessary because `agent-aaron` could only exist once. With `greenfield-agent-aaron` and `payments-agent-aaron` being separate, agents can work on multiple projects simultaneously without conflict.

**Remove:**
- `src/team/assignments.rs` - entire module
- `src/team/mod.rs` - `assignments` module export and `ASSIGNMENTS_FILE` constant
- `src/runner.rs` - all `release_assignments_for_project()` calls and assignment checking
- `src/git.rs` - commits that include `assignments.toml`
- `src/commands/init.rs` - assignments.toml initialization
- `src/commands/projects.rs` - any assignment display logic
- `.swarm-hug/assignments.toml` - the file itself (via migration/cleanup)

## Current Implementation

**Worktree creation:** `src/worktree/create.rs`
```rust
// Line ~40-50 in create_worktrees_in()
let branch = format!("agent-{}", name.to_lowercase());
let path = worktrees_dir.join(&branch);
```

**Worktree cleanup:** `src/worktree/cleanup.rs`
```rust
// Cleans up worktrees matching "agent-{initial}" pattern
```

**Branch naming:** `src/worktree/mod.rs`
```rust
pub fn agent_branch_name(initial: char) -> String {
    let name = agent::name_from_initial(initial).unwrap_or("unknown");
    format!("agent-{}", name.to_lowercase())
}
```

## Proposed Solution

### 1. Add run hash generation

The run hash is a short (6 character), URL-safe identifier generated once at the start of each sprint run. It's derived from random bytes to ensure uniqueness.

```rust
// src/run_hash.rs (NEW FILE)
use rand::Rng;

/// Generates a 6-character alphanumeric hash unique to this run.
/// Uses lowercase letters and digits for git branch name compatibility.
pub fn generate_run_hash() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    const HASH_LEN: usize = 6;

    let mut rng = rand::thread_rng();
    (0..HASH_LEN)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_length() {
        let hash = generate_run_hash();
        assert_eq!(hash.len(), 6);
    }

    #[test]
    fn test_hash_uniqueness() {
        let hash1 = generate_run_hash();
        let hash2 = generate_run_hash();
        assert_ne!(hash1, hash2); // Statistically should never collide
    }

    #[test]
    fn test_hash_is_alphanumeric() {
        let hash = generate_run_hash();
        assert!(hash.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }
}
```

### 2. Introduce `RunContext` to hold run-scoped identifiers

```rust
// src/run_context.rs (NEW FILE)
use crate::run_hash::generate_run_hash;

/// Context for a single sprint run. Created once at run start,
/// passed to all functions that need to create namespaced artifacts.
pub struct RunContext {
    pub project: String,
    pub sprint_number: u32,
    pub run_hash: String,
}

impl RunContext {
    pub fn new(project: &str, sprint_number: u32) -> Self {
        Self {
            project: project.to_string(),
            sprint_number,
            run_hash: generate_run_hash(),
        }
    }

    /// Sprint branch name: {project}-sprint-{n}-{hash}
    pub fn sprint_branch(&self) -> String {
        format!("{}-sprint-{}-{}", self.project, self.sprint_number, self.run_hash)
    }

    /// Agent branch name: {project}-agent-{name}-{hash}
    pub fn agent_branch(&self, initial: char) -> String {
        let name = crate::agent::name_from_initial(initial).unwrap_or("unknown");
        format!("{}-agent-{}-{}", self.project, name.to_lowercase(), self.run_hash)
    }

    /// Returns the hash suffix for display/logging
    pub fn hash(&self) -> &str {
        &self.run_hash
    }
}
```

### 3. Update `agent_branch_name()` to use RunContext

```rust
// src/worktree/mod.rs
pub fn agent_branch_name(ctx: &RunContext, initial: char) -> String {
    ctx.agent_branch(initial)
}

// Or simply use ctx.agent_branch() directly and deprecate this function
```

### 4. Update worktree creation to use RunContext

```rust
// src/worktree/create.rs
pub fn create_worktrees_in(
    worktrees_dir: &Path,
    assignments: &[(char, String)],
    base_branch: &str,
    ctx: &RunContext,  // NEW parameter (replaces project)
) -> Result<Vec<Worktree>, String> {
    // ...
    let branch = ctx.agent_branch(initial);
    let path = worktrees_dir.join(&branch);
    // ...
}
```

### 5. Update cleanup to use RunContext

```rust
// src/worktree/cleanup.rs
pub fn cleanup_agent_worktrees(
    worktrees_dir: &Path,
    initials: &[char],
    delete_branches: bool,
    ctx: &RunContext,  // NEW parameter
) -> CleanupSummary {
    for initial in initials {
        let branch = ctx.agent_branch(initial);
        // cleanup logic using namespaced branch
    }
}
```

### 6. Update runner to create and use RunContext

```rust
// src/runner.rs
use crate::run_context::RunContext;

pub async fn run_sprint(/* ... */) -> Result<()> {
    // Generate run context at the very start
    let ctx = RunContext::new(&team_name, sprint_number);

    println!("Starting sprint {} (run {})", sprint_number, ctx.hash());

    // Create sprint worktree with hash
    let sprint_branch = ctx.sprint_branch();

    let worktrees = worktree::create_worktrees_in(
        worktrees_dir,
        &assignments,
        &sprint_branch,
        &ctx,
    )?;

    // Later for cleanup - uses same ctx so targets exact same branches
    worktree::cleanup_agent_worktrees(
        worktrees_dir,
        &assigned_initials,
        true,
        &ctx,
    );
}
```

## Directory Structure After Change

```
worktrees/
├── greenfield-sprint-1-a3f8k2/    # Sprint worktree with run hash
├── greenfield-agent-aaron-a3f8k2/ # Agent worktree (same hash as sprint)
├── greenfield-agent-betty-a3f8k2/
├── payments-sprint-1-x9m2p7/      # Different project, different hash
├── payments-agent-aaron-x9m2p7/   # Same agent name, different hash - no conflict!
└── payments-agent-betty-x9m2p7/
```

If `greenfield-sprint-1` is cancelled and restarted, the new run gets a fresh hash:

```
worktrees/
├── greenfield-sprint-1-j4n9q1/    # NEW hash after restart
├── greenfield-agent-aaron-j4n9q1/ # Agents also get the new hash
├── greenfield-agent-betty-j4n9q1/
```

The old `a3f8k2` artifacts may still exist but won't conflict. Cleanup at the end of the new run only removes `j4n9q1` artifacts.

## Affected Files

### Create

| File | Purpose |
|------|---------|
| `src/run_hash.rs` | Run hash generation (6-char alphanumeric) |
| `src/run_context.rs` | `RunContext` struct holding project, sprint number, and run hash |

### Modify

| File | Change |
|------|--------|
| `src/lib.rs` | Add `run_hash` and `run_context` module exports, remove assignment-related exports |
| `src/worktree/mod.rs` | Update `agent_branch_name()` to accept `RunContext` |
| `src/worktree/create.rs` | Accept `RunContext` param in `create_worktrees_in()` |
| `src/worktree/cleanup.rs` | Accept `RunContext` param in cleanup functions |
| `src/runner.rs` | Create `RunContext` at run start, pass to all worktree functions, remove assignment logic |
| `src/git.rs` | Remove `assignments.toml` from commit file lists |
| `src/team/mod.rs` | Remove `assignments` module export and `ASSIGNMENTS_FILE` constant |
| `src/commands/projects.rs` | Remove assignment display logic (if any) |
| `Cargo.toml` | Add `rand` crate dependency for hash generation |

### Delete

| File | Reason |
|------|--------|
| `src/team/assignments.rs` | Entire module obsolete |
| `.swarm-hug/assignments.toml` | No longer needed (runtime deletion) |

### Update Tests

| File | Change |
|------|--------|
| `src/team/assignments/tests.rs` | Delete (module removed) |
| `src/worktree/create/tests.rs` | Update to use `RunContext` |
| `src/worktree/cleanup/tests.rs` | Update to use `RunContext` |
| `tests/integration.rs` | Remove assignment-related test assertions |

## Testing Plan

### Unit Tests: Run Hash Generation
```rust
#[test]
fn test_hash_length() {
    let hash = generate_run_hash();
    assert_eq!(hash.len(), 6);
}

#[test]
fn test_hash_uniqueness() {
    let hash1 = generate_run_hash();
    let hash2 = generate_run_hash();
    assert_ne!(hash1, hash2);
}

#[test]
fn test_hash_is_git_branch_safe() {
    let hash = generate_run_hash();
    assert!(hash.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
}
```

### Unit Tests: RunContext
```rust
#[test]
fn test_sprint_branch_includes_hash() {
    let ctx = RunContext::new("greenfield", 1);
    let branch = ctx.sprint_branch();
    assert!(branch.starts_with("greenfield-sprint-1-"));
    assert_eq!(branch.len(), "greenfield-sprint-1-".len() + 6);
}

#[test]
fn test_agent_branch_includes_same_hash() {
    let ctx = RunContext::new("greenfield", 1);
    let sprint = ctx.sprint_branch();
    let agent = ctx.agent_branch('A');

    // Extract hash from both
    let sprint_hash = sprint.split('-').last().unwrap();
    let agent_hash = agent.split('-').last().unwrap();
    assert_eq!(sprint_hash, agent_hash);
}

#[test]
fn test_different_runs_different_hashes() {
    let ctx1 = RunContext::new("greenfield", 1);
    let ctx2 = RunContext::new("greenfield", 1);
    assert_ne!(ctx1.sprint_branch(), ctx2.sprint_branch());
}
```

### Unit Tests: Agent Branch Naming
```rust
#[test]
fn test_agent_branch_name_format() {
    let ctx = RunContext::new("greenfield", 1);
    let branch = ctx.agent_branch('A');
    assert!(branch.starts_with("greenfield-agent-aaron-"));
}

#[test]
fn test_different_projects_different_branches() {
    let ctx1 = RunContext::new("greenfield", 1);
    let ctx2 = RunContext::new("payments", 1);
    // Different projects always have different hashes (and different prefixes)
    assert_ne!(ctx1.agent_branch('A'), ctx2.agent_branch('A'));
}
```

### Integration Tests
```rust
#[test]
fn test_parallel_projects_no_worktree_conflict() {
    // Start greenfield sprint with agent A
    // Start payments sprint with agent A
    // Both should succeed without conflict (different hashes)
    // Cleanup greenfield should not affect payments
}

#[test]
fn test_restart_creates_new_hash() {
    // Start greenfield sprint, note the hash
    // Cancel/abort the sprint
    // Start greenfield sprint again
    // Verify new hash is different
    // Verify old worktrees still exist (not cleaned up)
}

#[test]
fn test_cleanup_only_affects_current_run() {
    // Create worktrees with hash "abc123"
    // Create worktrees with hash "xyz789"
    // Cleanup with RunContext for "abc123"
    // Verify "xyz789" worktrees still exist
}
```

### Manual Tests
1. Run two projects in parallel: `swarm -p greenfield run` and `swarm -p payments run`
2. Verify both create separate agent worktrees with different hashes
3. Verify cleanup of one project doesn't affect the other
4. Cancel a sprint mid-run, restart it, verify new hash in output
5. Verify old worktrees from cancelled run can be cleaned up manually or via `git worktree prune`

## Acceptance Criteria
- [ ] Run hash is generated once per sprint run (6 alphanumeric characters)
- [ ] Sprint worktrees named `{project}-sprint-{n}-{hash}`
- [ ] Agent worktrees named `{project}-agent-{name}-{hash}`
- [ ] Agent branches named `{project}-agent-{name}-{hash}`
- [ ] All artifacts from a single run share the same hash
- [ ] Cancelling and restarting a sprint produces a different hash
- [ ] Two projects can run concurrently with overlapping agent assignments
- [ ] Cleanup only removes current run's worktrees (matched by hash)
- [ ] `assignments.toml` is no longer created or used
- [ ] `src/team/assignments.rs` is deleted
- [ ] `src/run_hash.rs` and `src/run_context.rs` created
- [ ] All existing tests pass (updated for new signatures, assignment tests removed)
- [ ] `cargo clippy` reports no new warnings

## Implementation Plan

### Phase 1: Add run hash infrastructure
1. Add `rand` crate to `Cargo.toml`
2. Create `src/run_hash.rs` with `generate_run_hash()` function
3. Create `src/run_context.rs` with `RunContext` struct
4. Add module exports in `src/lib.rs`
5. Write unit tests for hash generation and RunContext

### Phase 2: Update worktree functions to use RunContext
6. Update `agent_branch_name()` to accept `RunContext` (or deprecate in favor of `ctx.agent_branch()`)
7. Update `create_worktrees_in()` signature to accept `RunContext`
8. Update cleanup functions to accept `RunContext`
9. Update sprint branch creation to use `ctx.sprint_branch()`

### Phase 3: Integrate RunContext in runner
10. Create `RunContext` at the start of `run_sprint()`
11. Pass `RunContext` to all worktree creation and cleanup calls
12. Log the run hash at sprint start for visibility

### Phase 4: Remove assignments.toml
13. Delete `src/team/assignments.rs`
14. Remove `ASSIGNMENTS_FILE` constant and module export from `src/team/mod.rs`
15. Remove assignment logic from `src/runner.rs` (release_assignments_for_project, etc.)
16. Remove assignments.toml from git commit file lists in `src/git.rs`
17. Remove any assignment-related code from `src/commands/`

### Phase 5: Tests and cleanup
18. Delete assignment-related tests
19. Update worktree tests to use `RunContext`
20. Update integration tests
21. Manual verification: parallel projects, restart scenarios

## Risks
- **Migration**: Existing agent worktrees won't be found after upgrade. First run may need manual cleanup or auto-migration.
- **Branch pollution**: Old `agent-*` branches may linger. Could add migration to clean them up.
- **Orphaned artifacts from cancelled runs**: If a sprint is cancelled before cleanup runs, the hashed worktrees/branches remain. This is intentional (allows investigation), but may accumulate. Consider periodic garbage collection or a `swarm cleanup --stale` command.
- **Hash collisions**: With 36^6 ≈ 2.2 billion possibilities, collisions are extremely unlikely. No mitigation needed.
- **Debugging complexity**: Run hashes make branch names longer and less memorable. Mitigated by logging the hash prominently at run start.

## Migration Strategy

On first run after upgrade:
1. Detect any old-style `agent-*` worktrees/branches (without hash suffix) and remove them
2. Delete `.swarm-hug/assignments.toml` if it exists
3. Create new project-namespaced, hash-suffixed worktrees as needed

Or simply document that users should:
```bash
git worktree prune
git branch -D agent-aaron agent-betty agent-charlie ...  # old agent branches
rm .swarm-hug/assignments.toml
```

Since agent worktrees and assignments are ephemeral (recreated each sprint), no data is lost.

### Cleaning up orphaned hashed artifacts

For runs that were cancelled without cleanup:
```bash
# List orphaned worktrees
git worktree list | grep -E '\-[a-z0-9]{6}$'

# Remove all orphaned worktrees
git worktree prune

# Remove orphaned branches (be careful - review before running)
git branch | grep -E '\-[a-z0-9]{6}$' | xargs git branch -D
```

A future enhancement could add `swarm cleanup --orphaned` to automate this.

