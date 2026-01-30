# PRD: Target Branch Worktrees

## Summary
`--target-branch` currently merges into a branch checked out in the primary repo, which is risky when multiple sprints/merge agents run in parallel. Change the meaning of `--target-branch` so it always refers to a dedicated worktree under a shared worktrees directory, and perform post-sprint merges inside that worktree instead of the primary repo.

## Problem Statement
Today, `--target-branch` is just a branch name in the primary repo, and post-sprint merge operations run in the primary working directory. This allows concurrent merges to contend for the same working tree and causes brittle parallelism (e.g., two sprints trying to merge at once). We need isolation so merges happen in a dedicated worktree per target branch.

## Goals
- Treat `--target-branch` as a worktree-backed branch, not a branch checked out in the primary repo.
- Use a shared worktrees root at `./swarm-hub/.shared/worktrees`.
- If the target branch already exists but is not a worktree under the shared root, fail fast with a clear error.
- Otherwise, create (or reuse) the target branch worktree in the shared root.
- Post-sprint merge agent runs the merge inside the target branch worktree, not the primary repo.

## Non-goals
- Changing how `--target-branch` is parsed or named.
- Altering the default target branch selection logic (main/master detection stays as-is).
- Changing sprint/agent worktree behavior beyond the target branch worktree requirement.

## Current Behavior
- `--target-branch` selects a branch name in the primary repo.
- The post-sprint merge agent merges the sprint branch into that target branch while `cwd` is the primary repo.
- Parallel merges contend for the same working tree, causing conflicts and race conditions.

## Desired Behavior
When `--target-branch` is specified (or resolved by default), it must correspond to a worktree located under `./swarm-hub/.shared/worktrees`. The merge agent must `cd` into that worktree before merging. This isolates merges per target branch and removes the primary repo as the merge surface.

## Implementation

### 1) Add a shared target worktrees root
- **Path:** `./swarm-hub/.shared/worktrees`
- Ensure the directory exists before any target worktree operations.

### 2) Enforce target branch worktree ownership
Resolve the target branch name (explicit `--target-branch` or default). Then:

- Parse `git worktree list --porcelain` to locate worktrees for `refs/heads/<target-branch>`.
- If a worktree exists for the target branch and its path is **not** under `./swarm-hub/.shared/worktrees`, **error immediately**.
- If a worktree exists for the target branch under the shared root, **reuse it**.
- If the branch does **not** exist, **create a new worktree** for it in the shared root.
  - Worktree path: `./swarm-hub/.shared/worktrees/<target-branch>` (or a sanitized variant if needed).
  - Base commit: whatever the current target-branch creation logic uses today (do not change semantics; only change the worktree location).

### 3) Move post-sprint merges to the target worktree
- The post-sprint merge agent should `cd` to the worktree created in step 2 and perform the merge there.
- The primary repo should never be the working directory for the post-sprint merge.

## Acceptance Criteria
- If `--target-branch` is provided and the branch exists outside `./swarm-hub/.shared/worktrees`, the run fails with a clear error.
- If the target branch has a worktree under `./swarm-hub/.shared/worktrees`, that worktree is reused.
- If no such worktree exists, one is created under `./swarm-hub/.shared/worktrees`.
- Post-sprint merges occur inside the target worktree, not the primary repo.
- Parallel sprints/merges can run without contending for the primary repo working tree.

## Risks / Notes
- If a target branch already exists in the primary repo, users must migrate it to the shared worktrees root (or delete/recreate) before running.
- The shared worktrees directory must be accessible to all swarm runs expected to merge into the same target branch.
- **Implementation note:** The post-sprint merge behavior is defined in the merge agent’s prompt. That prompt (and only that prompt logic) must be updated to `cd` into the target-branch worktree before running the merge.
