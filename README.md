# swarm-hug

A Rust rewrite of the bash-based multi-agent sprint orchestration system.

## Status

This repo is in early scaffolding. The CLI and sprint runner are not implemented yet.
The following pieces are in place:

- TASKS.md parser/writer with assignment states
- Agent name/initial mapping (A-Z)
- CHAT.md line formatting helpers
- Unit tests for the modules above

## Goals (from SPECS.md)

- Multi-agent sprint planning with git worktrees
- File-based coordination via TASKS.md and CHAT.md
- Stubbed engine for deterministic tests
- Tail-based UI (no GridTUI)
- Lima VM bootstrap script (init.sh)

## Development workflow

- Track work in TASKS.md (one task per session and one commit per task).
- Keep README.md accurate after each session.
- Use ../ralph-bash-v2 as a reference only (it is older and fragile).

## Building and testing

```bash
cargo test
```

## Repository layout

- src/ - Rust modules (task parsing, agent naming, chat helpers)
- loop/ - log/output directory (generated)
- TASKS.md - active task backlog
- SPECS.md - product requirements and constraints

## Reference implementation

This project references ../ralph-bash-v2 for behavior parity, but that project is
legacy and should not be treated as a source of truth.
