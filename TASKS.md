# Tasks

## Current Priorities

### Lima VM Bootstrap
- [x] Create `init.sh` for Lima VM bootstrap: install Docker, git, bash, Rust toolchain, claude/codex CLIs; mount repo; expose `swarm`; document usage in README

### LLM-Assisted Planning (Optional)
- [ ] Add LLM-assisted sprint planning support (engine hook + scrum master prompts)

---

## Completed

### Per-Agent Logging with Rotation
- [x] Create `src/log.rs` module for per-agent logging
- [x] Implement log rotation (line-based, max 1000 lines per file)
- [x] Write agent logs to `loop/agent-<initial>.log`
- [x] Add tests for log module (12 tests)
- [x] Integrate logging into agent execution in main.rs

### Worktree Cleanup After Merge
- [x] Add automatic worktree cleanup after successful merges
- [x] Update `cmd_merge` to clean up worktrees and branches on success

### Goals
- [x] Rebuild bash orchestration in Rust with multi-agent sprints and file-based workflow
- [x] Start with a comprehensive, stub-first test suite for end-to-end behavior
- [x] Preserve core behavior (TASKS.md, CHAT.md, git worktrees, merge flow) without GridTUI
- [x] Support sprint limits for deterministic runs
- [x] Provide multi-team support with isolated artifacts

### CLI and Entrypoints
- [x] Implement `swarm` binary with default command = `run`
- [x] Implement commands: init, run, sprint, plan, status, agents, teams, team init, worktrees, worktrees-branch, cleanup, merge, tail
- [x] Implement `swarm init` to create `.swarm-hug/` root, assignments, and default config
- [x] Ensure `swarm init` creates default tasks/chat files and log directories
- [x] Implement `swarm status` to show task counts and recent chat lines
- [x] Implement `swarm worktrees` and `swarm worktrees-branch`
- [x] Implement `swarm cleanup` to remove worktrees
- [x] Implement `swarm merge` with conflict reporting to CHAT.md

### Configuration
- [x] Config file + CLI flags + env vars with precedence
- [x] Support required config keys (agents, files, engine, sprints)
- [x] Apply team-based path defaults under `.swarm-hug/<team>/`
- [x] Support env var overrides for agents/files/engine/sprints

### Multi-Team Architecture
- [x] Store all artifacts under `.swarm-hug/`
- [x] Team directories include tasks.md, chat.md, specs.md, prompt.md, loop/, worktrees/
- [x] Track agent assignments in `.swarm-hug/assignments.toml`
- [x] Enforce exclusive agent assignment per team
- [x] Implement `swarm teams` and `swarm team init <name>`

### Task File Format
- [x] Preserve checklist format with unassigned/assigned/completed states
- [x] Map checkbox initials to canonical agent names
- [x] Keep backlog ordering top-to-bottom

### Sprint Planning and Assignment
- [x] Algorithmic sprint assignment with tasks-per-agent
- [x] Commit assignment changes so worktrees see them
- [x] Write sprint plan summary to CHAT.md
- [x] Respect blocked markers (BLOCKED/blocked/Blocked by:)

### Adaptive Agent Spawning
- [x] Spawn agents based on available assignable tasks
- [x] Cap agent count with `agents.max_count`

### Sprint Limits
- [x] Support `--max-sprints` and config `sprints.max`
- [x] Stop cleanly when sprint cap reached

### Agent Execution Rules
- [x] Run each agent inside its own git worktree
- [x] Enforce one task = one commit per agent
- [x] Track lifecycle states (assigned -> working -> done -> terminated)
- [x] Agents operate only on assigned tasks

### Chat and UI
- [x] Append all communication to CHAT.md with required format
- [x] Provide `swarm tail` command that follows appended lines
- [x] Stream chat output during `swarm run` by default unless `--no-tail` is set

### Engine Abstraction
- [x] Support claude/codex/stub engines
- [x] Stub engine writes deterministic output files per invocation
- [x] Engine layer is swappable for tests vs production

### Git Worktree Workflow
- [x] Create real git worktrees under team worktrees dir
- [x] Use per-agent branches (agent/<name>)
- [x] Surface merge conflicts and report sprint failures without crashing

### Logs
- [x] Stub engine writes outputs to `loop/`
- [x] Per-agent log files under `loop/agent-<initial>.log`
- [x] Log rotation when files exceed 1000 lines

### Tests
- [x] Unit tests for tasks, agents, chat formatting, lifecycle, logging
- [x] Integration test harness for stubbed engine runs
- [x] Tests run without network access

### Workflow Requirements (Ongoing)
- [x] Maintain exhaustive TASKS.md coverage for PROMPT/SPECS
- [x] Keep README.md accurate, accessible, and friendly after each session
- [x] Use ../ralph-bash-v2 as reference only (legacy, fragile)
- [x] Run tests after each iteration and fix issues
- [x] Check in work after each completed task
- [x] Avoid GridTUI dependencies
- [x] Prefer batching up to 3 related tasks per session when allowed by session rules

### Repo Hygiene
- [x] Checked src/tests for >1000 LOC files; none found
