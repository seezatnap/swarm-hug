# Specifications: per-task-engine-selection

# PRD: Per-Task Engine Selection

## Summary
Change engine selection from per-agent to per-task. When multiple engines are configured (e.g., `--engine claude,codex`), a random engine should be selected for each task execution, not once per agent at spawn time.

## Goals
- Enable true load balancing across engines by selecting randomly per task.
- Allow a single agent to use different engines for different tasks within the same sprint.
- Provide visibility into which engine is being used for each task.

## Non-goals
- No changes to engine configuration syntax (`--engine claude,codex` remains valid).
- No changes to how stub mode works (stub mode always uses stub engine).
- No weighted selection beyond duplicate entries (e.g., `claude,claude,codex` for 2:1 weighting).

## Current Behavior
Engine selection happens once per agent when the agent thread spawns (in `runner.rs` lines 314-326). Betty gets assigned "codex" at spawn time and uses codex for all her tasks. Aaron gets assigned "claude" and uses claude for all his tasks.

## Desired Behavior
Engine selection happens for each task execution. Betty might use claude for task #1, codex for task #2, and claude again for task #3. The selection is random with equal probability for each engine in the configured list.

## Implementation

### 1) Move engine selection into the task loop
**Location:** `src/runner.rs`, inside the `for description in tasks` loop (around line 346).
**Change:** Move the engine selection logic from outside the loop to inside the loop, so a new engine is selected before each `engine.execute()` call.

### 2) Log selected engine to chat
**Current:** Engine selection is only logged to the agent's log file.
**Desired:** When an agent starts a task, include the engine name in the chat message.
**Format:** `Starting: <task description> [engine: claude]` or similar suffix.

### 3) Update engine creation to happen per-task
**Current:** `engine::create_engine()` is called once per agent.
**Desired:** Either call `create_engine()` per task, or pass the engine types list and select/create inside the loop.

## Acceptance Criteria
- With `--engine claude,codex`, a single agent uses both engines across their assigned tasks (verifiable via chat logs).
- Each task's chat "Starting:" message indicates which engine is being used.
- Agent log files show the engine used for each task.
- Stub mode continues to work (always uses stub engine).
- Single-engine configuration works unchanged.

## Risks
- Slightly more overhead from creating engine instances per task instead of per agent (likely negligible since engines are lightweight wrappers around CLI invocations).
- Random selection means no guarantee of even distribution in small samples.

