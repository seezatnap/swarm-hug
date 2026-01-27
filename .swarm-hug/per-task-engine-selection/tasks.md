# Tasks

## Backend - Engine Selection Logic

- [C] (#1) Refactor engine selection from agent spawn to task loop in `runner.rs` - move selection logic inside `for description in tasks` loop
- [C] (#2) Update `engine::create_engine()` to support per-task instantiation - pass engine types list and select/create per task
- [C] (#3) Add random engine selection helper function that picks from configured engine list with equal probability

## Backend - Logging and Chat Output

- [ ] (#4) Modify "Starting:" chat message format to include engine name suffix: `Starting: <task> [engine: <name>]` (blocked by #1)
- [ ] (#5) Update agent log file entries to include engine name for each task execution (blocked by #1)

## Testing

- [ ] (#6) Add unit tests for random engine selection logic - verify equal distribution over many iterations (blocked by #3)
- [ ] (#7) Add integration test for multi-engine configuration - verify single agent uses both engines across tasks (blocked by #1, #2)
- [ ] (#8) Add test to verify stub mode continues using stub engine exclusively (blocked by #1, #2)
- [ ] (#9) Add test to verify single-engine configuration works unchanged (blocked by #1, #2)

## Documentation

- [ ] (#10) Update README or user docs to document per-task engine selection behavior (blocked by #1, #4)
