# PRD: Project-Namespaced Agent Worktrees

## Summary
Namespace agent worktrees by project name to allow parallel sprints across different projects. Currently agent worktrees are named `agent-aaron`, which conflicts when multiple projects run concurrently. Change to `{project}-agent-aaron` format.

## Problem Statement

With the sprint branch isolation work, multiple projects can now run sprints in parallel. However, agent worktrees use a global naming scheme:

```
worktrees/
├── agent-aaron      ← Used by which project?
├── agent-betty      ← Conflict if greenfield and payments both need Betty
└── agent-charlie
```

If project "greenfield" and project "payments" both try to use agent Aaron, they'll conflict on the same worktree path and branch name.

## Goals
- Agent worktrees are namespaced by project: `{project}-agent-{name}`
- Agent branches are namespaced by project: `{project}-agent-{initial}`
- Multiple projects can run concurrently without worktree/branch conflicts
- Cleanup operations only affect the current project's worktrees

## Non-goals
- Changes to feature/sprint branch naming (already namespaced)

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

### 1. Update `agent_branch_name()` to accept project name

```rust
// src/worktree/mod.rs
pub fn agent_branch_name(project: &str, initial: char) -> String {
    let name = agent::name_from_initial(initial).unwrap_or("unknown");
    format!("{}-agent-{}", project, name.to_lowercase())
}
```

### 2. Update worktree creation to use project-namespaced paths

```rust
// src/worktree/create.rs
pub fn create_worktrees_in(
    worktrees_dir: &Path,
    assignments: &[(char, String)],
    base_branch: &str,
    project: &str,  // NEW parameter
) -> Result<Vec<Worktree>, String> {
    // ...
    let branch = agent_branch_name(project, initial);
    let path = worktrees_dir.join(&branch);
    // ...
}
```

### 3. Update cleanup to use project-namespaced pattern

```rust
// src/worktree/cleanup.rs
pub fn cleanup_agent_worktrees(
    worktrees_dir: &Path,
    initials: &[char],
    delete_branches: bool,
    project: &str,  // NEW parameter
) -> CleanupSummary {
    for initial in initials {
        let branch = agent_branch_name(project, initial);
        // cleanup logic using namespaced branch
    }
}
```

### 4. Update runner to pass project name

```rust
// src/runner.rs
let worktrees = worktree::create_worktrees_in(
    worktrees_dir,
    &assignments,
    &sprint_branch,
    &team_name,  // Pass project/team name
)?;

// Later for cleanup
worktree::cleanup_agent_worktrees(
    worktrees_dir,
    &assigned_initials,
    true,
    &team_name,
);
```

## Directory Structure After Change

```
worktrees/
├── greenfield-sprint-1/           # Sprint worktree (already namespaced)
├── greenfield-agent-aaron/        # Agent worktree (NEW naming)
├── greenfield-agent-betty/
├── payments-sprint-1/
├── payments-agent-aaron/          # Same agent, different project - no conflict!
└── payments-agent-betty/
```

## Affected Files

### Modify

| File | Change |
|------|--------|
| `src/worktree/mod.rs` | Update `agent_branch_name()` signature to include project |
| `src/worktree/create.rs` | Add project param to `create_worktrees_in()` |
| `src/worktree/cleanup.rs` | Add project param to cleanup functions |
| `src/runner.rs` | Pass project name to worktree functions, remove assignment logic |
| `src/git.rs` | Remove `assignments.toml` from commit file lists |
| `src/team/mod.rs` | Remove `assignments` module export and `ASSIGNMENTS_FILE` constant |
| `src/commands/projects.rs` | Remove assignment display logic (if any) |
| `src/lib.rs` | Remove any assignment-related exports |

### Delete

| File | Reason |
|------|--------|
| `src/team/assignments.rs` | Entire module obsolete |
| `.swarm-hug/assignments.toml` | No longer needed (runtime deletion) |

### Update Tests

| File | Change |
|------|--------|
| `src/team/assignments/tests.rs` | Delete (module removed) |
| `src/worktree/create/tests.rs` | Update to pass project name |
| `src/worktree/cleanup/tests.rs` | Update to pass project name |
| `tests/integration.rs` | Remove assignment-related test assertions |

## Testing Plan

### Unit Tests
```rust
#[test]
fn test_agent_branch_name_includes_project() {
    assert_eq!(
        agent_branch_name("greenfield", 'A'),
        "greenfield-agent-aaron"
    );
}

#[test]
fn test_different_projects_different_branches() {
    assert_ne!(
        agent_branch_name("greenfield", 'A'),
        agent_branch_name("payments", 'A')
    );
}
```

### Integration Tests
```rust
#[test]
fn test_parallel_projects_no_worktree_conflict() {
    // Start greenfield sprint with agent A
    // Start payments sprint with agent A
    // Both should succeed without conflict
    // Cleanup greenfield should not affect payments
}
```

### Manual Tests
1. Run two projects in parallel: `swarm -p greenfield run` and `swarm -p payments run`
2. Verify both create separate agent worktrees
3. Verify cleanup of one project doesn't affect the other

## Acceptance Criteria
- [ ] Agent worktrees named `{project}-agent-{name}`
- [ ] Agent branches named `{project}-agent-{name}`
- [ ] Two projects can run concurrently with overlapping agent assignments
- [ ] Cleanup only removes current project's agent worktrees
- [ ] `assignments.toml` is no longer created or used
- [ ] `src/team/assignments.rs` is deleted
- [ ] All existing tests pass (updated for new signatures, assignment tests removed)
- [ ] `cargo clippy` reports no new warnings

## Implementation Plan

### Phase 1: Add project namespacing
1. Update `agent_branch_name()` to accept project parameter
2. Update `create_worktrees_in()` signature and implementation
3. Update cleanup functions to use project-namespaced names
4. Update runner.rs to pass project name

### Phase 2: Remove assignments.toml
5. Delete `src/team/assignments.rs`
6. Remove `ASSIGNMENTS_FILE` constant and module export from `src/team/mod.rs`
7. Remove assignment logic from `src/runner.rs` (release_assignments_for_project, etc.)
8. Remove assignments.toml from git commit file lists in `src/git.rs`
9. Remove any assignment-related code from `src/commands/`

### Phase 3: Tests and cleanup
10. Delete assignment-related tests
11. Update worktree tests to pass project name
12. Update integration tests
13. Manual verification with parallel projects

## Risks
- **Migration**: Existing agent worktrees won't be found after upgrade. First run may need manual cleanup or auto-migration.
- **Branch pollution**: Old `agent-*` branches may linger. Could add migration to clean them up.

## Migration Strategy

On first run after upgrade:
1. Detect any old-style `agent-*` worktrees/branches and remove them
2. Delete `.swarm-hug/assignments.toml` if it exists
3. Create new project-namespaced worktrees as needed

Or simply document that users should:
```bash
git worktree prune
git branch -D agent-aaron agent-betty agent-charlie ...  # old agent branches
rm .swarm-hug/assignments.toml
```

Since agent worktrees and assignments are ephemeral (recreated each sprint), no data is lost.
