# Tasks

## P0 - Test strategy (must be first)
- [x] Update the integration test to use `.swarm-hug/<team>/` layout with the stub engine (verify TASKS completion, CHAT plan + completion lines, stub outputs under team loop/, worktrees created + cleaned, and max-sprints behavior).
- [x] Unit tests cover task parsing/blocking/assignment.
- [x] Unit tests cover agent name/initial mapping.
- [x] Unit tests cover chat formatting/parsing.
- [x] Unit tests cover lifecycle transitions.
- [x] Unit tests cover engine stub determinism/selection.
- [x] Unit tests cover config/env/CLI parsing.

## P1 - Core CLI/config behavior
- [x] Config supports required values (agents/files/engine/sprints/planning) with CLI/env overrides and team path resolution.
- [x] Move default (non-team) files under `.swarm-hug/` (no root TASKS.md/CHAT.md) and update config defaults, `swarm init`, docs, and tests accordingly.
- [x] Ensure `swarm cleanup` removes agent branches as well as worktrees.
- [x] Confirm `swarm status` reports task counts and recent chat lines.
- [x] Enforce exclusive agent assignment per team using `.swarm-hug/assignments.toml` during planning/spawn and release on cleanup/merge.
- [x] CLI supports init/run/sprint/plan/status/agents/worktrees/worktrees-branch/cleanup/merge/tail/teams/team init.
- [x] Default command is `run` and tails chat unless `--no-tail` is set.

## P1 - Multi-team layout
- [x] Team init scaffolds required files under `.swarm-hug/<team>/` (tasks/chat/specs/prompt/loop/worktrees).
- [x] `--team/-t` resolves files to `.swarm-hug/<team>/`.
- [x] `.swarm-hug/assignments.toml` exists and can list agent assignments.
- [x] `swarm teams` lists teams and available agents.

## P2 - Sprint planning/execution
- [ ] Confirm merge conflicts are recorded in CHAT.md and do not crash the runner (review merge flow end-to-end).
- [ ] Ensure sprint planning writes a concise sprint plan summary to CHAT.md.
- [ ] Confirm LLM-assisted planning (when enabled) falls back to algorithmic assignment on failure.
- [ ] Ensure task assignment respects top-to-bottom TASKS.md order (backlog priority).
- [ ] Ensure agent prompts/rules enforce assigned-tasks-only and one-task-per-commit behavior.
- [ ] Verify max-sprints stops cleanly and leaves remaining tasks unassigned.
- [x] Task format supports unassigned/assigned/completed with blocked detection.
- [x] Adaptive agent spawning is based on assignable tasks.
- [x] Sprint planning commits task assignment changes to git.

## P2 - Worktrees/merge/logging
- [x] Worktrees are created under `.swarm-hug/<team>/worktrees` with `agent/<name>` branches.
- [x] Merge cleans up worktrees/branches after success and reports conflicts to CHAT.md.
- [x] Per-agent logs with rotation live under the team loop dir.

## P2 - Engine abstraction
- [x] Engines support claude/codex/stub and stub is deterministic/offline.

## P2 - init.sh
- [x] Lima bootstrap script installs deps, mounts repo RW, and exposes `swarm`.

## P3 - Docs/process
- [ ] Keep README accurate, accessible, and friendly after each change (update paths/flags/test gates as behavior evolves).
- [x] README notes ralph-bash-v2 is a legacy reference only.
- [ ] Run tests after each task and fix failures (fast gate: `cargo test --lib --tests`).
- [ ] Check in work after each completed task (commit once tests pass).
- [ ] BLOCKED: Resolve prompt request to do up to 3 related tasks per session vs AGENTS one-task-per-session rule.

## Maintenance
- [ ] Split `src/main.rs` (1099 LOC) into smaller modules.
