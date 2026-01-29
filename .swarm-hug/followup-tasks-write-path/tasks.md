# Tasks

Now I understand the context. The user wants me to convert the PRD into a task list. The PRD describes a bug where follow-up tasks are written to the wrong path. Looking at the code, I can see that parts of this have already been addressed (the code uses `worktree_tasks_path`), but there are still issues - particularly with `config.files_chat` being used instead of a worktree-relative path.

Let me now produce the task breakdown as requested. The user specifically asked for PRD-to-tasks conversion with the exact output format (starting with `## ` heading).

## Path Fix Implementation

- [ ] (#1) Construct worktree-relative `chat.md` path in `run_post_sprint_review()` using `feature_worktree` and `team_name`, replacing all `config.files_chat` usage with the worktree path for both pre-review and post-review chat writes (lines 1025-1030, 1073-1074) [5 pts]

## Commit Path Fix

- [ ] (#2) Update `commit_files_in_worktree_on_branch()` call to use worktree-relative chat path instead of `config.files_chat`, ensuring both tasks.md and chat.md are committed from the worktree context (line 1084) [4 pts] (blocked by #1)

## Testing

- [ ] (#3) Add automated test `test_followup_tasks_written_to_worktree()` that verifies: (a) follow-up tasks appear in sprint worktree's tasks.md, (b) main repo tasks.md unchanged, (c) main repo chat.md unchanged, (d) commit exists in sprint branch history [5 pts] (blocked by #2)

- [ ] (#4) Manual verification: run full sprint to completion, verify "Sprint review added N follow-up task(s)" message appears, main repo shows no uncommitted changes, sprint branch tasks.md contains "## Follow-up tasks" section, and commit exists in history [5 pts] (blocked by #3)

## Validation

- [ ] (#5) Run `cargo test` and `cargo clippy` to ensure no test failures or new warnings introduced by the path fixes [4 pts] (blocked by #2)
