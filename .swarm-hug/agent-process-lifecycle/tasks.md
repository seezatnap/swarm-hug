# Tasks

## Phase 1: Critical Fixes

- [x] (#1) Fix zombie processes in both engines by adding `child.wait()` after all `child.kill()` calls in `src/engine/claude.rs` and `src/engine/codex.rs`, and fix Codex streaming thread joins to ensure stdout/stderr reader threads are joined on timeout exit path [5 pts] (A)

## Phase 2: Process Registry

- [x] (#2) Create `src/process_registry.rs` module with thread-safe `ProcessRegistry` struct using `Mutex<HashSet<u32>>`, implementing `new()`, `register(pid)`, `unregister(pid)`, `all_pids()`, and `kill_all()` methods, plus a global `PROCESS_REGISTRY` static using `lazy_static` or `once_cell` [5 pts] (B)
- [x] (#3) Integrate process registry into both claude.rs and codex.rs engines: register PID immediately after `Command::spawn()`, unregister PID after `child.wait()` on all exit paths (success, timeout, shutdown, error), and wire `PROCESS_REGISTRY.kill_all()` to the shutdown handler in `src/shutdown.rs` [5 pts] (blocked by #2) (A)

## Phase 3: Process Group Management

- [x] (#4) Create `src/process_group.rs` helper module with `spawn_in_new_process_group()` function that uses `pre_exec` with `libc::setpgid(0, 0)` on Unix and standard spawn on Windows, then update both claude.rs and codex.rs engine spawn calls to use this helper [5 pts] (A)
- [x] (#5) Extract shared `kill_process_tree(pid)` function to `src/process.rs` that sends SIGTERM to process group, waits 100ms, sends SIGKILL to process group, and uses pkill as backup for children; include Windows taskkill implementation; update TUI and plain text modes to use this shared function [5 pts] (B)

## Phase 4: Signal Propagation

- [x] (#6) Add shutdown check to engine polling loops in both claude.rs and codex.rs that calls `shutdown::requested()` and terminates subprocess gracefully when triggered, returning appropriate shutdown error result with exit code 130 [4 pts] (blocked by #3) (A)
- [x] (#7) Implement graceful signal escalation in `kill_subprocess()` helper: send SIGTERM first, wait 100ms, check if still running with `try_wait()`, send SIGKILL if needed, always call `child.wait()` to reap; use this helper in both timeout and shutdown paths in both engines [5 pts] (blocked by #1) (A)

## Phase 5: Testing

- [ ] (#8) Add integration tests for subprocess cleanup: test `test_engine_timeout_no_zombie` that spawns engine with short timeout and verifies no zombie processes remain; test `test_shutdown_kills_subprocess` that sets shutdown flag and verifies subprocess terminates within 500ms [5 pts] (blocked by #1, #6, #7)
- [x] (#9) Add multi-instance isolation tests: test `test_multi_instance_isolation` with two ProcessRegistry instances verifying kill_all on one doesn't affect the other; test `test_process_registry_thread_safety` with concurrent register/unregister/kill_all operations verifying no race conditions [5 pts] (blocked by #2) (B)

## Follow-up tasks (from sprint review)
- [x] (#10) Implement the missing `src/process_group.rs` (`spawn_in_new_process_group`) and wire `claude.rs`/`codex.rs` spawns to use it; #4 is marked complete but the module/function aren’t present. (B)

## Follow-up tasks (from sprint review)
- [x] (#11) Update `ProcessRegistry::kill_all` on Unix to terminate the full process group for each registered PID (e.g., call `kill_process_tree` or signal `-pid`) so Ctrl+C shutdown doesn’t leave child processes running. (A)
