# Tasks

I understand. The user is asking me to convert the PRD about "Sprint Branch Creation Before Setup Files" into a structured task list. Let me do the two-pass process internally and output only the final consolidated tasks.

## Sprint History Module

- [x] (#1) Add `peek_next_sprint()` method to `SprintHistory` struct in `src/team/sprint_history.rs` that returns `total_sprints + 1` without mutating state, and add `increment()` method to separate mutation from the existing `next_sprint()` logic [4 pts] (A)

- [x] (#2) Add `load_from(path: &Path)` method to `SprintHistory` struct that loads from an explicit path instead of deriving from team name, supporting both existing file and first-sprint (file not exists) cases [5 pts] (A)

## Team State Module

- [x] (#3) Add `load_from(path: &Path)` method to `TeamState` struct in `src/team/state.rs` that loads from an explicit path instead of deriving from team name, with support for creating default state when file doesn't exist [5 pts] (B)

## Runner Reordering

- [x] (#4) Reorder `run_sprint()` in `src/runner.rs` to create sprint branch/worktree FIRST before any file writes: determine sprint number using `peek_next_sprint()`, compute sprint branch name, then call `create_feature_worktree_in()` before loading/saving any state files [5 pts] (blocked by #1) (A)

- [x] (#5) Update `run_sprint()` file write operations to use sprint worktree paths: construct `worktree_swarm_dir` from feature worktree path, use `load_from()` methods to load state from worktree, and write `sprint-history.json`, `team-state.json`, and `tasks.md` to the worktree instead of main repo [5 pts] (blocked by #2, #3, #4) (A)

## Testing

- [x] (#6) Add integration test `test_sprint_init_keeps_target_branch_clean` that creates a repo with tasks.md, starts a sprint, and asserts main branch has no uncommitted changes while sprint branch has all state files committed [5 pts] (blocked by #5) (A)

- [x] (#7) Add integration test `test_first_sprint_creates_files_in_sprint_branch` that creates a repo with no existing sprint-history.json or team-state.json, starts first sprint, and asserts files are created only in sprint branch [4 pts] (blocked by #5) (A)

## Validation

- [ ] (#8) Run `cargo test` and `cargo clippy` to verify all tests pass and no new warnings are introduced, fix any issues found during validation [5 pts] (blocked by #6, #7)
