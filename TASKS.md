# Tasks

## Current Priorities
- [ ] Integrate tailing into `swarm run` by default unless `--no-tail` is set
- [ ] Add per-agent log files under loop/agent-<initial>.log with rotation
- [ ] Create Lima VM bootstrap script init.sh (Docker, git, bash, Rust, claude/codex; no GridTUI)
- [ ] Add optional LLM-assisted planning via engine layer
- [ ] Clean up worktrees automatically after merge

---

## Completed

### Agent Execution (just completed)
- [x] Run agent execution inside each agent worktree (pass worktree path to engine)
- [x] Add per-agent lifecycle tracking (assigned -> working -> done -> terminated)
- [x] Enforce one task = one commit rule per agent (commits in worktree with agent attribution)
- [x] Ensure agents only work on assigned tasks

### Multi-Team Architecture
- [x] Create `.swarm-hug/` as root for all team config and artifacts
- [x] Add `--team <name>` CLI flag to specify which team context to use
- [x] Update config to resolve paths relative to `.swarm-hug/<team>/`
- [x] Each team gets its own `specs.md`, `prompt.md`, `tasks.md` in `.swarm-hug/<team>/`
- [x] Each team gets its own `loop/` directory in `.swarm-hug/<team>/loop/`
- [x] Each team gets its own `worktrees/` directory in `.swarm-hug/<team>/worktrees/`
- [x] Team's `chat.md` lives in `.swarm-hug/<team>/chat.md`
- [x] Create `.swarm-hug/assignments.toml` to track agent-to-team assignments
- [x] Implement agent assignment logic (alphabetical: Aaron, Betty, Carlos, etc.)
- [x] Ensure agents cannot be assigned to multiple teams simultaneously
- [x] Add `swarm teams` command to list active teams and their assigned agents
- [x] Add `swarm team init <name>` command to initialize a new team
- [x] Update `swarm init` to create `.swarm-hug/` structure
- [x] Update `swarm run` to use `--team` for team-specific paths
- [x] Update `swarm status` to show team-specific status
- [x] Update `swarm cleanup` to clean team-specific artifacts

### Core CLI + Engine
- [x] Implement CLI interface and dispatch
- [x] Implement default `swarm` behavior (defaults to `run`)
- [x] Implement `swarm init`
- [x] Implement `swarm run`
- [x] Implement `swarm sprint`
- [x] Implement `swarm plan`
- [x] Implement `swarm status`
- [x] Implement `swarm agents`
- [x] Implement `swarm teams`
- [x] Implement `swarm team init`
- [x] Implement `swarm tail`
- [x] Implement `swarm worktrees` command (lists team worktrees dir)
- [x] Implement `swarm worktrees-branch` command (list git branches)
- [x] Implement `swarm cleanup` command
- [x] Implement `swarm merge`
- [x] Implement configuration loading (config + CLI flags + env vars)
- [x] Support required config keys
- [x] Implement engine abstraction layer
- [x] Support claude/codex/stub engines
- [x] Stub engine writes deterministic output files

### Planning + Tasks
- [x] Implement task file parser/writer (TASKS.md format)
- [x] Implement blocked-task detection
- [x] Implement algorithmic sprint assignment
- [x] Commit assignment changes to git so worktrees see updates
- [x] Write sprint plan summary to CHAT.md
- [x] Implement adaptive agent spawning
- [x] Support hard sprint cap

### Git Workflow + Merge
- [x] Implement per-agent branch naming (agent/<name>)
- [x] Implement real git worktree management (not placeholder dirs)
- [x] Implement merge workflow (agents merge branch back to main)
- [x] Surface merge conflicts in CHAT.md and report sprint failure without crashing

### Tests
- [x] Write unit tests for core modules
- [x] Write integration test harness for stubbed engine runs

### Workflow Requirements (ongoing)
- [x] Maintain TASKS.md coverage for all PROMPT/SPECS requirements
- [x] Keep README.md accurate, accessible, and friendly after each session
- [x] Use ../ralph-bash-v2 only as a behavior reference (legacy)
- [x] Check in work after each completed task (one task per commit)
- [x] Run tests after every iteration and fix issues
- [x] Keep outputs ASCII-only unless a file already uses Unicode
- [x] Avoid GridTUI integration or dependencies
