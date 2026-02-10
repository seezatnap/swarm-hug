# Tasks

## Merge Agent Prompt

- [x] (#1) Refactor `prompts/merge_agent.md` preflight flow using the two bug reports (`~/bugs/2026-02-09T2045-swarm-merge-agent-failure.md` and `~/bugs/2026-02-09T2155-swarm-merge-agent-squash-instead-of-merge.md`) so Step 0 clearly aborts only pre-existing stale merges and never aborts the merge started by this run after conflicts occur [5 pts] (A)
- [x] (#2) Add a strict `Critical Rules` section in `prompts/merge_agent.md` that explicitly bans `git merge --squash`, `git cherry-pick`, `git diff | git apply`, and `git rebase`, and requires conflict resolution to stay inside the original `git merge --no-ff` path [5 pts] (blocked by #1) (A)
- [x] (#3) Add merge-state safety and verification instructions in `prompts/merge_agent.md`: require `.git/MERGE_HEAD` before any manual commit, define recovery by restarting `git merge --no-ff` if `MERGE_HEAD` is lost, and require post-commit verification that `git rev-parse HEAD^2` succeeds [5 pts] (blocked by #1) (A)

## Runner Reliability

- [x] (#4) Update `src/runner.rs` to retry `run_merge_agent` exactly once when `ensure_feature_merged` fails, then re-check merge status before returning a fatal error, preserving clear error/log context across attempts [5 pts] (B)
- [x] (#5) Enhance `ensure_feature_merged` to keep ancestry validation and additionally verify the target branch tip has two parents when feature and target branches differ, returning a specific squash-merge diagnostic when a single-parent commit is detected [5 pts] (B)

## Testing & Verification

- [x] (#6) Add or update tests for runner retry behavior to cover: one retry on initial verification failure, success on second attempt, and fatal exit after second failure with no extra retries [5 pts] (blocked by #4) (B)
- [x] (#7) Add or update tests for `ensure_feature_merged` parent-count enforcement and error messaging, including single-parent squash detection, valid two-parent merge acceptance, and unchanged behavior when feature and target are the same branch [5 pts] (blocked by #5) (B)
- [A] (#8) Execute the existing test suite plus new targeted tests for prompt and runner changes, then update any affected fixtures/expected messages so the full suite passes cleanly [5 pts] (blocked by #2, #3, #6, #7)

## Follow-up tasks (from sprint review)
- [x] (#9) Mark tasks #6 and #7 as complete in TASKS.md — the tests required by #6 (runner retry: success on retry, failure on both attempts, error context preservation) were delivered inline with #4, and the tests required by #7 (parent-count: squash detection, two-parent acceptance, same-branch skip) were delivered inline with #5 (C)
