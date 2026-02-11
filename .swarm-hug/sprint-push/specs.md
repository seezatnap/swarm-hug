# Specifications: sprint-push

# Sprint Push: Auto-push target branch to GitHub after sprint completion

## Overview

After a swarm sprint successfully merges into a `--target-branch`, automatically push that branch to the GitHub remote using `git push`. This only applies when `--target-branch` was explicitly provided by the user.

## Motivation

Currently, after a sprint completes and merges into the target branch, the results only exist locally. Operators must manually push the branch to GitHub. This feature automates that step so sprint results are immediately available on the remote for review.

## Requirements

### 1. Push target branch after successful merge

- **Where**: In `src/runner.rs`, after a successful merge into the target branch (after the `merged_ok` block around line 1651), push the target branch to the remote.
- **Condition**: Only push when `config.target_branch` is `Some(...)` (i.e., the user explicitly passed `--target-branch`). If no target branch was specified, do nothing.
- **Method**: Use `git push origin <target_branch>` via `std::process::Command`. Do NOT use `--force`.
- **Logging**: Print a status message (e.g., `"  Push: pushed '<branch>' to origin"`) and log it to the merge logger and chat. On failure, print a warning but do NOT fail the sprint — the merge already succeeded locally.

### 2. Handle push failures gracefully

- If the push fails (e.g., no remote, auth issues, network error), print a warning message and continue. The sprint result should still be returned as successful since the local merge succeeded.
- Log the failure to the merge logger for debugging.

### 3. Skip push when not applicable

- If `--target-branch` was not provided (target equals the auto-detected default), skip the push entirely.
- If the sprint branch equals the target branch (merge was skipped), skip the push.
- If shutdown was requested, skip the push.

### 4. Tests

- Unit test: verify push is called (or skipped) based on `config.target_branch` presence.
- Unit test: verify push failure does not cause `run_sprint` to return an error.
- Integration-style test: the push command is constructed correctly with the right branch name.

## Non-goals

- No PR creation. Only push the branch.
- No force-push. Standard `git push` only.
- No new CLI flags. The behavior is automatic when `--target-branch` is used.

## Files to modify

- `src/runner.rs` — Add push logic after successful merge
- `src/git.rs` — Add a `push_branch_to_remote()` helper function
- Tests in existing test files or new test module

