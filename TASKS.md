# Tasks

- [x] Clear chat.md on swarm run boot and log "SWARM HUG BOOTING UP" with emoji
- [x] Update --with-prd task formatting to include numbered tasks with blocking info
- [x] Fix dynamic unblocking: tasks with "(blocked by #N)" now become assignable when blocker completes
- [x] Remove legacy BLOCKED: prefix logic
- [x] Update scrum master prompt to distribute tasks across agents (not all to one)
- [x] Add --agent-timeout option (default: 3600s/1hr), show defaults in CLI help
- [x] Change default max-agents from 4 to 3
- [x] Stream Codex output to debug file for real-time visibility
- [x] Support comma-separated engine list for random per-agent selection (e.g., --engine codex,codex,claude)
