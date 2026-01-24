# swarm-hug

A sprint analogy for agent orchestration, in a cli interface.

## Quick Start

`swarm` will spawn agents that run in "full automatic" mode; they will run arbitrary commands, and might make catastrophic mistakes, like deleting your home folder. Who knows! 

As such, you should only run this in a sandbox. This script will set up a [Lima](https://github.com/lima-vm/lima) VM and mount your target folders, then set up the `swarm` alias for you to use within:

```bash
# Wherever you checked this out:
../my-repos/swarm-hug/init_lima.sh --name my-project ~/Sites/my-project ~/some-other-folder
```

## CLI Usage

The idea here is to make a command that can spawn a custom-tailored sprint, or set of sprints, to accomplish specific goals. i.e. you might have a small project where the tickets all block each other, in which case you might just want one agent. Later you might have broader tasks that can be parallelized with several agents.

```
swarm - multi-agent sprint-based orchestration system

USAGE:
    swarm [OPTIONS] [COMMAND]

COMMANDS:
    init              Initialize a new swarm project (creates .swarm-hug/)
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
    customize-prompts Copy prompts to .swarm-hug/prompts/ for customization

OPTIONS:
    -h, --help              Show this help message
    -V, --version           Show version
    -c, --config <PATH>     Path to config file (default: swarm.toml)
    -t, --team <NAME>       Team to operate on (uses .swarm-hug/<team>/)
    --max-agents <N>        Maximum number of agents to spawn
    --tasks-per-agent <N>   Tasks to assign per agent per sprint
    --tasks-file <PATH>     Path to tasks file (default: tasks.md in team dir)
    --chat-file <PATH>      Path to chat file (default: chat.md in team dir)
    --log-dir <PATH>        Path to log directory (default: loop/ in team dir)
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
Assignments are claimed during planning/run and released on `swarm cleanup` or `swarm merge`.


## Requirements for init_lima.sh

- Lima (`limactl`) and Docker installed on the host

## Development

Within the vm, you can rebuild `swarm` with this command:

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