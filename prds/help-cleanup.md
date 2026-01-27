# PRD: CLI Help Cleanup

## Summary
Streamline the `swarm --help` output by removing deprecated, internal, or rarely-used commands and options. This cleanup also removes the underlying code and updates tests accordingly.

## Goals
- Simplify the CLI interface for end users.
- Remove commands that are internal implementation details or debugging tools.
- Clean up unused code paths and their associated tests.
- Keep the help output focused on the primary workflow.

## Non-goals
- No changes to core sprint execution logic.
- No changes to the TUI or agent behavior.

## Commands to Remove

### 1) `sprint`
**Reason:** Internal debugging command. Users should use `run` which handles sprint iteration automatically.

### 2) `plan`
**Reason:** Internal implementation detail. Sprint planning happens automatically as part of `run`.

### 3) `status`
**Reason:** Rarely used. Users can check task files directly or rely on TUI output.

### 4) `worktrees`
**Reason:** Internal debugging command for inspecting git worktrees.

### 5) `worktrees-branch`
**Reason:** Internal debugging command for inspecting worktree branches.

### 6) `cleanup`
**Reason:** Worktree cleanup should happen automatically or be a manual git operation.

## Options to Remove

### 1) `--no-tail`
**Reason:** Tailing behavior is an internal implementation detail. The TUI handles output display.

## Sections to Remove from Help

### 1) MULTI-PROJECT MODE
**Reason:** The directory structure is an implementation detail. Users don't need to know the internal file layout.

## Examples to Simplify

Remove these examples:
- `swarm project init payments` (redundant second project example)
- `swarm --project authentication run` (verbose; -p shorthand is sufficient)
- `swarm -p payments status` (status command being removed)

Keep these examples:
- `swarm init`
- `swarm project init authentication`
- `swarm projects`

Add a simple run example:
- `swarm -p authentication run`

## Implementation Notes

For each removed command:
1. Remove the `Command` enum variant in `src/config/cli.rs`
2. Remove the match arm in `Command::from_str()`
3. Remove the command handler function in `src/commands/`
4. Remove the dispatch in `src/main.rs`
5. Update or remove associated tests

For removed options:
1. Remove the field from `CliArgs` struct
2. Remove the parsing in `parse_args()`
3. Remove from `Config` if applicable
4. Update tests

## Target Help Output

```
swarm - multi-agent sprint-based orchestration system

USAGE:
    swarm [OPTIONS] [COMMAND]

COMMANDS:
    init                  Initialize a new swarm repo (creates .swarm-hug/)
    run                   Run sprints until done or max-sprints reached (default)
    agents                List agent names and initials
    projects              List all projects and their assigned agents
    project init <name>   Initialize a new project
                          Use --with-prd <file> to auto-generate tasks from a PRD
    customize-prompts     Copy prompts to .swarm-hug/prompts/ for customization
    set-email <email>     Set co-author email for commits

OPTIONS:
    -h, --help                Show this help message
    -V, --version             Show version
    -c, --config <PATH>       Path to config file [default: swarm.toml]
    -p, --project <NAME>      Project to operate on
    --max-agents <N>          Maximum number of agents to spawn [default: 3]
    --tasks-per-agent <N>     Tasks to assign per agent per sprint [default: 2]
    --agent-timeout <SECS>    Agent execution timeout in seconds [default: 3600]
    --tasks-file <PATH>       Path to tasks file
    --chat-file <PATH>        Path to chat file
    --log-dir <PATH>          Path to log directory
    --engine <TYPE>           Engine type(s): claude, codex, stub [default: claude]
                              Comma-separated for load balancing (e.g., claude,claude,codex)
    --stub                    Enable stub mode for testing
    --max-sprints <N>         Maximum sprints to run (0 = unlimited) [default: 0]
    --no-tui                  Disable TUI mode (use plain text output)

EXAMPLES:
    swarm init                        Initialize .swarm-hug/ structure
    swarm project init myproject      Create a new project
    swarm projects                    List all projects
    swarm -p myproject run            Run sprints for a project
```

## Acceptance Criteria
- Help output matches the target format above.
- All removed commands return an error or are unrecognized.
- No dead code remains for removed features.
- All tests pass after cleanup.
- `cargo clippy` reports no new warnings.

## Risks
- Users with scripts using removed commands will need to update them.
- The `--no-tail` removal may affect users who pipe output.
