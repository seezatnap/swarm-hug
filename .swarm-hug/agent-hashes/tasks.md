# Tasks

## Infrastructure

- [x] (#1) Create `src/run_hash.rs` module with `generate_run_hash()` function (6-char alphanumeric, git-branch-safe) including unit tests for length, uniqueness, and character validity; create `src/run_context.rs` with `RunContext` struct containing project, sprint_number, and run_hash fields plus `new()`, `sprint_branch()`, `agent_branch()`, and `hash()` methods with full unit test coverage; add both module exports to `src/lib.rs` [5 pts] (A)

## Worktree Module Updates

- [x] (#2) Update `src/worktree/mod.rs` to modify `agent_branch_name()` to accept `RunContext` parameter and use `ctx.agent_branch()` for namespaced branch names; update `src/worktree/create.rs` to accept `RunContext` parameter in `create_worktrees_in()` replacing the project string, generate branch names using `ctx.agent_branch(initial)` and paths using the namespaced branch name [5 pts] (blocked by #1) (A)
- [x] (#3) Update `src/worktree/cleanup.rs` cleanup functions to accept `RunContext` parameter, use `ctx.agent_branch()` for matching worktrees/branches during cleanup, ensuring cleanup only affects current run's artifacts (matched by hash); update related worktree tests in `src/worktree/` to use `RunContext` [5 pts] (blocked by #1) (A)

## Runner Integration

- [x] (#4) Update `src/runner.rs` to create `RunContext` at the start of `run_sprint()`, pass it to all worktree creation and cleanup calls, update sprint branch creation to use `ctx.sprint_branch()`, log the run hash at sprint start for visibility, and update all call sites that previously passed project string to pass `RunContext` instead [5 pts] (blocked by #2, #3) (A)

## Assignments Removal

- [x] (#5) Delete `src/team/assignments.rs` module entirely; remove `assignments` module export and `ASSIGNMENTS_FILE` constant from `src/team/mod.rs`; remove all `release_assignments_for_project()` calls and assignment checking logic from `src/runner.rs`; delete assignment-related tests [5 pts] (blocked by #4) (A)
- [x] (#6) Remove assignments.toml from git commit file lists in `src/git.rs`; remove any assignment display logic from `src/commands/projects.rs`; remove assignments.toml initialization from `src/commands/init.rs`; add migration logic to delete `.swarm-hug/assignments.toml` if it exists on first run [5 pts] (blocked by #5) (A)

## Testing and Validation

- [x] (#7) Write integration tests for parallel project execution (two projects with same agents running concurrently without conflict), restart isolation (cancelled sprint creates new hash, old artifacts remain), and cleanup scope (cleanup only affects current run's hash); verify all existing tests pass with updated signatures [5 pts] (blocked by #4, #5, #6) (A)
- [ ] (#8) Run `cargo clippy` and fix any new warnings; run `cargo test` and ensure all tests pass; perform manual verification of parallel projects, restart scenarios, and cleanup behavior; document any migration steps for old-style `agent-*` worktrees [4 pts] (blocked by #7)
