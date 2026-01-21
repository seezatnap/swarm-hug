# Specifications: swarm-hug

Rust rewrite of ../ralph-bash-v2: a multi-agent, sprint-based orchestration system that is robust, minimal, and deeply tested.

## Goals
- Rebuild the bash-based agent orchestration in Rust for correctness and maintainability.
- Start with a comprehensive test suite that validates end-to-end behavior, using stubbed engines by default.
- Preserve the core behavior of the current system (multi-agent sprints, tasks.md, chat.md, git worktrees, merge flow) while removing GridTUI.
- Provide a simple, tail-based UI that streams chat.md to the operator.
- Support limiting the number of sprints for deterministic tests and short runs.
- Provide a VM bootstrap script (init.sh) similar to ralph-bash-v2 that provisions a Lima VM and exposes the `swarm` command inside it.
- **Multi-team support**: Multiple teams can work on the same repo simultaneously with isolated artifacts.

## Non-goals
- GridTUI or any terminal UI beyond tailing CHAT.md.
- Introducing heavy dependencies or complex services (no DB, no web UI).
- Changing the file-based workflow (TASKS.md, CHAT.md).
- Altering the one-task/one-commit agent rule.

## Functional requirements

### CLI and entrypoints
- Primary executable: `swarm` (Rust binary).
- Provide a `./init.sh` that spins up a Lima VM and container environment where `swarm` is on PATH.
- CLI supports (at minimum):
  - `swarm init` (create default config, TASKS.md, CHAT.md, log dir)
  - `swarm run` (run sprints until done or until max-sprints is reached)
  - `swarm sprint` (run exactly one sprint)
  - `swarm plan` (sprint planning only)
  - `swarm status` (show task counts and recent chat lines)
  - `swarm agents` (list agent names/initials)
  - `swarm worktrees` / `swarm worktrees-branch`
  - `swarm cleanup` (remove worktrees/branches)
  - `swarm merge` (merge agent branches)
  - `swarm tail` (tail CHAT.md; default UI behavior)
- Default `swarm` with no subcommand behaves like `swarm run` and starts tailing CHAT.md unless `--no-tail` is set.

### Configuration
- Config file (e.g., `swarm.toml`) plus CLI flags and environment variables.
- Required config values:
  - `agents.max_count` (cap on agents that may be spawned)
  - `agents.tasks_per_agent` (N)
  - `files.tasks` (tasks.md path; auto-resolved per team in multi-team mode)
  - `files.chat` (chat.md path; auto-resolved per team in multi-team mode)
  - `files.log_dir` (loop/; auto-resolved per team in multi-team mode)
  - `engine.type` (`claude`, `codex`, or `stub`)
  - `engine.stub_mode` (enables stubbed engine for tests)
  - `sprints.max` (max sprints to run; 0 or absent means unlimited)

### Multi-team architecture
- All swarm-hug configuration and artifacts live in `.swarm-hug/`.
- Each team has its own directory: `.swarm-hug/<team-name>/`.
- Team directory structure:
  - `.swarm-hug/<team>/tasks.md` - Team's task list
  - `.swarm-hug/<team>/chat.md` - Team's chat log
  - `.swarm-hug/<team>/specs.md` - Team's specifications
  - `.swarm-hug/<team>/prompt.md` - Team's prompt
  - `.swarm-hug/<team>/loop/` - Agent logs
  - `.swarm-hug/<team>/worktrees/` - Git worktrees for agents
- Agent assignments tracked in `.swarm-hug/assignments.toml`.
- Agents use canonical alphabetical names (Aaron, Betty, Carlos, ..., Zane).
- **Exclusive assignment**: An agent can only be assigned to one team at a time.
- CLI flag `--team <name>` (or `-t <name>`) specifies which team context to use.
- New commands:
  - `swarm teams` - List all teams and their assigned agents
  - `swarm team init <name>` - Initialize a new team

### Task file format (TASKS.md)
- Preserve checklist format:
  - Unassigned: `- [ ] Task description`
  - Assigned: `- [A] Task description`
  - Completed: `- [x] Task description (A)`
- The initial in the checkbox maps to the assigned agent name.
- Task ordering is backlog priority (top to bottom).

### Agent names and initials
- Fixed A-Z mapping identical to ralph-bash-v2 (Aaron, Betty, Carlos, ... Zane).
- Agent initials are used for assignment and attribution.

### Sprint planning and assignment
- Scrum master assigns up to N tasks per agent per sprint.
- If fewer tasks exist, only available tasks are assigned.
- Assignment updates TASKS.md and is committed to git so worktrees see the changes.
- Planning can be algorithmic or LLM-assisted (optional, via engine).
- The scrum master posts a sprint plan summary to CHAT.md.

### Adaptive agent spawning
- `agents.max_count` is a cap, not a fixed spawn count.
- Scrum master selects an active agent count based on available unblocked work:
  - If only one unblocked task exists (typical at init), spawn one agent.
  - If most remaining tasks are blocked, do not spawn idle agents.
- Blocked detection is based on explicit markers in task descriptions (minimum: recognize `BLOCKED`, `blocked`, or `Blocked by:`).
- Agents are only spawned for the number of tasks that can be assigned this sprint.

### Sprint limits (for tests and short runs)
- Support a hard sprint cap via config/flag (e.g., `--max-sprints 3`).
- When the limit is reached, stop cleanly and leave remaining tasks unassigned.

### Agent execution rules
- Each agent works in its own git worktree and branch.
- Agents execute assigned tasks only.
- Each task results in exactly one commit.
- Agents must merge their branch back to the main branch when done (or the scrum master merges them).
- Agent lifecycle: assigned -> working -> done -> terminated.

### Chat and tail-based UI
- All communication is appended to CHAT.md.
- Required format per line:
  - `YYYY-MM-DD HH:MM:SS | <AgentName> | AGENT_THINK: <message>`
- The UI is a simple tail of CHAT.md. No GridTUI.
- `swarm tail` streams the file and is used by `swarm run` unless disabled.

### Engine abstraction (real + stub)
- Support `claude` and `codex` CLIs as engines.
- Provide a stubbed engine for tests and offline runs:
  - Writes deterministic output files per invocation (e.g., `loop/turn1-agentA.md`).
  - Content is minimal (e.g., `OK`) but consistent for verification.
  - Must not call the network.
- Engine layer must be swappable so tests can run stubbed while production uses real engines.

### Git worktree workflow
- Each agent uses a worktree under `worktrees/agent-<initial>-<name>`.
- Dedicated branch per agent (e.g., `agent/aaron`).
- Worktrees are created before agents run, and cleaned up after merge.
- Merge conflicts are surfaced in CHAT.md and cause the sprint to report failure but not crash the runner.

### Logs
- Per-agent log files under `loop/agent-<initial>.log`.
- Log rotation when size exceeds a safe threshold (line-based or size-based).

### init.sh (Lima VM bootstrap)
- Script similar to ralph-bash-v2, adapted for swarm-hug.
- Provisions a Lima VM with Docker and installs:
  - git, bash, Rust toolchain, and required CLIs (claude/codex).
- Mounts the repo into the VM and exposes `swarm` inside the container.
- Does not include GridTUI.

## Test strategy (must be implemented first)
- Rust test suite includes:
  - Unit tests for task parsing, state transitions, agent naming, and chat formatting.
  - Integration test that:
    - Creates a temp git repo.
    - Writes TASKS.md and CHAT.md.
    - Runs `swarm` with stubbed engine and `--max-sprints` set (e.g., 1 or 3).
    - Verifies:
      - TASKS.md transitions to completed for assigned tasks.
      - CHAT.md contains sprint plan and completion entries.
      - Worktrees are created and cleaned up.
      - Stub output files exist (e.g., `turn1-agentA.md`).
- Tests must run without network access.
- Optional flag to run against real engines is allowed but off by default.

## Technical decisions
- Implementation language: Rust.
- Prefer minimal dependencies; standard library where possible.
- File-based coordination remains the source of truth.
- No GridTUI or external UI dependencies.

## Constraints
- ASCII-only output in files unless a file already uses Unicode.
- No new dependencies unless required to satisfy the spec.
- Behavior must be deterministic under stubbed engine for tests.
- Keep code minimal while meeting all requirements above.

## Workflow requirements (from PROMPT.md)
- Keep README.md accurate, accessible, and friendly after each session.
- Use ../ralph-bash-v2 as a reference for behavior, noting it is older and fragile.
- Maintain an exhaustive TASKS.md that captures all PROMPT/SPECS requirements.
- Check in work (commit) after each completed task.
- If any file exceeds 1000 LOC, add a TASKS.md entry to break it apart.
- Prefer batching up to 3 related tasks per session when allowed by session rules.
