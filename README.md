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

# Initialize the swarm-hug structure
./target/debug/swarm init

# Create teams
./target/debug/swarm team init authentication
./target/debug/swarm team init payments

# List teams and their agents
./target/debug/swarm teams

# Run sprints for a specific team
./target/debug/swarm --team authentication run
./target/debug/swarm -t payments --stub --max-sprints 1 run

# Check team status
./target/debug/swarm -t authentication status
```

## Directory Structure

```
your-repo/
├── .swarm-hug/
│   ├── assignments.toml          # Agent-to-team assignments
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
    -t, --team <NAME>       Team to operate on (uses .swarm-hug/<team>/)
    --max-agents <N>        Maximum number of agents to spawn
    --tasks-per-agent <N>   Tasks to assign per agent per sprint
    --engine <TYPE>         Engine type: claude, codex, stub
    --stub                  Enable stub mode for testing
    --max-sprints <N>       Maximum sprints to run (0 = unlimited)
    --no-tail               Don't tail chat.md during run
```

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

## Configuration

Create `swarm.toml` for global settings:

```toml
[agents]
max_count = 4
tasks_per_agent = 2

[files]
tasks = "TASKS.md"      # Default; overridden by --team
chat = "CHAT.md"        # Default; overridden by --team
log_dir = "loop"        # Default; overridden by --team

[engine]
type = "claude"         # claude, codex, or stub
stub_mode = false

[sprints]
max = 0                 # 0 = unlimited
```

Environment variables (override config file):
- `SWARM_AGENTS_MAX_COUNT`
- `SWARM_AGENTS_TASKS_PER_AGENT`
- `SWARM_ENGINE_TYPE`
- `SWARM_ENGINE_STUB_MODE`
- `SWARM_SPRINTS_MAX`

## Building and Testing

```bash
# Run tests
cargo test --lib --tests

# Build release
cargo build --release
```

## Git Workflow

swarm-hug manages git worktrees and branches for parallel agent work:

### Agent Branches

Each agent gets a dedicated branch named `agent/<lowercase_name>`:
- Agent A (Aaron) → `agent/aaron`
- Agent B (Betty) → `agent/betty`

List agent branches:
```bash
./target/debug/swarm worktrees-branch
```

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

### Task Assignment Commits

When tasks are assigned during sprint planning, the changes are automatically committed to git. This ensures worktrees can pull the latest task assignments.

## Status

The core multi-team architecture is complete:
- Team isolation with separate directories
- Agent assignment tracking (exclusive per team)
- CLI commands for team management (`teams`, `team init`)
- Path resolution based on `--team` flag
- Worktree listing and cleanup per team
- Agent branch listing (`worktrees-branch`)
- Agent branch merging (`merge`)
- Automatic git commits for task assignments

**Still in progress:**
- Full git worktree creation with branch setup (current worktrees are placeholder directories)
- Per-agent logging with rotation
- Lima VM bootstrap script (init.sh)

## Development Workflow

- Track work in TASKS.md
- Keep README.md accurate after each session
- Use ../ralph-bash-v2 as a reference only (it is older and fragile)

## Reference Implementation

This project references ../ralph-bash-v2 for behavior parity, but that project is
legacy and should not be treated as a source of truth.
