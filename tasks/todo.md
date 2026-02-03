# Task Plan

## Specs
- Improve handling of “agent branch not found” during agent-branch merge.
- Attempt safe recovery by recreating the missing branch from the agent worktree HEAD and retrying the merge.
- Emit clearer logs/chat with expected branch name and HEAD commit info when recovery fails.

## Plan
- [x] Locate merge failure path for NoBranch and decide where to add recovery.
- [x] Implement branch recreation + retry merge and enhance logging details.
- [x] Add/adjust a small unit test if feasible and note verification.

## Review
- [x] Summarize behavior changes and any follow-ups.
- [x] Record any verification performed.

Notes:
- When an agent branch is missing, the runner now recreates it from the agent worktree HEAD and retries the merge.
- If recovery fails, merge failure logs include the expected branch and HEAD info.

Tests:
- `cargo test test_create_branch_at_commit_creates_branch`
