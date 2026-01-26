# Tasks

## Sprint Lifecycle Logging
- [x] (#1) Move chat reset to `swarm run` initialization so chat persists across sprints within a single run
- [x] (#2) Append SPRINT STATUS summary lines to chat after each sprint completes (A)
- [x] (#3) Restore 5-minute heartbeat/agent-activity chat logs while agents run, not visible in TUI

## Agent Start Messaging
- [x] (#4) Emit a "planning started" chat entry when the sprint planning agent begins processing
- [x] (#5) Emit a "post-mortem started" chat entry when the post-mortem agent begins processing

## Task Generation
- [x] (#6) Update sprint follow-up ticket generator to output prd-to-task format `- [ ] (#N) description (blocked by #M)` with proper numbering and blocked-by list

## Testing
- [x] (#9) Add test covering 5-minute heartbeat log cadence and shutdown when agents finish
- [ ] (#7) Add regression test for chat history persistence across consecutive sprints in a single run
- [x] (#8) Add regression test asserting sprint status summary is appended to chat after sprint completion (blocked by #2) (A)
- [x] (#10) Add tests asserting planning and post-mortem start messages are written to chat
- [ ] (#11) Add test verifying follow-up tickets match prd-to-task formatting and numbering (blocked by #6)

## Documentation
- [ ] (#12) Update README/runbook to document chat persistence, sprint status output, heartbeat logs, and follow-up ticket format (blocked by #2, #3, #6)
