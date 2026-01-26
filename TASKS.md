# Tasks

- [x] (#4) Emit a "planning started" chat entry when the sprint planning agent begins processing (include tests)
- [ ] (#5) Emit a "post-mortem started" chat entry when the post-mortem agent begins processing (include tests)
- [ ] (#6) Update sprint follow-up ticket generator to output prd-to-task format `- [ ] (#N) description (blocked by #M)` with proper numbering and blocked-by list (include tests)
- [ ] (#1) Move chat reset to `swarm run` initialization so chat persists across sprints within a single run (include tests)
- [ ] (#2) Append SPRINT STATUS summary lines to chat after each sprint completes (include tests)
- [ ] (#3) Restore 5-minute heartbeat/agent-activity chat logs while agents run, not visible in TUI (include tests)
- [ ] (#12) Update README/runbook to document chat persistence, sprint status output, heartbeat logs, and follow-up ticket format (blocked by #1, #2, #3, #4, #5, #6)
