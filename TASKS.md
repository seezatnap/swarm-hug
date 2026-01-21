# Tasks

## CLI and Entrypoints
- [ ] Define CLI interface for `swarm` (init/run/sprint/plan/status/agents/worktrees/worktrees-branch/cleanup/merge/tail)
- [ ] Implement default `swarm` behavior (runs like `swarm run` and tails CHAT.md unless `--no-tail`)
- [ ] Implement `swarm init` to create default config, TASKS.md, CHAT.md, log dir
- [ ] Implement `swarm run` (run sprints until done or max-sprints reached)
- [ ] Implement `swarm sprint` (run exactly one sprint)
- [ ] Implement `swarm plan` (sprint planning only)
- [ ] Implement `swarm status` (task counts + recent chat lines)
- [ ] Implement `swarm agents` (list agent names/initials)
- [ ] Implement `swarm worktrees` and `swarm worktrees-branch` commands
- [ ] Implement `swarm cleanup` (remove worktrees/branches)
- [ ] Implement `swarm merge` (merge agent branches)
- [ ] Implement `swarm tail` (stream CHAT.md)

## Configuration
- [ ] Implement configuration loading (swarm.toml + CLI flags + env vars) with precedence
- [ ] Support required config keys: agents.max_count, agents.tasks_per_agent, files.tasks, files.chat, files.log_dir, engine.type, engine.stub_mode, sprints.max

## Task File Format (TASKS.md)
- [ ] Implement task file parser for checklist format (unassigned `- [ ]`, assigned `- [A]`, completed `- [x] ... (A)`)
- [ ] Implement task file writer preserving format and backlog order (top to bottom priority)

## Agent Names and Initials
- [ ] Implement A-Z agent name/initial mapping matching ralph-bash-v2 (Aaron, Betty, Carlos, ... Zane)

## CHAT.md and UI
- [ ] Implement CHAT.md writer with required format: `YYYY-MM-DD HH:MM:SS | <AgentName> | AGENT_THINK: <message>`
- [ ] Integrate CHAT.md tailing into `swarm run` with `--no-tail` flag to disable

## Sprint Planning and Assignment
- [ ] Implement sprint planning (algorithmic) to assign up to N tasks per agent respecting backlog order
- [ ] Implement optional LLM-assisted planning via engine layer
- [ ] Commit assignment changes to git so worktrees see the changes
- [ ] Post sprint plan summary to CHAT.md from scrum master

## Adaptive Agent Spawning
- [ ] Implement adaptive agent spawning (agents.max_count is a cap, not fixed spawn count)
- [ ] Implement blocked-task detection (recognize BLOCKED, blocked, "Blocked by:")
- [ ] Spawn agents only for the number of tasks that can be assigned this sprint

## Sprint Limits
- [ ] Support hard sprint cap via config/flag (--max-sprints)
- [ ] Stop cleanly when limit reached, leaving remaining tasks unassigned

## Agent Execution Rules
- [ ] Implement per-agent lifecycle tracking (assigned -> working -> done -> terminated)
- [ ] Enforce one task = one commit rule per agent
- [ ] Agents work only on assigned tasks

## Git Worktree Workflow
- [ ] Implement worktree management under worktrees/agent-<initial>-<name>
- [ ] Implement per-agent branch naming (agent/<name>)
- [ ] Create worktrees before agents run, clean up after merge
- [ ] Implement merge workflow (agents merge branch back to main)
- [ ] Surface merge conflicts to CHAT.md; report failure but do not crash runner

## Engine Abstraction
- [ ] Implement engine abstraction layer (swappable for tests vs production)
- [ ] Support `claude` CLI as engine
- [ ] Support `codex` CLI as engine
- [ ] Implement stub engine for tests/offline (no network calls)
- [ ] Stub engine writes deterministic output files (e.g., loop/turn1-agentA.md with OK)

## Logs
- [ ] Implement per-agent log files under loop/agent-<initial>.log
- [ ] Implement log rotation when size exceeds threshold

## init.sh (Lima VM Bootstrap)
- [ ] Create init.sh that provisions Lima VM with Docker
- [ ] Install git, bash, Rust toolchain, and required CLIs (claude/codex) in VM
- [ ] Mount repo into VM and expose `swarm` on PATH inside container
- [ ] Ensure no GridTUI in init.sh

## Tests
- [ ] Write unit tests for task parsing and state transitions
- [ ] Write unit tests for agent naming (A-Z mapping)
- [ ] Write unit tests for chat formatting
- [ ] Write integration test: create temp git repo, write TASKS.md/CHAT.md, run stubbed sprint(s)
- [ ] Integration test verifies: TASKS.md transitions to completed, CHAT.md has plan/completion entries
- [ ] Integration test verifies: worktrees created and cleaned up, stub output files exist
- [ ] Ensure all tests run without network access
- [ ] Ensure behavior is deterministic under stubbed engine

## Constraints
- [ ] Ensure ASCII-only output in files (unless file already uses Unicode)
- [ ] Ensure no GridTUI integration or dependencies
- [ ] Keep dependencies minimal (prefer standard library)
