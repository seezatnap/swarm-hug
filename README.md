# swarm-hug

A Rust rewrite of the bash-based multi-agent sprint orchestration system.

## Status

The CLI and core functionality are implemented. The following are working:

- **Configuration**: Load from `swarm.toml`, environment variables, and CLI flags
- **CLI**: Full command-line interface with help, version, and all subcommands
- **Task Management**: Parse and write TASKS.md with assignment states
- **Agent Naming**: A-Z mapping (Aaron through Zane)
- **Chat**: CHAT.md formatting and read/write
- **Engine Abstraction**: Swappable backends (claude, codex, stub)
- **Stub Engine**: Deterministic output for testing (no network calls)
- **Sprint Execution**: Plan, run, and limit sprints
- **Integration Tests**: Stubbed end-to-end run in a temp git repo
- **Worktree Prep**: Placeholder worktree directories per assigned agent

**Not yet implemented:**
- Full git worktree management (current worktrees are placeholder directories)
- Worktree listing/branch listing
- Agent branch merging
- Per-agent logging

## Quick Start

```bash
# Build
cargo build

# Initialize a new project
./target/debug/swarm init

# Show available agents
./target/debug/swarm agents

# Check task status
./target/debug/swarm status

# Run with stub engine (for testing)
./target/debug/swarm --stub --max-sprints 1 run
```

## CLI Usage

```
swarm - multi-agent sprint-based orchestration system

USAGE:
    swarm [OPTIONS] [COMMAND]

COMMANDS:
    init              Initialize a new swarm project
    run               Run sprints until done or max-sprints reached (default)
    sprint            Run exactly one sprint
    plan              Run sprint planning only
    status            Show task counts and recent chat lines
    agents            List agent names and initials
    worktrees         List active git worktrees
    worktrees-branch  List worktree branches
    cleanup           Remove worktrees and branches
    merge             Merge agent branches to main
    tail              Tail CHAT.md

OPTIONS:
    -h, --help              Show help
    -V, --version           Show version
    -c, --config <PATH>     Config file (default: swarm.toml)
    --max-agents <N>        Max agents to spawn
    --tasks-per-agent <N>   Tasks per agent per sprint
    --engine <TYPE>         Engine: claude, codex, stub
    --stub                  Enable stub mode for testing
    --max-sprints <N>       Max sprints (0 = unlimited)
    --no-tail               Don't tail CHAT.md during run
```

## Configuration

Create `swarm.toml`:

```toml
[agents]
max_count = 4
tasks_per_agent = 2

[files]
tasks = "TASKS.md"
chat = "CHAT.md"
log_dir = "loop"

[engine]
type = "claude"
stub_mode = false

[sprints]
max = 0
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

Note: doctests may fail in noexec temp environments; use `cargo test --lib --tests` when that occurs.

## Goals (from SPECS.md)

- Multi-agent sprint planning with git worktrees
- File-based coordination via TASKS.md and CHAT.md
- Stubbed engine for deterministic tests
- Tail-based UI (no GridTUI)
- Lima VM bootstrap script (init.sh)

## Development Workflow

- Track work in TASKS.md (one task per session, one commit per task)
- Keep README.md accurate after each session
- Use ../ralph-bash-v2 as a reference only (it is older and fragile)

## Repository Layout

```
src/
  lib.rs          - Library exports
  main.rs         - CLI entry point
  agent.rs        - Agent naming (A-Z)
  chat.rs         - CHAT.md read/write
  config.rs       - Configuration loading
  engine.rs       - Engine abstraction (claude, codex, stub)
  task.rs         - TASKS.md parsing
  worktree.rs     - Placeholder worktree directory management
tests/            - Integration tests
loop/             - Log/output directory
TASKS.md          - Active task backlog
SPECS.md          - Product requirements
```

## Reference Implementation

This project references ../ralph-bash-v2 for behavior parity, but that project is
legacy and should not be treated as a source of truth.
