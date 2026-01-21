# Tasks

## Multi-Team Architecture Refactor (from PROMPT.md)

### Core Directory Structure
- [x] Create `.swarm-hug/` as root for all team config and artifacts
- [x] Add `--team <name>` CLI flag to specify which team context to use
- [x] Update Config to resolve paths relative to `.swarm-hug/<team>/`

### Team Isolation
- [x] Each team gets its own `specs.md`, `prompt.md`, `tasks.md` in `.swarm-hug/<team>/`
- [x] Each team gets its own `loop/` directory in `.swarm-hug/<team>/loop/`
- [x] Each team gets its own `worktrees/` directory in `.swarm-hug/<team>/worktrees/`
- [x] Team's `chat.md` lives in `.swarm-hug/<team>/chat.md`

### Agent Assignment System
- [x] Create `.swarm-hug/assignments.toml` to track agent-to-team assignments
- [x] Implement agent assignment logic (alphabetical: aaron, betty, carlos, etc.)
- [x] Ensure agents cannot be assigned to multiple teams simultaneously
- [x] Add `swarm teams` command to list active teams and their assigned agents
- [x] Add `swarm team init <name>` command to initialize a new team

### CLI Updates for Multi-Team
- [x] Update `swarm init` to create `.swarm-hug/` structure
- [x] Update `swarm run` to use `--team` for team-specific paths
- [x] Update `swarm status` to show team-specific status
- [x] Update `swarm cleanup` to clean team-specific artifacts

---

## Remaining Tasks from SPECS.md

### CLI and Entrypoints
- [x] Implement `swarm worktrees` command (lists worktrees in team's worktrees dir)
- [ ] Implement `swarm worktrees-branch` command (list git branches)
- [ ] Implement `swarm merge` (merge agent branches)

### Sprint Planning and Assignment
- [ ] Commit assignment changes to git so worktrees see updates
- [ ] Add optional LLM-assisted planning via engine layer

### Agent Execution Rules
- [ ] Implement per-agent lifecycle tracking (assigned -> working -> done -> terminated)
- [ ] Enforce one task = one commit rule per agent
- [ ] Ensure agents only work on assigned tasks

### Git Worktree Workflow
- [ ] Implement real git worktree management under worktrees/agent-<initial>-<name>
- [ ] Implement per-agent branch naming (agent/<name>)
- [ ] Create worktrees before agents run and clean up after merge
- [ ] Implement merge workflow (agents merge branch back to main)
- [ ] Surface merge conflicts in CHAT.md and report sprint failure without crashing

### Logs
- [ ] Implement per-agent log files under loop/agent-<initial>.log
- [ ] Implement log rotation when size exceeds threshold

### init.sh (Lima VM Bootstrap)
- [ ] Create init.sh that provisions Lima VM with Docker
- [ ] Install git, bash, Rust toolchain, and required CLIs (claude/codex) in VM
- [ ] Mount repo into VM and expose `swarm` on PATH inside container
- [ ] Ensure no GridTUI in init.sh

### Constraints & Compliance
- [ ] Ensure ASCII-only output in files (unless file already uses Unicode)
- [ ] Ensure no GridTUI integration or dependencies

---

## Completed
- [x] Keep README.md accurate, accessible, and friendly after each session
- [x] Maintain TASKS.md coverage for all PROMPT/SPECS requirements
- [x] Use ../ralph-bash-v2 only as a behavior reference (not a source of truth)
- [x] Check in work after each completed task (one task per commit)
- [x] Batch up to 3 related tasks only when session rules allow
- [x] Write integration test harness
- [x] Define CLI interface and dispatch
- [x] Implement default `swarm` behavior
- [x] Implement `swarm init`
- [x] Implement `swarm run`
- [x] Implement `swarm sprint`
- [x] Implement `swarm plan`
- [x] Implement `swarm status`
- [x] Implement `swarm agents`
- [x] Implement `swarm tail`
- [x] Implement configuration loading
- [x] Support required config keys
- [x] Integrate sprint planning
- [x] Implement adaptive agent spawning
- [x] Support hard sprint cap
- [x] Implement engine abstraction layer
- [x] Support claude/codex/stub engines
- [x] Stub engine writes deterministic output files
- [x] Implement task file parser/writer
- [x] Implement A-Z agent naming
- [x] Implement CHAT.md writer and parser
- [x] Implement blocked-task detection
- [x] Implement algorithmic sprint assignment
- [x] Write unit tests for core modules
