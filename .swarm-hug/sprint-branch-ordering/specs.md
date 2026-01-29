# Specifications: sprint-branch-ordering-v4

# PRD: Sprint Branch Creation Before Setup Files

## Summary
Fix the ordering of sprint initialization so the sprint branch is created BEFORE any sprint setup files are written. Currently, `sprint-history.json`, `team-state.json`, and task assignments in `tasks.md` are written to the target branch (main/master) before the sprint branch exists, polluting the target branch with sprint-specific state.

## Problem Statement

When starting a sprint, the following sequence occurs in `src/runner.rs`:

```
CURRENT (BROKEN) SEQUENCE:
1. Load sprint_history, increment counter
2. WRITE sprint-history.json           ← main branch polluted
3. Load team_state, set feature_branch
4. WRITE team-state.json               ← main branch polluted
5. Assign tasks to agents ([A], [B], [C])
6. WRITE tasks.md with assignments     ← main branch polluted
7. CREATE sprint branch                ← too late
8. Commit changes to sprint branch     ← re-commits already-dirty files
```

**Result:**
- Target branch (main/master) has uncommitted changes after swarm runs
- Sprint branch's first commit contains diffs that already exist (uncommitted) on main
- If user runs `git status` on main after swarm, they see dirty working tree
- These files are not meant to be in main - they're sprint-specific state

**Evidence from user:**
```
georgepantazis@mac kyber-v2 (master) % git status
Changes not staged for commit:
    modified:   .swarm-hug/greenfield/tasks.md

Untracked files:
    .swarm-hug/greenfield/sprint-history.json
    .swarm-hug/greenfield/team-state.json
```

## Goals
- Sprint branch is created BEFORE any sprint files are written
- All sprint setup files are written directly to the sprint branch worktree
- Target branch (main/master) remains clean after sprint initialization
- No changes leak from sprint branch back to target branch

## Non-goals
- Changes to task assignment logic
- Changes to agent worktree management
- Changes to sprint postmortem or merge-back flow

## Affected Files

| File | Line | Current Operation | Issue |
|------|------|-------------------|-------|
| `src/runner.rs` | 145 | `sprint_history.save()` | Written before branch exists |
| `src/runner.rs` | 174 | `team_state.save()` | Written before branch exists |
| `src/runner.rs` | 179-180 | `fs::write(&config.files_tasks, ...)` | Written before branch exists |
| `src/runner.rs` | 166 | `create_feature_worktree_in()` | Happens too late |

## Proposed Solution

Reorder operations so branch creation happens FIRST:

```
CORRECTED SEQUENCE:
1. Determine sprint number (peek, don't write)
2. CREATE sprint branch/worktree FIRST
3. Load sprint_history, increment counter
4. WRITE sprint-history.json to sprint worktree
5. Load team_state, set feature_branch
6. WRITE team-state.json to sprint worktree
7. Assign tasks to agents ([A], [B], [C])
8. WRITE tasks.md to sprint worktree
9. Commit changes to sprint branch
```

### Implementation Details

**1. Split sprint number determination from persistence**

Currently `sprint_history.next_sprint()` increments AND the save writes. We need to peek the next number without writing:

```rust
// In src/team/sprint_history.rs
pub fn peek_next_sprint(&self) -> usize {
    self.total_sprints + 1
}

pub fn increment(&mut self) {
    self.total_sprints += 1;
}
```

**2. Create worktree before any file writes**

```rust
// In src/runner.rs run_sprint()

// 1. Determine sprint number without writing
let sprint_history = SprintHistory::load(&history_path)?;
let sprint_number = sprint_history.peek_next_sprint();
let sprint_branch = format!("{}-sprint-{}", team_name, sprint_number);

// 2. Create sprint branch/worktree FIRST
let feature_worktree_path = worktree::create_feature_worktree_in(
    worktrees_dir,
    &sprint_branch,
    target_branch
)?;

// 3. NOW write files to the sprint worktree
let worktree_swarm_dir = feature_worktree_path.join(".swarm-hug").join(&team_name);

// Update and save sprint history to worktree
let mut sprint_history = SprintHistory::load_from(&worktree_swarm_dir.join("sprint-history.json"))?;
sprint_history.increment();
sprint_history.save()?;

// Update and save team state to worktree
let mut team_state = TeamState::load_from(&worktree_swarm_dir.join("team-state.json"))?;
team_state.set_feature_branch(&sprint_branch);
team_state.save()?;

// Write tasks to worktree
let worktree_tasks_path = worktree_swarm_dir.join("tasks.md");
fs::write(&worktree_tasks_path, task_list.to_string())?;

// 4. Commit to sprint branch
commit_task_assignments(&feature_worktree_path, &sprint_branch, sprint_number)?;
```

**3. Update file path helpers to support worktree context**

The `SprintHistory` and `TeamState` structs need methods that accept explicit paths:

```rust
// In src/team/sprint_history.rs
impl SprintHistory {
    pub fn load_from(path: &Path) -> Result<Self, String> {
        // Load from explicit path instead of deriving from team name
    }
}

// In src/team/state.rs
impl TeamState {
    pub fn load_from(path: &Path) -> Result<Self, String> {
        // Load from explicit path instead of deriving from team name
    }
}
```

## Edge Cases

### First sprint (files don't exist yet)
- `sprint-history.json` and `team-state.json` may not exist on first sprint
- Solution: Create them in the sprint worktree if they don't exist
- They should NOT be created in the target branch

### Resuming an interrupted sprint
- If swarm restarts mid-sprint, files may already exist in sprint branch
- Current resume logic should work - it reads from the existing sprint worktree

### Target branch has existing .swarm-hug files
- Task definitions (`tasks.md` initial content) should exist in target branch
- Only sprint-specific state (assignments, history, team-state) goes to sprint branch

## Testing Plan

### Manual Tests
1. Start fresh repo with no `.swarm-hug/*/sprint-history.json` or `team-state.json`
2. Run `swarm --project greenfield run`
3. After sprint starts, check target branch: `git status` should show clean
4. Check sprint branch has the setup files committed

### Automated Tests
```rust
#[test]
fn test_sprint_init_keeps_target_branch_clean() {
    // Setup: create repo with tasks.md
    // Run: start a sprint
    // Assert: main branch has no uncommitted changes
    // Assert: sprint branch has sprint-history.json, team-state.json, updated tasks.md
}

#[test]
fn test_first_sprint_creates_files_in_sprint_branch() {
    // Setup: repo with no sprint-history.json or team-state.json
    // Run: start first sprint
    // Assert: files created only in sprint branch, not main
}
```

## Acceptance Criteria
- [ ] After sprint initialization, `git status` on target branch shows clean working tree
- [ ] `sprint-history.json` exists only in sprint branch (not target branch)
- [ ] `team-state.json` exists only in sprint branch (not target branch)
- [ ] Task assignments (`[A]`, `[B]`, `[C]`) appear only in sprint branch's `tasks.md`
- [ ] First sprint works when state files don't exist
- [ ] Sprint resume works when state files exist in sprint branch
- [ ] `cargo test` passes
- [ ] `cargo clippy` reports no new warnings

## Risks
- **File path assumptions**: Code may assume `.swarm-hug/` paths are always in the main repo root. Need to audit all file path construction.
- **Config object mutations**: `config.files_tasks` points to main repo. May need sprint-worktree-aware config.
- **Race conditions**: Ensure worktree is fully created before writing files to it.

## Implementation Plan

1. Add `peek_next_sprint()` method to `SprintHistory`
2. Add `load_from(path)` methods to `SprintHistory` and `TeamState`
3. Reorder `run_sprint()` to create worktree first
4. Update file write paths to use sprint worktree
5. Add tests for clean target branch
6. Manual verification with real sprint run

