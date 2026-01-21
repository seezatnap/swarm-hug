# swarm-hug

A Rust rewrite of the bash-based multi-agent sprint orchestration system, now with multi-team support.

## What's New: Multi-Team Architecture

Multiple teams can work on the same repository simultaneously without conflicts. Each team gets:
- Its own isolated directory under `.swarm-hug/<team>/`
- Separate task lists, chat logs, and worktrees
- Dedicated agents that can't be double-booked

## Quick Start

```bash
# Build
cargo build

# Initialize the swarm-hug structure (creates swarm.toml and .swarm-hug/default/{tasks.md,chat.md,loop/,worktrees/})
./target/debug/swarm init

# Ensure there is at least one git commit (required for worktrees)
git commit --allow-empty -m "init"

# Run using the default workspace (no team flag)
./target/debug/swarm run

# Create teams (each team gets its own tasks/chat/loop/worktrees under .swarm-hug/)
./target/debug/swarm team init authentication
./target/debug/swarm team init payments

# List teams and their agents
./target/debug/swarm teams

# Run sprints for a specific team
./target/debug/swarm --team authentication run
./target/debug/swarm -t payments --stub --max-sprints 1 run
# By default, `run` tails chat.md; use --no-tail to disable.

# Check team status
./target/debug/swarm -t authentication status
```

## Lima VM Bootstrap (init.sh)

If you want an isolated, repeatable environment, use the Lima bootstrap script:

```bash
# From the repo root
./init.sh ~/code/swarm-hug
```

What it does:
- Creates/starts a Lima VM with Docker
- Builds a container image with git, bash, Rust, and the codex/claude CLIs
- Mounts your repo at `/opt/swarm-hug` and exposes `swarm` in PATH

To enter the container later:
```bash
docker --context "lima-swarmbox" exec -it "swarmbox-agent" bash -l
```

Inside the container, the `swarm` wrapper will auto-build if needed. You can also rebuild manually:
```bash
rebuild-swarm    # Alias that rebuilds and reports success
# or manually:
cd /opt/swarm-hug && cargo build
```

## Requirements for init.sh

- Lima (`limactl`) and Docker installed on the host
- Python 3 (used for path normalization)

## init.sh Options

```bash
./init.sh [--name VM] [--container NAME] [--ports 3000,5173] [--no-auth] <folder1> <folder2> ...
```

## Directory Structure

```
your-repo/
├── .swarm-hug/
│   ├── assignments.toml          # Agent-to-team assignments
│   ├── default/                  # Used when no --team is specified
│   │   ├── tasks.md
│   │   ├── chat.md
│   │   ├── loop/
│   │   └── worktrees/
│   ├── authentication/           # Team directory
│   │   ├── tasks.md              # Team's task list
│   │   ├── chat.md               # Team's chat log
│   │   ├── specs.md              # Team's specifications
│   │   ├── prompt.md             # Team's prompt
│   │   ├── loop/                 # Agent logs
│   │   └── worktrees/            # Git worktrees
│   └── payments/                 # Another team
│       ├── tasks.md
│       ├── chat.md
│       └── ...
└── swarm.toml                    # Global configuration
```

## CLI Usage

```
swarm - multi-agent sprint-based orchestration system

USAGE:
    swarm [OPTIONS] [COMMAND]

COMMANDS:
    init              Initialize .swarm-hug/ structure
    run               Run sprints until done or max-sprints reached (default)
    sprint            Run exactly one sprint
    plan              Run sprint planning only (assign tasks)
    status            Show task counts and recent chat lines
    agents            List agent names and initials
    teams             List all teams and their assigned agents
    team init <name>  Initialize a new team
    worktrees         List active git worktrees
    worktrees-branch  List worktree branches
    cleanup           Remove worktrees and branches
    merge             Merge agent branches to main
    tail              Tail chat.md (stream output)

OPTIONS:
    -h, --help              Show this help message
    -V, --version           Show version
    -c, --config <PATH>     Path to config file (default: swarm.toml)
    -t, --team <NAME>       Team to operate on (default uses .swarm-hug/default/)
    --max-agents <N>        Maximum number of agents to spawn
    --tasks-per-agent <N>   Tasks to assign per agent per sprint
    --engine <TYPE>         Engine type: claude, codex, stub
    --stub                  Enable stub mode for testing
    --max-sprints <N>       Maximum sprints to run (0 = unlimited)
    --no-tail               Don't tail chat.md during run
    --llm-planning          Enable LLM-assisted sprint planning (experimental)
```

Note: `swarm cleanup` removes team worktrees and any local `agent/*` branches.
Tip: `swarm status` prints task counts and the last 5 chat lines for the selected team.

## Agent Assignments

Agents are assigned alphabetically (Aaron, Betty, Carlos, etc.) and tracked in `.swarm-hug/assignments.toml`:

```toml
# Agent Assignments
# An agent can only be assigned to one team at a time.

[agents]
A = "authentication"
B = "authentication"
C = "payments"
D = "payments"
```

An agent working on one team cannot be assigned to another until released.
Assignments are claimed during planning/run and released on `swarm cleanup` or `swarm merge`.

## Configuration

Create `swarm.toml` for global settings:

```toml
[agents]
max_count = 4
tasks_per_agent = 2

[files]
tasks = ".swarm-hug/default/tasks.md"  # Default; overridden by --team
chat = ".swarm-hug/default/chat.md"    # Default; overridden by --team
log_dir = ".swarm-hug/default/loop"    # Default; overridden by --team

[engine]
type = "claude"         # claude, codex, or stub
stub_mode = false

[sprints]
max = 0                 # 0 = unlimited

[planning]
llm_enabled = false     # Use LLM for intelligent task assignment
```

Environment variables (override config file):
- `SWARM_AGENTS_MAX_COUNT`
- `SWARM_AGENTS_TASKS_PER_AGENT`
- `SWARM_ENGINE_TYPE`
- `SWARM_ENGINE_STUB_MODE`
- `SWARM_SPRINTS_MAX`
- `SWARM_PLANNING_LLM_ENABLED`

## Building and Testing

```bash
# Run tests
cargo test --lib --tests

# Build release
cargo build --release
```

Notes:
- Integration tests run the `swarm` binary in a temp git repo, using a stub engine and a team directory under `.swarm-hug/`.
- No network access is required for tests.

## Git Workflow

swarm-hug manages git worktrees and branches for parallel agent work:

### Agent Branches

Each agent gets a dedicated branch named `agent/<lowercase_name>`:
- Agent A (Aaron) -> `agent/aaron`
- Agent B (Betty) -> `agent/betty`

Worktrees are real git worktrees created with `git worktree add` under the
team's worktree directory (for example, `.swarm-hug/<team>/worktrees`).
The repository must have at least one commit before worktrees can be created.

List agent branches:
```bash
./target/debug/swarm worktrees-branch
```

### Agent Execution in Worktrees

Agents execute their tasks inside their own worktrees, ensuring isolation:
- Each agent works in `.swarm-hug/<team>/worktrees/agent-<INITIAL>-<Name>/`
- The engine receives the worktree path as the working directory
- This prevents agents from stepping on each other's changes

### Lifecycle Tracking

Each agent goes through these states during a sprint:
1. **Assigned** - Task has been assigned, agent ready to start
2. **Working** - Agent is actively executing the task
3. **Done** - Agent completed (success or failure)
4. **Terminated** - Agent has been cleaned up

### One Task = One Commit

Each agent creates exactly one commit per task:
- Commits are made in the agent's worktree/branch
- Commit author is the agent (e.g., "Agent Aaron <agent-A@swarm.local>")
- Commit message includes the task description

### Merging

When agents complete their work, merge their branches:
```bash
./target/debug/swarm merge
```

The merge command:
- Attempts to merge all active agent branches
- Uses `--no-ff` to create merge commits
- Reports conflicts (and aborts conflicted merges for manual resolution)
- Skips branches with no changes
- Posts merge status to chat.md
- Automatically cleans up worktrees and branches after successful merges

### Task Assignment Commits

When tasks are assigned during sprint planning, the changes are automatically committed to git. This ensures worktrees can pull the latest task assignments.
The commit also includes `.swarm-hug/assignments.toml` so agent claims stay in sync across worktrees.

### Per-Agent Logging

Each agent writes logs to `loop/agent-<initial>.log`:

```
2026-01-21 17:00:00 | Aaron | Assigned task: Implement feature
2026-01-21 17:00:00 | Aaron | Working directory: .swarm-hug/myteam/worktrees/agent-A-Aaron
2026-01-21 17:00:00 | Aaron | State: ASSIGNED -> WORKING
2026-01-21 17:00:01 | Aaron | Executing with engine: stub
2026-01-21 17:00:02 | Aaron | State: WORKING -> DONE (success)
2026-01-21 17:00:02 | Aaron | Task completed: Implement feature
```

Features:
- Automatic log rotation when files exceed 1000 lines
- Rotated logs are backed up with timestamps (e.g., `agent-A.log.20260121-170000.bak`)
- Session separators for clarity between runs

## LLM-Assisted Planning (Experimental)

By default, tasks are assigned algorithmically (round-robin). Enable LLM-assisted planning for smarter assignments:

```bash
# Via CLI flag
./target/debug/swarm --team myteam --llm-planning run

# Via config file (swarm.toml)
[planning]
llm_enabled = true

# Via environment variable
SWARM_PLANNING_LLM_ENABLED=true ./target/debug/swarm run
```

When enabled, the LLM acts as scrum master and:
- Groups related tasks to the same agent
- Respects task dependencies
- Avoids file conflicts between agents
- Uses priority order (earlier tasks = higher priority)

If LLM planning fails, it automatically falls back to algorithmic assignment.

## Status

Core multi-team orchestration is in place, with a few planning/merge polish items still in progress.
See `TASKS.md` for current priorities and remaining verification work.

**Highlights already working:**
- Team isolation with separate directories
- Exclusive agent assignment tracking per team
- CLI commands for team management (`teams`, `team init`)
- Path resolution based on `--team` flag
- Worktree listing and cleanup per team
- Agent branch listing (`worktrees-branch`)
- Agent branch merging (`merge`)
- Automatic git commits for task assignments
- Agent execution inside worktrees (engine uses worktree path)
- Per-agent lifecycle tracking (assigned -> working -> done -> terminated)
- One task = one commit rule enforced
- Per-agent logging with rotation
- LLM-assisted sprint planning (experimental)

**Also available:**
- Lima VM bootstrap script (`init.sh`) for a reproducible Docker+Lima environment

## Development Workflow

- Track work in the team task file (default: `.swarm-hug/default/tasks.md`)
- Keep README.md accurate after each session
- Use ../ralph-bash-v2 as a reference only (it is older and fragile)

## Reference Implementation

This project references ../ralph-bash-v2 for behavior parity, but that project is
legacy and should not be treated as a source of truth.
