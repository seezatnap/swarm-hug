# Specifications: source-branch

# PRD: --source-branch Option

## Summary

Add a `--source-branch` CLI option to swarm so users can specify which branch to fork from (the "source") independently from which branch to merge into (the "target"). Currently `--target-branch` serves double duty as both the fork-from point and the merge destination, which makes it impossible to fork off a feature branch into a follow-up branch.

## Motivation

Practical workflow the user wants:

1. `swarm run --source-branch main --target-branch feature-1` — fork from main, merge results into feature-1
2. `swarm run --source-branch feature-1 --target-branch feature-1-follow-ups` — fork from feature-1, merge results into feature-1-follow-ups (follow-up work without merging feature-1 first)

## CLI Semantics

| Flags provided | source_branch | target_branch | Behavior |
|---|---|---|---|
| Neither | auto-detect (main/master) | auto-detect (main/master) | Backwards-compatible default |
| `--source-branch X` only | X | X | Source and target are the same branch |
| `--target-branch Y` only | — | — | **ERROR** with explicit message: must also provide `--source-branch` |
| `--source-branch X --target-branch Y` | X | Y | Fork from X, merge into Y |

## Error Messages

When `--target-branch` is provided without `--source-branch`:
```
Error: --target-branch requires --source-branch. Specify both flags explicitly.
  Example: swarm run --source-branch main --target-branch feature-1
```

## Implementation Scope

### 1. CLI Parsing (`src/config/cli.rs`)
- Add `source_branch: Option<String>` to `CliArgs`
- Parse `--source-branch` flag

### 2. Config Types (`src/config/types.rs`)
- Add `source_branch: Option<String>` to `Config`
- In `apply_cli()` / `Config::load()`, implement the validation matrix above
- When both are provided, set both fields
- When only `--source-branch` is provided, set target_branch = source_branch
- When only `--target-branch` is provided, return an error
- When neither is provided, auto-detect as today (both default to main/master)

### 3. Help Text (`src/config/cli.rs` or wherever --help is generated)
- Add `--source-branch` to help output with description:
  ```
  --source-branch <NAME>    Branch to fork/branch from. If --target-branch is omitted, this branch is also the merge target.
  ```
- Update `--target-branch` help text:
  ```
  --target-branch <NAME>    Branch to merge results into. Requires --source-branch.
  ```

### 4. Runner / Worktree Usage (`src/runner.rs`, `src/worktree/`)
- Where sprint/feature branches are created FROM the target branch, change to use `source_branch` instead
- Where merges happen INTO the target branch, keep using `target_branch`
- Specifically:
  - `create_feature_worktree_in()` / `create_feature_branch_in()`: use source_branch as the base
  - `sync_target_branch_state()`: sync FROM source_branch (or target_branch — both may be needed depending on state location)
  - Merge operations (sprint→target): still merge INTO target_branch
  - Target branch worktree creation for merge: still uses target_branch

### 5. TUI Passthrough (`src/commands/run.rs`)
- When re-invoking in TUI mode, pass `--source-branch` through to the subprocess

### 6. Integration Tests (`tests/integration.rs`)
- **Test: error when --target-branch without --source-branch** — verify the exact error message
- **Test: --source-branch alone sets both source and target** — run a sprint with only --source-branch, verify it forks from and merges to that branch
- **Test: --source-branch + --target-branch** — run a sprint with both, verify it forks from source and merges into target (the key "follow-up branch" workflow)
- **Test: neither flag provided** — verify backwards-compatible auto-detection still works
- **Test: source branch doesn't exist** — verify clear error
- **Test: follow-up workflow end-to-end** — simulate the two-step workflow: first run with source=main target=feature-1, then run with source=feature-1 target=feature-1-follow-ups. Verify feature-1-follow-ups contains commits from both runs.

## Out of Scope
- Changes to `swarm project init` (it doesn't run sprints)
- Changes to task planning or agent logic
- Any UI/TUI changes beyond passthrough

