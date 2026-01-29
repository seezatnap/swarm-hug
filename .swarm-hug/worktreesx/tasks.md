# Tasks

## Worktree Overhaul

### Feature/Sprint Branch Management

- [x] (#1) Add `--target-branch` CLI flag to specify base/merge target branch (defaults to auto-detected main/master) (A)
- [x] (#2) Implement target branch auto-detection logic (check for main, then master, then current branch) (A)
- [x] (#3) Create feature/sprint branch creation function that forks from target branch (e.g., `greenfield-sprint-1`) (A)
- [x] (#4) Add feature branch worktree creation under `.swarm-hug/<project>/worktrees/<sprint-name>` (A)

### Agent Worktree Lifecycle

- [x] (#5) Modify agent worktree creation to fork from feature/sprint branch instead of target branch (blocked by #3) (A)
- [x] (#6) Implement agent worktree recreation logic - delete and recreate fresh from feature branch after each task completion (blocked by #5) (A)
- [x] (#7) Update agent branch naming convention to use `agent-<name>` format (e.g., `agent-aaron`) (B)

### Task Merge Flow

- [x] (#8) Implement agent-to-feature-branch merge after task completion using `--no-ff` (blocked by #5) (A)
- [x] (#9) Add agent worktree cleanup after successful merge to feature branch; Handle merge conflicts during agent-to-feature merge - surface errors in CHAT.md without crashing (blocked by #8) (A)

### Merge Agent Implementation

- [x] (#11) Create merge agent prompt template in `prompts/` directory for feature-to-target merges (C)
- [x] (#12) Implement merge agent execution logic that handles feature-to-target branch merging; Add conflict resolution guidance in merge agent prompt - preserve upstream, focus on getting code/tests out of conflict (blocked by #11) (B)
- [x] (#14) Integrate merge agent invocation at sprint completion (blocked by #12, #3) (A)

### Sprint Workflow Updates

- [x] (#15) Ensure sprint planning commits occur within feature/sprint branch; Ensure postmortem commits occur within feature/sprint branch; Ensure sprint close commits occur within feature/sprint branch (blocked by #4) (B)
- [x] Ensure sprint planning, postmortem, and sprint completion commits run on the sprint feature branch (not the current branch) (B)

### Sprint Completion Flow

- [x] (#18) Implement feature branch merge to target branch at sprint completion using merge agent (blocked by #14) (A)
- [x] (#19) Add feature branch deletion after successful merge to target; Add feature branch worktree cleanup after merge (blocked by #18) (A)

### Configuration

- [x] (#21) Add `target_branch` field to config struct with default of `None`; Update config parsing to handle `--target-branch` CLI override (blocked by #1) (A)
- [x] (#23) Store sprint/feature branch name in team state for reference during merge operations (blocked by #3) (C)

### Testing

- [x] (#24) Add unit tests for target branch auto-detection logic (blocked by #2) (A)
- [x] (#25) Add integration test for full worktree lifecycle: feature branch → agent worktree → merge → cleanup (blocked by #8, #9) (B)
- [x] (#26) Add integration test for merge agent conflict resolution scenario (blocked by #14) (B)
- [x] (#27) Add test for `--target-branch` flag override behavior (blocked by #21) (A)

### Documentation

- [x] (#28) Update README.md with new worktree workflow documentation (blocked by #18) (A)
- [x] (#29) Document merge agent behavior and conflict resolution approach (blocked by #14) (A)

## Follow-up tasks (from sprint review)
- [x] (#30) Ensure sprint planning, postmortem, and sprint completion commits run on the sprint feature branch (not the current branch) (B)

## Follow-up tasks (from sprint review)
- [x] (#31) Add an integration test that exercises merge-agent conflict resolution (conflict path currently untested) (A)
