# Tasks

## Current Priority
- [x] Make `swarm init` create default TASKS.md, CHAT.md, and log dir when no team is specified (match ralph-bash-v2 init behavior).

## Spec Coverage (Done)
- [x] Rust rewrite with multi-agent sprints, file-based workflow, and no GridTUI.
- [x] CLI command suite: init, run (default), sprint, plan, status, agents, teams, team init, worktrees, worktrees-branch, cleanup, merge, tail.
- [x] Default command is `run` and tails chat unless `--no-tail` is set.
- [x] Config file + CLI flags + env var overrides (agents/files/engine/sprints/planning).
- [x] Multi-team architecture under `.swarm-hug/<team>/` with isolated tasks/chat/specs/prompt/loop/worktrees.
- [x] Assignments tracked in `.swarm-hug/assignments.toml` with exclusive agent assignment.
- [x] Task file format: unassigned/assigned/completed checkboxes with initials.
- [x] Fixed agent name/initial mapping A-Z.
- [x] Sprint planning and assignment (algorithmic + optional LLM) with sprint plan summary in chat.
- [x] Adaptive agent spawning based on assignable tasks and max agents cap.
- [x] Sprint limits via config/flags for deterministic runs.
- [x] Agent lifecycle enforcement: one task per commit in per-agent worktrees.
- [x] Tail-based UI that streams chat.md (`swarm tail`).
- [x] Engine abstraction (claude/codex/stub) with deterministic stub outputs.
- [x] Git worktree workflow: create/list/cleanup/merge with conflict reporting.
- [x] Per-agent logging with rotation under loop/.
- [x] Lima VM bootstrap script (`init.sh`) that provisions Docker+Rust and exposes `swarm`.
- [x] Test suite: unit tests + integration test with stub engine and no network.
- [x] Constraints honored: ASCII-only edits, minimal deps, deterministic stub behavior.
- [x] Workflow requirements tracked: README kept accurate, tests run each session, work committed; ralph-bash-v2 used as legacy reference; >1000 LOC files flagged (none found).
