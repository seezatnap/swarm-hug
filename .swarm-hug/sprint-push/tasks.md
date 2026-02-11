# Tasks

## Git Command Layer

- [ ] (#1) Add `push_branch_to_remote()` in `src/git.rs` that executes `git push origin <target_branch>` via `std::process::Command` (without `--force`), captures exit status/stdout/stderr, and returns structured success/failure data usable by runner logic and tests [5 pts]

## Sprint Runner Flow

- [ ] (#2) Update `src/runner.rs` after the `merged_ok` block to call push only when `config.target_branch` is explicitly `Some(...)`, the merge was not skipped (`sprint_branch != target_branch`), and shutdown was not requested; preserve current behavior when these conditions are not met [5 pts] (blocked by #1)
- [ ] (#3) Implement push outcome handling in `src/runner.rs`: on success print and record a push status message in merge logger/chat; on failure print a warning, log failure details for debugging, and continue returning sprint success because local merge already succeeded [5 pts] (blocked by #2)

## Testing

- [ ] (#4) Add unit tests validating push invocation/skipping based on applicability rules: explicit `--target-branch` provided vs auto-detected default, merge-skipped path (`sprint_branch == target_branch`), and shutdown-requested path [5 pts] (blocked by #2)
- [ ] (#5) Add unit test coverage proving push failure is non-fatal to `run_sprint` (success still returned after local merge) and that warning/debug logging paths are exercised [5 pts] (blocked by #3)
- [ ] (#6) Add an integration-style test (or equivalent command-construction test module) verifying the push command is formed exactly as `git push origin <target_branch>` with correct branch propagation and no force flag [5 pts] (blocked by #1, #2)
