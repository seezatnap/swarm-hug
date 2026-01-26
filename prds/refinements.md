# PRD: UX Refinements (logging and chat behavior)

## Summary
A set of quality-of-life improvements to restore and enhance logging visibility during swarm runs. These changes address missing feedback in the chat interface and improve transparency of agent activity.

## Goals
- Improve visibility into agent activity during sprints.
- Restore previously working logging behavior that has regressed.
- Provide clearer status feedback at sprint boundaries.

## Non-goals
- No changes to core sprint logic or task execution.
- No changes to agent behavior or prompt content.

## Refinements

### 1) Preserve chat history across sprints
**Current behavior:** Chat is cleared after each sprint.
**Desired behavior:** Only clear chat when `swarm run` initializes. Preserve chat history between sprints within a single run.

### 2) Restore sprint status printout
**Current behavior:** Sprint status no longer appears after sprint completion.
**Desired behavior:** After each sprint completes, print the SPRINT STATUS to chat. This can be implemented as chat line entries (does not need to be a separate UI component).

### 3) Sprint planning agent should log when starting
**Current behavior:** Sprint planning agent begins work silently.
**Desired behavior:** Sprint planning agent should log a message to chat when it starts thinking/processing.

### 4) Post-mortem should log when starting
**Current behavior:** Post-mortem begins silently.
**Desired behavior:** Post-mortem should log a message to chat when it starts thinking/processing.

### 5) Restore periodic agent activity logs
**Current behavior:** The logs that used to appear every 5 minutes while agents are running no longer show up.
**Desired behavior:** Restore the 5-minute interval logging to chat logs so users can see agents are still active during long-running operations.

## Acceptance criteria
- Chat history persists between sprints until a new `swarm run` is initiated.
- Sprint status summary is printed to chat after each sprint completes.
- Sprint planning agent logs a "starting to think" message.
- Post-mortem logs a "starting to think" message.
- Periodic (5-minute) activity logs appear in chat while agents are running.

## Risks
- Preserving chat history may increase memory usage for very long runs.
