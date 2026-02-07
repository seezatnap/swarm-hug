# Specifications: swarm-bugfixes

# PRD: Swarm Concurrent Variation Bug Fixes

## Overview
Fix two related bugs in swarm that prevent concurrent variations (`swarm run` with same `--project` but different `--target-branch`) from working correctly.

## Bug 1: Shared Runtime State Across Variations
**Source:** `swarm-shared-runtime-state-across-variations.md`

When running multiple `swarm run` invocations concurrently with the same `--project` but different `--target-branch`, all runs share the same runtime project state (tasks.md, sprint planning, worktree creation). Variations are not independent — they plan the same sprints, assign the same tasks, and create the same worktrees.

### Requirements
- Each `swarm run` with a unique `--target-branch` must maintain independent runtime state
- Read tasks.md from the target branch (via its worktree), not from the main worktree
- Fork sprint worktrees from the target branch tip, not the initial base commit
- Use per-run identifiers for sprint worktree names to avoid collisions
- Maintain per-run sprint history so concurrent runs don't interfere

## Bug 2: Merge Agent Fails on Stale Worktree Registration
**Source:** `swarm-worktree-merge-stale-registration.md`

The merge agent fails because a shared worktree path is already registered to a different branch (an agent branch from a prior sprint that wasn't cleaned up). This causes the affected variation to terminate with lost work.

### Requirements
- Clean up stale worktree registrations before attempting merges
- Force-remove and re-register worktree paths on branch mismatch detection
- Use distinct worktree paths per variation (incorporating target branch name into sprint worktree paths) to avoid collisions entirely

## Constraints
- Fixes must not break single-variation `swarm run` (no `--target-branch` or single target)
- Backward compatible with existing `.swarm-hug/` project layout
- Must handle the case where a previous run left behind stale worktrees

