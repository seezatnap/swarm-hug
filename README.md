![swarm-hug](https://github.com/user-attachments/assets/2eced3f3-5518-431b-bf24-fd648f7041c9)

A sprint analogy for agent orchestration, in a cli interface.

## Quick Start

`swarm` will spawn agents that run in "automatic" mode (they will take actions without any confirmation); they will run arbitrary commands, and might make catastrophic mistakes, like deleting your home folder. Who knows! 

As such, you should only run this in a sandbox. This script will set up a [Lima](https://github.com/lima-vm/lima) VM with [Docker](https://www.docker.com/products/docker-desktop/) and mount the provided folders, then set up the `swarm` alias for you to use within:

```bash
# Wherever you checked this out:
../my-repos/swarm-hug/init_lima.sh --name my-project ~/Sites/my-project ~/some-other-folder
```

## Your First Swarm Project

* Write a PRD (or have an LLM do it for you). Review it manually + carefully!
* Start a project: `swarm init project greenfield --with-prd ./prds/greenfield.md`
* Start swarmin': `swarm run --project greenfield`

<img width="894" height="573" alt="Screenshot 2026-01-25 at 9 31 44 PM" src="https://github.com/user-attachments/assets/4ebb039f-63b1-4cb2-9e37-9188e29c8fe0" />

## CLI Usage

```
swarm - multi-agent sprint-based orchestration system

USAGE:
    swarm [OPTIONS] [COMMAND]

COMMANDS:
    init                  Initialize a new swarm repo (creates .swarm-hug/)
    run                   Run sprints until done or max-sprints reached (default)
    sprint                Run exactly one sprint
    plan                  Run sprint planning only (assign tasks)
    status                Show task counts and recent chat lines
    agents                List agent names and initials
    projects              List all projects and their assigned agents
    project init <name>   Initialize a new project
                          Use --with-prd <file> to auto-generate tasks from a PRD
    worktrees             List active git worktrees
    worktrees-branch      List worktree branches
    cleanup               Remove worktrees and branches
    customize-prompts     Copy prompts to .swarm-hug/prompts/ for customization
    set-email <email>     Set co-author email for commits (stored in .swarm-hug/email.txt)

OPTIONS:
    -h, --help                Show this help message
    -V, --version             Show version
    -c, --config <PATH>       Path to config file [default: swarm.toml]
    -p, --project <NAME>      Project to operate on (uses .swarm-hug/<project>/)
    --max-agents <N>          Maximum number of agents to spawn [default: 3]
    --tasks-per-agent <N>     Tasks to assign per agent per sprint [default: 2]
    --agent-timeout <SECS>    Agent execution timeout in seconds [default: 3600]
    --tasks-file <PATH>       Path to tasks file [default: <project>/tasks.md]
    --chat-file <PATH>        Path to chat file [default: <project>/chat.md]
    --log-dir <PATH>          Path to log directory [default: <project>/loop/]
    --engine <TYPE>           Engine type(s): claude, codex, stub (comma-separated for per-task selection) [default: claude]
    --stub                    Enable stub mode for testing [default: false]
    --max-sprints <N>         Maximum sprints to run (0 = unlimited) [default: 0]
    --no-tail                 Don't tail chat.md during run [default: false]
    --no-tui                  Disable TUI mode (use plain text output) [default: false]
```

## Runbook

- Chat history is cleared once when `swarm run` starts, then preserved across all sprints in that run.
- After each sprint completes, `SPRINT STATUS` summary lines are appended to `chat.md`.
- While agents are running, a heartbeat line is appended to `chat.md` roughly every 5 minutes; it stops when agents finish.
- Sprint follow-up tickets use the prd-to-task format: `- [ ] (#123) description (blocked by #100, #101)`.

## Engine Selection

- `--engine` accepts a comma-separated list (e.g., `claude,codex`). When multiple engines are provided, each task randomly selects one engine.
- Chat entries include the engine used for that task: `Starting: <task> [engine: <name>]`.
- You can weight selection by repeating an engine (e.g., `claude,claude,codex`).

## Requirements for init_lima.sh

- Lima (`limactl`) and Docker installed on the host

## Development

If you change the source files, you can rebuild `swarm` inside the VM using:

```bash
rebuild-swarm    # Alias that rebuilds and reports success
```

## Building and Testing

```bash
# Run tests
cargo test --lib --tests

# Build release
cargo build --release
```
