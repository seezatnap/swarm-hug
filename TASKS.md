# Tasks

## Current Task

- [x] (#25) Add integration test for full worktree lifecycle: feature branch -> agent worktree -> merge -> cleanup (blocked by #8, #9)
- [x] (#30) Ensure sprint planning, postmortem, and sprint completion commits run on the sprint feature branch (not the current branch)

## Worktree Overhaul

### Feature/Sprint Branch Management

- [x] (#1) Add `--target-branch` CLI flag to specify base/merge target branch (defaults to auto-detected main/master)
- [x] (#2) Implement target branch auto-detection logic (check for main, then master, then current branch)
- [x] (#3) Create feature/sprint branch creation function that forks from target branch (e.g., `greenfield-sprint-1`)
- [x] (#4) Add feature branch worktree creation under `.swarm-hug/<project>/worktrees/<sprint-name>`

### Agent Worktree Lifecycle

- [x] (#7) Update agent branch naming convention to use `agent-<name>` format (e.g., `agent-aaron`)
- [x] (#5) Modify agent worktree creation to fork from feature/sprint branch instead of target branch (blocked by #3)
- [x] (#6) Implement agent worktree recreation logic - delete and recreate fresh from feature branch after each task completion (blocked by #5)

### Task Merge Flow

- [x] (#8) Implement agent-to-feature-branch merge after task completion using `--no-ff` (blocked by #5)
- [x] (#9) Add agent worktree cleanup after successful merge to feature branch; Handle merge conflicts during agent-to-feature merge - surface errors in CHAT.md without crashing (blocked by #8)

### Merge Agent Implementation

- [x] (#11) Create merge agent prompt template in `prompts/` directory for feature-to-target merges
- [x] (#12) Implement merge agent execution logic that handles feature-to-target branch merging; Add conflict resolution guidance in merge agent prompt - preserve upstream, focus on getting code/tests out of conflict (blocked by #11)
- [x] (#14) Integrate merge agent invocation at sprint completion (blocked by #12, #3)

### Sprint Workflow Updates

- [x] (#15) Ensure sprint planning commits occur within feature/sprint branch; Ensure postmortem commits occur within feature/sprint branch; Ensure sprint close commits occur within feature/sprint branch (blocked by #4)

### Sprint Completion Flow

- [ ] (#18) Implement feature branch merge to target branch at sprint completion using merge agent (blocked by #14)
- [ ] (#19) Add feature branch deletion after successful merge to target; Add feature branch worktree cleanup after merge (blocked by #18)

### Configuration

- [x] (#21) Add `target_branch` field to config struct with default of `None`; Update config parsing to handle `--target-branch` CLI override (blocked by #1)
- [x] (#23) Store sprint/feature branch name in team state for reference during merge operations

### Testing

- [x] (#24) Add unit tests for target branch auto-detection logic (blocked by #2)
- [ ] (#26) Add integration test for merge agent conflict resolution scenario (blocked by #14)
- [x] (#27) Add test for `--target-branch` flag override behavior

### Documentation

- [ ] (#28) Update README.md with new worktree workflow documentation (blocked by #18)
- [ ] (#29) Document merge agent behavior and conflict resolution approach (blocked by #14)

### Maintenance

- [ ] (#31) Refactor src/runner.rs to split modules (file >1000 LOC)
