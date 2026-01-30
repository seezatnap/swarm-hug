# Tasks

## Worktree Infrastructure

- [x] (#1) Add shared target worktrees root directory creation at `./swarm-hub/.shared/worktrees`, ensuring the directory exists before any target worktree operations, and implement utility function to get the shared worktrees root path [5 pts] (A)

## Target Branch Worktree Management

- [x] (#2) Implement target branch worktree resolution by parsing `git worktree list --porcelain` output to locate worktrees for `refs/heads/<target-branch>`, returning worktree path if found or None if not [5 pts] (blocked by #1) (A)
- [x] (#3) Implement target branch worktree validation that checks if existing worktree is under shared root, erroring immediately with clear message if worktree exists outside `./swarm-hub/.shared/worktrees`, and reusing existing worktree if under shared root [5 pts] (blocked by #2) (A)
- [x] (#4) Implement target branch worktree creation for branches without existing worktrees, creating new worktree at `./swarm-hub/.shared/worktrees/<target-branch>` (with path sanitization for special characters), using current target-branch creation semantics for base commit [5 pts] (blocked by #3) (A)

## Merge Agent Integration

- [ ] (#5) Update post-sprint merge agent prompt to `cd` into the target-branch worktree (resolved via worktree management from #2-4) before running merge operations, ensuring primary repo is never used as merge working directory [5 pts] (blocked by #4)

## Testing

- [x] (#6) Add unit tests for worktree parsing and path resolution logic, including tests for porcelain output parsing, worktree path detection, and shared root path utilities [5 pts] (blocked by #2) (A)
- [ ] (#7) Add integration tests for target branch worktree lifecycle: test error case when worktree exists outside shared root, test reuse of existing worktree under shared root, and test creation of new worktree when none exists [5 pts] (blocked by #4)
- [ ] (#8) Add integration tests for parallel merge isolation verifying that concurrent sprints can merge without contending for primary repo working tree [5 pts] (blocked by #5)

## Follow-up tasks (from sprint review)
- [x] (#9) Implement target-branch worktree resolution (`find_target_branch_worktree[_in]`) that parses `git worktree list --porcelain` for `refs/heads/<branch>` and returns `Option<PathBuf>` (missing from merged code) (B)

## Follow-up tasks (from sprint review)
- [x] (#10) Fix `path_is_under_root` to compare canonicalized paths before returning true so `<root>/../outside` (or symlink escapes) are rejected; add a unit test for this escape case (A)
