# Tasks

## Commands and runner split

- [x] (#1) Create `src/commands` module and move CLI command handlers into `init.rs`, `run.rs`, `status.rs`, `agents.rs`, `worktrees.rs`, `projects.rs`, `misc.rs`; re-export via `src/commands/mod.rs` (A)
- [x] (#2) Move sprint orchestration and result types from `src/main.rs` into `src/runner.rs` and update call sites (blocked by #1) (A)
- [x] (#3) Move git helpers, output helpers, and tail follow utilities from `src/main.rs` into `src/git.rs`, `src/output.rs`, `src/tail.rs`; update call sites (blocked by #1) (A)
- [ ] (#4) Reduce `src/main.rs` to a thin CLI dispatcher using new modules; ensure compile success (blocked by #2, #3)

## Config module split

- [x] (#5) Create `src/config` module with `mod.rs`, `types.rs`, `cli.rs`, `env.rs`, `toml.rs`, `tests.rs`; move code and preserve public exports (B)
- [x] (#6) Verify `Config::load` and `parse_args` behavior remains identical after split; update imports as needed (blocked by #5) (B)

## TUI module split

- [x] (#7) Create `src/tui` module with `app.rs`, `message.rs`, `render.rs`, `ansi.rs`, `process.rs`, `tail.rs`, `run.rs`, and `mod.rs`; move code preserving behavior (C)
- [x] (#8) Rewire `run_tui` entrypoint and ensure key handling and subprocess behavior are unchanged (blocked by #7) (A)

## Preventive splits for near-threshold files

- [x] (#9) Split `src/planning.rs` into `src/planning/{assign.rs,review.rs,prd.rs,parse.rs,mod.rs}` with re-exports and no logic changes (C)
- [x] (#10) Split `src/engine.rs` into `src/engine/{mod.rs,claude.rs,codex.rs,stub.rs,util.rs}` with re-exports and no logic changes (A)
- [x] (#11) Split `src/worktree.rs` into `src/worktree/{git.rs,create.rs,cleanup.rs,list.rs}` with re-exports and no logic changes (B)
- [x] (#12) Split `src/task.rs` into `src/task/{model.rs,parse.rs,assign.rs,tests.rs}` with re-exports and no logic changes (B)
- [x] (#13) Split `src/team.rs` into `src/team/{team.rs,assignments.rs,sprint_history.rs}` with re-exports and no logic changes (A)
- [x] (#14) Resolve visibility/import issues across new modules and remove unused imports (blocked by #9, #10, #11, #12, #13) (B)

## Validation

- [ ] (#15) Run `cargo test --lib --tests` and fix any failures (blocked by #4, #6, #8, #14)
- [ ] (#16) Verify no Rust source file exceeds 1000 LOC and adjust splits if any remain (blocked by #4, #6, #8, #14)

## Follow-up tasks (from sprint review)
- [x] Fix TASKS.md accuracy: #7 and #9 are marked done but `src/tui.rs` and `src/planning.rs` are still monolithic (no `src/tui/` or `src/planning/` dirs). (B)
