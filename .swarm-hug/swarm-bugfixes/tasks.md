# Tasks

## Runtime State Isolation

- [x] (#1) Introduce a per-run runtime identifier (for example, derived from project + target branch + run instance) and thread it through swarm run context so concurrent variations no longer share runtime state keys [5 pts] (A)
- [ ] (#2) Refactor planning/task orchestration to use per-run state namespaces, including sprint planning artifacts, task assignment state, and persisted sprint history so each target branch variation is fully isolated [5 pts] (blocked by #1)
- [ ] (#3) Change `tasks.md` resolution to read from the target branch worktree instead of the main worktree, with explicit fallback to current behavior when running single-variation/no `--target-branch` [5 pts] (blocked by #1)
- [ ] (#4) Update sprint worktree creation to fork from the current target-branch tip (not the initial base commit) and generate per-run worktree names/paths to eliminate cross-variation collisions [5 pts] (blocked by #1)

## Worktree And Merge Recovery

- [ ] (#5) Implement merge-agent branch-mismatch recovery for registered worktree paths by detecting stale path-to-branch registrations, force-removing stale entries, and re-registering expected branch/path before merge [5 pts] (blocked by #4)
- [ ] (#6) Add stale worktree cleanup in merge/worktree lifecycle flows to handle leftovers from previous runs, preserving valid active worktrees while reconciling abandoned stale registrations safely [5 pts] (blocked by #5)

## Compatibility And Migration

- [ ] (#7) Preserve backward compatibility with existing `.swarm-hug/` project layout by adding migration/fallback handling for legacy runtime keys/worktree conventions and confirming no behavior change for single-variation runs [5 pts] (blocked by #2, #3, #4, #6)

## Testing And Validation

- [ ] (#8) Add automated concurrency tests for same-project/different-target-branch runs verifying independent sprint plans, independent task assignments, target-branch `tasks.md` loading, target-tip worktree forking, and isolated sprint history [5 pts] (blocked by #2, #3, #4)
- [ ] (#9) Add automated stale-worktree/merge regression tests covering mismatch cleanup + re-registration, recovery from prior-run stale worktrees, no lost-work merge behavior, and single-variation regression checks [5 pts] (blocked by #6, #7)
