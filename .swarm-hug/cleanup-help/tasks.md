# Tasks

## CLI Module

- [x] (#1) Remove `sprint` command enum variant, from_str match arm, and dispatch in main.rs (A)
- [x] (#2) Remove `plan` command enum variant, from_str match arm, and dispatch in main.rs (A)
- [x] (#3) Remove `status` command enum variant, from_str match arm, and dispatch in main.rs (A)
- [x] (#4) Remove `worktrees` command enum variant, from_str match arm, and dispatch in main.rs (B)
- [x] (#5) Remove `worktrees-branch` command enum variant, from_str match arm, and dispatch in main.rs (B)
- [x] (#6) Remove `cleanup` command enum variant, from_str match arm, and dispatch in main.rs (B)

## Command Handlers

- [x] (#7) Remove sprint command handler function and module in src/commands/ (blocked by #1)
- [x] (#8) Remove plan command handler function and module in src/commands/ (blocked by #2)
- [A] (#9) Remove status command handler function and module in src/commands/ (blocked by #3)
- [x] (#10) Remove worktrees command handler function and module in src/commands/ (blocked by #4)
- [x] (#11) Remove worktrees-branch command handler function and module in src/commands/ (blocked by #5) (?)
- [A] (#12) Remove cleanup command handler function and module in src/commands/ (blocked by #6)

## CLI Options

- [x] (#13) Remove `--no-tail` field from CliArgs struct and parse_args() (B)
- [B] (#14) Remove `--no-tail` from Config struct if present and update related code (blocked by #13)

## Help Text Updates

- [A] (#15) Remove MULTI-PROJECT MODE section from help output (blocked by #1, #2, #3, #4, #5, #6)
- [ ] (#16) Update examples section: remove deprecated examples and add `swarm -p authentication run` (blocked by #15)
- [ ] (#17) Verify help output matches target format exactly (blocked by #16)

## Testing

- [ ] (#18) Remove or update tests for sprint command (blocked by #7)
- [ ] (#19) Remove or update tests for plan command (blocked by #8)
- [ ] (#20) Remove or update tests for status command (blocked by #9)
- [ ] (#21) Remove or update tests for worktrees command (blocked by #10)
- [B] (#22) Remove or update tests for worktrees-branch command (blocked by #11)
- [ ] (#23) Remove or update tests for cleanup command (blocked by #12)
- [ ] (#24) Remove or update tests for --no-tail option (blocked by #14)
- [ ] (#25) Add test verifying removed commands return appropriate error (blocked by #18, #19, #20, #21, #22, #23)

## Validation

- [ ] (#26) Run full test suite and fix any failures (blocked by #25)
- [ ] (#27) Run cargo clippy and resolve any new warnings (blocked by #26)
- [ ] (#28) Manual verification that help output matches target format (blocked by #27)
