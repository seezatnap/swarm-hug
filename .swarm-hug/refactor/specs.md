# Specifications: refactor

# PRD: Rust refactor sweep (no functional changes)

## Summary
Perform a refactor-only sweep of the Rust codebase to improve structure and maintainability while keeping behavior identical. The primary constraint is to eliminate any Rust source file exceeding 1000 LOC and reduce near-threshold files by splitting into focused modules. No functional or behavioral changes are permitted.

## Goals
- Reduce all Rust source files to <= 1000 LOC.
- Improve code organization, readability, and separation of concerns.
- Keep public API and runtime behavior unchanged.
- Keep tests passing without modification to expected outputs.

## Non-goals
- No new features.
- No behavioral changes.
- No dependency additions unless strictly required (avoid if possible).
- No changes to runtime configuration or CLI interface.

## Current findings (LOC hot spots)
Files over 1000 LOC:
- src/main.rs (1919 LOC)
- src/config.rs (1073 LOC)
- src/tui.rs (1018 LOC)

Near-threshold files likely to exceed 1000 LOC soon:
- src/planning.rs (927 LOC)
- src/engine.rs (874 LOC)
- src/worktree.rs (899 LOC)
- src/task.rs (860 LOC)
- src/team.rs (831 LOC)

## Constraints
- Functional equivalence is mandatory.
- ASCII-only output in files unless a file already uses Unicode (this PRD is ASCII).
- Do not modify lockfiles unless absolutely necessary.
- Do not introduce new dependencies unless required by the refactor (avoid). 

## Proposed refactor plan (module splits)

### 1) src/main.rs -> src/commands/* + src/runner.rs + src/git.rs + src/output.rs + src/tail.rs
Rationale: main.rs currently mixes CLI parsing, command dispatch, sprint orchestration, git commit helpers, IO/tailing, and formatting.

Proposed structure:
- src/commands/mod.rs: re-exports per-command handlers.
- src/commands/init.rs: cmd_init + init_default_files + ensure_parent_dir.
- src/commands/run.rs: cmd_run + cmd_run_tui + cmd_sprint + cmd_plan + run_sprint entrypoint wiring.
- src/commands/status.rs: cmd_status.
- src/commands/agents.rs: cmd_agents.
- src/commands/worktrees.rs: cmd_worktrees + cmd_worktrees_branch + cmd_cleanup.
- src/commands/projects.rs: cmd_projects + cmd_project_init.
- src/commands/misc.rs: cmd_customize_prompts + cmd_set_email.
- src/runner.rs: run_sprint + SprintResult + post-sprint review and lifecycle orchestration.
- src/git.rs: commit_files + commit_task_assignments + commit_sprint_completion + get_current_commit + get_git_log_range.
- src/output.rs: print_sprint_start_banner + print_team_status_banner + format_duration.
- src/tail.rs: tail_follow.

Notes:
- Keep public function signatures intact where used elsewhere.
- main.rs should become a thin CLI dispatcher only.

### 2) src/config.rs -> src/config/*
Rationale: config.rs bundles EngineType, Config, CLI parsing, TOML parsing, env application, and tests.

Proposed structure:
- src/config/mod.rs: public types + re-exports.
- src/config/types.rs: EngineType, Config, defaults.
- src/config/cli.rs: CliArgs + parse_args.
- src/config/env.rs: apply_env.
- src/config/toml.rs: load_from_file + parse_toml helpers.
- src/config/tests.rs: tests moved out of main module.

Notes:
- Maintain current parse_args behavior and defaults.
- Keep Config::load signature and semantics unchanged.

### 3) src/tui.rs -> src/tui/*
Rationale: tui.rs mixes app state, rendering, ANSI parsing, subprocess management, and tailing.

Proposed structure:
- src/tui/mod.rs: public exports.
- src/tui/app.rs: TuiApp, InputMode, state handling.
- src/tui/message.rs: TuiMessage definition.
- src/tui/render.rs: draw_ui + draw_header + draw_content + draw_search_bar + draw_quit_modal.
- src/tui/ansi.rs: strip_ansi + parse SGR + highlight helpers.
- src/tui/process.rs: run_tui_with_subprocess + kill_process_tree.
- src/tui/tail.rs: tail_chat_to_tui.
- src/tui/run.rs: run_tui entrypoint.

Notes:
- Preserve terminal behavior and key handling exactly.
- Preserve subprocess stdout/stderr suppression and process group behavior.

### 4) Prevent future 1000 LOC: split near-threshold modules
These are not strictly required to reach the immediate <=1000 LOC requirement, but recommended to keep headroom.

- src/planning.rs -> src/planning/*
  - assign.rs: generate_scrum_master_prompt, parse_llm_assignments, stub_assignment.
  - review.rs: generate_review_prompt, parse_review_response, run_sprint_review.
  - prd.rs: generate_prd_prompt, parse_prd_response, convert_prd_to_tasks, stub_prd_conversion.
  - parse.rs: JSON-ish parsing helpers (find_matching_brace, parse_number_at, etc.).
  - mod.rs: public exports.

- src/engine.rs -> src/engine/*
  - mod.rs: Engine trait, EngineResult, create_engine.
  - claude.rs: ClaudeEngine.
  - codex.rs: CodexEngine.
  - stub.rs: StubEngine.
  - util.rs: resolve_cli_path, read_coauthor_email, generate_coauthor_line, output_to_result.

- src/worktree.rs -> src/worktree/*
  - git.rs: git_repo_root, registered_worktrees, parse_worktrees_with_branch.
  - create.rs: create_worktrees_in, worktree_path, worktree_is_registered.
  - cleanup.rs: cleanup_worktrees_in, cleanup_agent_worktrees, remove_worktree_by_path, delete_branch helpers.
  - list.rs: list_worktrees, list_agent_branches.

- src/task.rs -> src/task/*
  - model.rs: TaskStatus, Task, TaskList.
  - parse.rs: parse_task_line and parsing helpers.
  - assign.rs: assign_sprint, is_task_assignable, blocking logic.
  - tests.rs: task tests.

- src/team.rs -> src/team/*
  - team.rs: Team struct + path helpers.
  - assignments.rs: Assignments struct.
  - sprint_history.rs: SprintHistory struct.

## Execution plan (incremental, no behavior change)
Phase 1: Split main.rs
- Move helpers to new modules and rewire main.rs imports.
- Ensure all commands still compile and behave identically.

Phase 2: Split config.rs and tui.rs
- Create new modules and re-export from mod.rs.
- Keep parsing and CLI behavior identical.

Phase 3: Optional preventive splits
- planning.rs, engine.rs, worktree.rs, task.rs, team.rs.
- Focus only on module boundaries and visibility, no logic changes.

Phase 4: Cleanup
- Remove unused imports after splits.
- Run formatting if required by repo norms (avoid if not requested).

## Acceptance criteria
- No Rust source file exceeds 1000 LOC.
- All tests pass: `cargo test --lib --tests`.
- No changes to CLI output content or format (including emoji/color sequences).
- No changes to on-disk file formats or paths.
- No dependency additions.

## Risks
- Accidental behavior change while moving code (mitigate via minimal edits and tests).
- Visibility/export mistakes that break cross-module use (mitigate via compile/test cycle).

## Success metrics
- LOC reductions per file meet threshold.
- Stable test results and identical CLI behavior for a sample run.

## Open questions
- Should the refactor also enforce a hard <= 900 LOC target for near-threshold files to reduce churn?
- Is it acceptable to add a lightweight internal module for git command helpers used in multiple files?

