# Tasks

## Follow-up

- [x] (#29) Fix help text to remove deprecated commands (sprint, plan, status, worktrees, worktrees-branch, cleanup, --no-tail)
- [x] (#30) Remove --no-tail flag usage from integration tests (blocked by #13)
- [x] (#31) Reconcile TASKS.md with .swarm-hug/cleanup-help/tasks.md - task #22 status is inconsistent between files

## Testing

- [x] (#18) Remove or update tests for sprint command (blocked by #7)
- [x] (#19) Remove or update tests for plan command (blocked by #8)
- [x] (#20) Remove or update tests for status command (blocked by #9)
- [x] (#21) Remove or update tests for worktrees command (blocked by #10)
- [x] (#22) Remove or update tests for worktrees-branch command (blocked by #11)
- [x] (#23) Remove or update tests for cleanup command (blocked by #12)
- [x] (#24) Remove or update tests for --no-tail option (blocked by #14)
- [ ] (#25) Add test verifying removed commands return appropriate error (blocked by #18, #19, #20, #21, #22, #23)

## CLI Module

- [x] (#1) Remove `sprint` command enum variant, from_str match arm, and dispatch in main.rs
- [x] (#2) Remove `plan` command enum variant, from_str match arm, and dispatch in main.rs
- [x] (#3) Remove `status` command enum variant, from_str match arm, and dispatch in main.rs
- [x] (#4) Remove `worktrees` command enum variant, from_str match arm, and dispatch in main.rs
- [x] (#5) Remove `worktrees-branch` command enum variant, from_str match arm, and dispatch in main.rs
- [x] (#6) Remove `cleanup` command enum variant, from_str match arm, and dispatch in main.rs

## Command Handlers

- [x] (#7) Remove sprint command handler function and module in src/commands/ (blocked by #1)
- [x] (#8) Remove plan command handler function and module in src/commands/ (blocked by #2)
- [x] (#9) Remove status command handler function and module in src/commands/ (blocked by #3)
- [x] (#10) Remove worktrees command handler function and module in src/commands/ (blocked by #4)
- [x] (#11) Remove worktrees-branch command handler function and module in src/commands/ (blocked by #5)
- [x] (#12) Remove cleanup command handler function and module in src/commands/ (blocked by #6)

## CLI Options

- [x] (#13) Remove `--no-tail` field from CliArgs struct and parse_args()
- [x] (#14) Remove `--no-tail` from Config struct if present and update related code (blocked by #13)

## Help Text Updates

- [x] (#15) Remove MULTI-PROJECT MODE section from help output (blocked by #1, #2, #3, #4, #5, #6)
- [x] (#16) Update examples section: remove deprecated examples and add `swarm -p authentication run` (blocked by #15)
- [ ] (#17) Verify help output matches target format exactly (blocked by #16)

## Validation

- [ ] (#26) Run full test suite and fix any failures (blocked by #25)
- [ ] (#27) Run cargo clippy and resolve any new warnings (blocked by #26)
- [ ] (#28) Manual verification that help output matches target format (blocked by #27)
