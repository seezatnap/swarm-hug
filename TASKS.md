# Tasks

## Process & Docs (requirements from PROMPT.md)
- [x] Keep README.md accurate, accessible, and friendly after each session (and note ../ralph-bash-v2 is legacy).
- [x] Maintain TASKS.md coverage for all PROMPT/SPECS requirements.
- [x] Use ../ralph-bash-v2 only as a behavior reference (not a source of truth).
- [x] Check in work after each completed task (one task per commit).
- [x] Batch up to 3 related tasks only when session rules allow (AGENTS currently limits to 1).

## Tests (must be implemented first for product behavior)
- [x] Write integration test harness that runs `swarm` in a temp git repo with stub engine and `--max-sprints`, verifying TASKS.md completion, CHAT.md plan/completion entries, worktree create/cleanup, and stub output files; ensure deterministic/no-network behavior.

## CLI and Entrypoints
- [x] Define CLI interface and dispatch for `swarm` (init/run/sprint/plan/status/agents/worktrees/worktrees-branch/cleanup/merge/tail).
- [x] Implement default `swarm` behavior (no subcommand == `swarm run` and tail CHAT.md unless `--no-tail`).
- [x] Implement `swarm init` (create default config, TASKS.md, CHAT.md, log dir).
- [x] Implement `swarm run` (run sprints until done or max-sprints reached).
- [x] Implement `swarm sprint` (run exactly one sprint).
- [x] Implement `swarm plan` (sprint planning only).
- [x] Implement `swarm status` (task counts + recent chat lines).
- [x] Implement `swarm agents` (list agent names/initials).
- [ ] Implement `swarm worktrees` and `swarm worktrees-branch` commands.
- [ ] Implement `swarm cleanup` (remove worktrees/branches).
- [ ] Implement `swarm merge` (merge agent branches).
- [x] Implement `swarm tail` (stream CHAT.md).

## Configuration
- [x] Implement configuration loading (swarm.toml + CLI flags + env vars) with precedence.
- [x] Support required config keys: agents.max_count, agents.tasks_per_agent, files.tasks, files.chat, files.log_dir, engine.type, engine.stub_mode, sprints.max.

## Sprint Planning and Assignment
- [x] Integrate sprint planning into `swarm plan/run/sprint` (assign tasks, write TASKS.md, and post CHAT.md summary).
- [ ] Commit assignment changes to git so worktrees see updates.
- [ ] Add optional LLM-assisted planning via engine layer.

## Adaptive Agent Spawning
- [x] Implement adaptive agent spawning based on unblocked task count and agents.max_count cap.
- [x] Spawn agents only for the number of tasks that can be assigned this sprint.

## Sprint Limits
- [x] Support hard sprint cap via config/flag (--max-sprints).
- [x] Stop cleanly when the limit is reached, leaving remaining tasks unassigned.

## Agent Execution Rules
- [ ] Implement per-agent lifecycle tracking (assigned -> working -> done -> terminated).
- [ ] Enforce one task = one commit rule per agent.
- [ ] Ensure agents only work on assigned tasks.

## Chat and Tail UI
- [x] Integrate CHAT.md tailing into `swarm run` with `--no-tail` flag.

## Engine Abstraction
- [x] Implement engine abstraction layer (swappable for tests vs production).
- [x] Support `claude` CLI engine.
- [x] Support `codex` CLI engine.
- [x] Implement stub engine for tests/offline (no network calls).
- [x] Stub engine writes deterministic output files (e.g., loop/turn1-agentA.md with OK).

## Git Worktree Workflow
- [ ] Implement worktree management under worktrees/agent-<initial>-<name>.
- [ ] Implement per-agent branch naming (agent/<name>).
- [ ] Create worktrees before agents run and clean up after merge.
- [ ] Implement merge workflow (agents merge branch back to main).
- [ ] Surface merge conflicts in CHAT.md and report sprint failure without crashing.
- [ ] Surface merge conflicts to CHAT.md; report failure but do not crash runner.

## Logs
- [ ] Implement per-agent log files under loop/agent-<initial>.log.
- [ ] Implement log rotation when size exceeds threshold.

## init.sh (Lima VM Bootstrap)
- [ ] Create init.sh that provisions Lima VM with Docker.
- [ ] Install git, bash, Rust toolchain, and required CLIs (claude/codex) in VM.
- [ ] Mount repo into VM and expose `swarm` on PATH inside container.
- [ ] Ensure no GridTUI in init.sh.

## Constraints & Compliance
- [ ] Ensure ASCII-only output in files (unless file already uses Unicode).
- [ ] Ensure no GridTUI integration or dependencies.
- [x] Ensure behavior is deterministic under stubbed engine for tests.

## Completed
- [x] Implement task file parser for checklist format.
- [x] Implement task file writer preserving format and backlog order.
- [x] Implement A-Z agent name/initial mapping matching ralph-bash-v2.
- [x] Implement CHAT.md writer and parser with required format.
- [x] Implement blocked-task detection.
- [x] Implement algorithmic sprint assignment (per-agent task cap, backlog order).
- [x] Write unit tests for task parsing and state transitions.
- [x] Write unit tests for agent naming (A-Z mapping).
- [x] Write unit tests for chat formatting.
