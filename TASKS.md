# Tasks

## Team Init Changes
- [ ] Add `prd.md` creation to `swarm team init` (in `.swarm-hug/<team>/prd.md`)
  - Default content: `# PRD: <team-name>\n\nAdd product requirements here.`
- [ ] Add `operator_feedback.md` creation to `swarm team init` (in `.swarm-hug/<team>/operator_feedback.md`)
  - Default content: `# Operator Feedback: <team-name>\n\nLive feedback and instructions go here.`
- [ ] Update scrum master logic to read `prd.md` and ensure `tasks.md` stays in sync with PRD requirements

## File Location Changes
- [ ] Move CHAT.md from project root to `.swarm-hug/CHAT.md`
  - Update `Config::default()` to use `.swarm-hug/CHAT.md` as default
  - Update team mode to use `.swarm-hug/<team>/chat.md` (already does this)
  - Update `swarm init` to create CHAT.md in `.swarm-hug/` not project root
- [ ] Remove root TASKS.md creation from `swarm init`
  - Tasks should only exist per-team in `.swarm-hug/<team>/tasks.md`
  - Remove the default `files_tasks: "TASKS.md"` or point it to team location

## Configuration Simplification
- [ ] Remove `swarm.toml` support entirely
  - Delete `Config::load_from_file()` and related TOML parsing
  - Remove `swarm.toml` creation from `swarm init`
  - Remove `-c, --config` CLI flag
  - Remove `Config::default_toml()` method
  - All configuration via CLI flags only (e.g., `--max-agents`, `--engine`)
- [ ] Make `--llm-planning` always on (remove the flag)
  - Remove `--llm-planning` CLI flag from argument parsing
  - Set `planning_llm_enabled = true` unconditionally in Config
  - Remove `llm_planning` field from `CliArgs` struct
  - Update help text to remove the option

## Tests & Documentation
- [ ] Update tests that reference swarm.toml or root TASKS.md/CHAT.md
- [ ] Update README.md to reflect new file locations and removed config file
- [ ] Update any integration tests that depend on old file structure

## Completed (Previous Work)
- [x] Add `rebuild-swarm` alias in init.sh for rebuilding swarm binary inside the VM
- [x] Ensure /opt/swarm-hug is mounted RW (not RO) so cargo build works
- [x] Update README.md with rebuild-swarm documentation
- [x] Fix CARGO_HOME to use writable volume in VM
- [x] Fix rebuild-swarm to preserve working directory (subshell)
