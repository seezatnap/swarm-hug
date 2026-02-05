# Tasks

## CLI & Configuration

- [x] (#1) Add `source_branch: Option<String>` across CLI and config types by updating `src/config/cli.rs` (`CliArgs` + `--source-branch` parsing) and `src/config/types.rs` (`Config` field), and update help output so `--source-branch` describes fork-from plus fallback merge target behavior while `--target-branch` states it requires `--source-branch` [5 pts] (A)
- [x] (#2) Implement the full branch-flag resolution matrix in `apply_cli()` / `Config::load()`: neither flag auto-detects `main/master` for both source and target, `--source-branch` alone sets both source and target to that value, both flags set independent source/target, and `--target-branch` alone returns the exact specified error text and example [5 pts] (blocked by #1) (A)

## Runner & Worktree

- [x] (#3) Refactor branch creation paths in `src/runner.rs` and `src/worktree/` so feature/sprint branches are created from `source_branch` (not `target_branch`), including `create_feature_worktree_in()` / `create_feature_branch_in()` call chains [5 pts] (blocked by #2) (A)
- [x] (#4) Update sync and merge flow to preserve split semantics: `sync_target_branch_state()` syncs from the correct branch input for the new model, merge operations still merge sprint output into `target_branch`, target-branch merge worktree creation remains target-based, and missing source branch errors are surfaced clearly [5 pts] (blocked by #3) (A)

## Run Command / TUI

- [x] (#5) Update `src/commands/run.rs` TUI re-invocation to pass `--source-branch` through to subprocesses and keep argument behavior consistent with non-TUI execution for all supported flag combinations [5 pts] (blocked by #2) (A)

## Integration Testing

- [x] (#6) Add integration coverage in `tests/integration.rs` for flag semantics and compatibility: exact error message when only `--target-branch` is provided, `--source-branch`-only mode sets both source and target, and neither-flag mode preserves current auto-detection behavior [5 pts] (blocked by #2, #5) (A)
- [A] (#7) Add integration coverage for branch-flow behavior: `--source-branch + --target-branch` forks from source and merges into target, non-existent source branch returns a clear error, and the two-step follow-up workflow (`main -> feature-1`, then `feature-1 -> feature-1-follow-ups`) yields expected commits in `feature-1-follow-ups` [5 pts] (blocked by #3, #4, #6)
