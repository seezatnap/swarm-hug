# Specifications: followup-tasks-write-path

# PRD: Follow-up Tasks Written to Wrong Path

**Prerequisite:** This work assumes `sprint-branch-ordering` is complete and all sprint initialization files are correctly written to the sprint worktree.

## Summary
Fix the bug where follow-up tasks identified during post-sprint review are written to the main repo's `tasks.md` instead of the sprint worktree's `tasks.md`, causing them to be silently lost when the commit happens from the worktree context.

## Problem Statement

During post-sprint review in `run_post_sprint_review()`, the system:
1. Identifies follow-up tasks from the LLM review
2. Reports "Sprint review added N follow-up task(s)"
3. But the tasks never appear in the committed `tasks.md`

**Root Cause:** The follow-up tasks are written to `config.files_tasks` which points to the main repo path (e.g., `.swarm-hug/team/tasks.md`), but the commit operation runs from `feature_worktree` which has its own copy of the file. The write and commit target different file system locations.

**Code Location:** `src/runner.rs` lines 1009-1047

```rust
// BUG: Writes to main repo path
let mut current_content = fs::read_to_string(&config.files_tasks)
    .unwrap_or_default();
// ... append follow-up tasks ...
fs::write(&config.files_tasks, current_content)?;  // <-- writes to main repo

// BUG: Commits from worktree (different directory)
commit_files_in_worktree_on_branch(
    feature_worktree,           // <-- worktree root
    sprint_branch,
    &[&config.files_tasks, ...], // <-- but uses main repo path!
    &commit_msg,
)
```

**Result:**
- Follow-up tasks written to `<main_repo>/.swarm-hug/team/tasks.md`
- Commit looks for changes in `<worktree>/.swarm-hug/team/tasks.md`
- Worktree's copy is unchanged, so nothing gets committed
- Main repo has uncommitted changes (same pollution pattern as sprint-branch-ordering)

**Evidence:** User sees "Sprint review added 1 follow-up task(s)" in logs but no `## Follow-up tasks` section appears in the committed `tasks.md`.

## Goals
- Follow-up tasks are written to the sprint worktree's `tasks.md`
- Follow-up tasks are properly committed to the sprint branch
- Main repo remains clean after sprint review
- Message "Sprint review added N follow-up task(s)" matches actual committed tasks

## Non-goals
- Changes to how follow-up tasks are parsed from LLM output
- Changes to the review prompt
- Changes to sprint initialization (covered by sprint-branch-ordering)

## Affected Files

| File | Lines | Current Operation | Issue |
|------|-------|-------------------|-------|
| `src/runner.rs` | 1009 | `fs::read_to_string(&config.files_tasks)` | Reads from main repo |
| `src/runner.rs` | 1025 | `fs::write(&config.files_tasks, ...)` | Writes to main repo |
| `src/runner.rs` | 1043 | `&[&config.files_tasks, &config.files_chat]` | Passes main repo paths to worktree commit |

## Proposed Solution

The `run_post_sprint_review()` function already receives `feature_worktree` and `team_name` parameters. Use them to construct worktree-relative paths instead of using `config.files_tasks`:

```rust
// In run_post_sprint_review() - replace config.files_tasks usage

// Construct worktree-relative paths
let worktree_swarm_dir = Path::new(feature_worktree)
    .join(".swarm-hug")
    .join(&team_name);
let worktree_tasks_path = worktree_swarm_dir.join("tasks.md");
let worktree_chat_path = worktree_swarm_dir.join("chat.md");

// Read from worktree (not config.files_tasks)
let mut current_content = fs::read_to_string(&worktree_tasks_path)
    .unwrap_or_default();

// Ensure newline before appending
if !current_content.ends_with('\n') {
    current_content.push('\n');
}

// Add follow-up tasks
current_content.push_str("\n## Follow-up tasks (from sprint review)\n");
for task in &formatted_follow_ups {
    current_content.push_str(task);
    current_content.push('\n');
    println!("    {}", task);
}

// Write to worktree (not config.files_tasks)
fs::write(&worktree_tasks_path, current_content)
    .map_err(|e| format!("failed to write follow-up tasks: {}", e))?;

// Commit using worktree paths
commit_files_in_worktree_on_branch(
    feature_worktree,
    sprint_branch,
    &[
        worktree_tasks_path.to_str().unwrap(),
        worktree_chat_path.to_str().unwrap(),
    ],
    &commit_msg,
)
```

### Also fix chat.md write

The same issue likely affects `chat.md` writes during review. Check and fix:
- Line 989-991: `chat::write_message(&config.files_chat, ...)` before review
- Line 1033-1034: `chat::write_message(&config.files_chat, ...)` after review

These should also use worktree-relative paths.

## Testing Plan

### Manual Test
1. Run a sprint that completes with tasks (ensure LLM review identifies follow-ups)
2. Check that "Sprint review added N follow-up task(s)" message appears
3. Verify `git status` on main branch shows NO uncommitted changes
4. Verify sprint branch `tasks.md` contains "## Follow-up tasks" section
5. Verify the follow-up commit exists in sprint branch history

### Automated Test
```rust
#[test]
fn test_followup_tasks_written_to_worktree() {
    // Setup: create repo, run sprint to completion
    // Mock: LLM returns 1 follow-up task during review
    // Run: post-sprint review
    // Assert: main repo tasks.md unchanged
    // Assert: worktree tasks.md has "## Follow-up tasks" section
    // Assert: commit exists with follow-up tasks message
}
```

## Acceptance Criteria
- [ ] Follow-up tasks appear in sprint branch's `tasks.md` after review
- [ ] Main repo `tasks.md` is not modified by sprint review
- [ ] Main repo `chat.md` is not modified by sprint review
- [ ] "Sprint review added N follow-up task(s)" message matches actual committed tasks
- [ ] Follow-up tasks commit appears in sprint branch git history
- [ ] `cargo test` passes
- [ ] `cargo clippy` reports no new warnings

## Implementation Plan

1. Update `run_post_sprint_review()` to construct worktree-relative paths for `tasks.md`
2. Update `run_post_sprint_review()` to construct worktree-relative paths for `chat.md`
3. Replace all `config.files_tasks` and `config.files_chat` usage with worktree paths
4. Add test for follow-up tasks appearing in sprint branch
5. Manual verification: run sprint to completion, verify follow-up tasks committed correctly

## Risks
- **chat::write_message API**: May need to accept path parameter or be refactored to support worktree context
- **Other config.files_* usages**: Audit `run_post_sprint_review()` for any other main-repo path references

