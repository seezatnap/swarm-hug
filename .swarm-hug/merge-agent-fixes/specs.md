# Specifications: merge-agent-fixes

# PRD: Fix Swarm Merge Agent Reliability

## Problem

The merge agent fails ~50% of the time when conflicts exist. Two distinct failure modes:

1. **Outright failure**: Agent cannot resolve conflicts and the merge never completes.
2. **Squash-merge**: Agent resolves conflicts but creates a single-parent commit instead of a true 2-parent merge commit. Git does not consider the sprint branch merged, so the ancestry check fails and kills the pipeline.

Both bugs are filed in `~/bugs/2026-02-09T2045-swarm-merge-agent-failure.md` and `~/bugs/2026-02-09T2155-swarm-merge-agent-squash-instead-of-merge.md`.

## Root Cause Analysis

The prompt (`prompts/merge_agent.md`) has several gaps that allow an LLM to accidentally destroy merge state:

1. **Step 0 tells the agent to abort in-progress merges**, but doesn't distinguish a *stale* leftover merge from the merge *just started in step 3*. An LLM that loops back to preflight checks after hitting conflicts will abort its own merge, losing MERGE_HEAD.
2. **No explicit ban on alternative strategies**: nothing prevents the LLM from falling back to `git merge --squash`, `git cherry-pick`, or `git diff | git apply` when `--no-ff` produces conflicts.
3. **No MERGE_HEAD awareness**: the prompt doesn't explain that `MERGE_HEAD` must exist when committing after conflict resolution to produce a 2-parent merge commit. If MERGE_HEAD is lost, `git commit` silently creates a 1-parent commit.
4. **No post-commit verification**: the agent is never told to confirm the commit has 2 parents.
5. **No recovery path for lost MERGE_HEAD**: if the merge state is lost, there's no instruction to re-initiate `git merge --no-ff` rather than manually committing.

There are also Rust-side code improvements available:

6. **No retry in runner.rs**: when `ensure_feature_merged` fails, the runner gives up immediately. A single retry of the merge agent would recover many transient failures.
7. **Ancestry-only verification**: `ensure_feature_merged` checks `merge-base --is-ancestor` but does not verify that the merge commit actually has 2 parents, which would catch the squash-merge bug earlier and produce a more diagnostic error.

## Scope

### In scope (prompt fixes — primary focus)

- Rewrite `prompts/merge_agent.md` to address items 1–5 above.
- Add a critical rules section that explicitly bans `--squash`, `cherry-pick`, `diff | apply`, and `rebase`.
- Add a MERGE_HEAD guard before any manual commit: check that `.git/MERGE_HEAD` exists.
- Add a post-commit verification step: `git rev-parse HEAD^2` must succeed (confirming 2 parents).
- Add a MERGE_HEAD recovery path: if MERGE_HEAD is lost after conflicts, re-run `git merge --no-ff` from scratch.
- Restructure step 0 to make it clear that abort is ONLY for pre-existing stale merges, not for the merge this agent initiates.

### In scope (code wins)

- Add a single retry of `run_merge_agent` in `src/runner.rs` when `ensure_feature_merged` fails (before returning the fatal error).
- Improve `ensure_feature_merged` to also verify the tip commit of the target branch has 2 parents when the feature and target are different branches, producing a clearer error message for squash-merge cases.

### Out of scope

- Engine fallback (trying a different LLM engine for the merge)
- Skip-and-continue (allowing the operator to intervene mid-run)
- Changes to agent-to-sprint branch merges (different code path)

## Success Criteria

- The merge agent prompt explicitly bans squash/cherry-pick/apply strategies.
- The prompt includes MERGE_HEAD verification before committing.
- The prompt includes post-commit parent count verification.
- The runner retries the merge agent once on failure.
- `ensure_feature_merged` produces a specific "squash-merge detected" error when the commit has 1 parent.
- Existing tests pass; new tests cover the retry path and parent-count check.

