# PRD: Agent Process Lifecycle Management

## Summary
Fix critical issues with subprocess cleanup that lead to zombie processes, orphaned subprocesses, and incomplete graceful shutdown. The current implementation has several gaps that allow `claude` and `codex` subprocesses to survive parent termination, accumulate as zombies, and ignore shutdown signals.

## Problem Statement
Users observe multiple orphaned `claude` and `codex` processes consuming resources after swarm exits or times out. Additionally, multiple swarm instances may run concurrently on the same system, requiring careful tracking of which subprocesses belong to which swarm invocation. Root causes identified:

1. **Zombie processes on timeout**: Engines call `child.kill()` but never `child.wait()`, leaving zombies
2. **Orphaned subprocesses in plain text mode**: No process group management for CLI subprocesses
3. **Incomplete graceful shutdown**: Ctrl+C doesn't terminate running subprocesses
4. **Codex streaming thread leaks**: Stdout/stderr reader threads not joined on timeout
5. **No signal escalation**: Uses SIGKILL immediately instead of graceful SIGTERM first

## Goals
- Ensure all spawned subprocesses are properly reaped (no zombies)
- Enable clean process tree termination on shutdown
- Implement graceful signal escalation (SIGTERM → SIGKILL)
- Propagate shutdown signals to running engine subprocesses
- Unify process group management across TUI and plain text modes

## Non-goals
- Changes to agent task distribution logic
- Changes to worktree management
- Cross-platform Windows improvements (focus on Unix first)

## Multi-Instance Consideration

Multiple swarm invocations can run concurrently on the same system (e.g., different projects, different terminals). This creates a critical requirement: **we must only terminate subprocesses belonging to our specific swarm instance**, not subprocesses from other concurrent runs.

### Current State
- No tracking of which PIDs belong to which swarm instance
- Using `pkill -f claude` or similar would kill ALL claude processes system-wide
- Process groups help isolate children but aren't tracked across the swarm lifecycle

### Required: Process Registry

Each swarm instance must maintain a registry of spawned subprocess PIDs:

```rust
// New: src/process_registry.rs
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Thread-safe registry of subprocess PIDs owned by this swarm instance
pub struct ProcessRegistry {
    pids: Mutex<HashSet<u32>>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self { pids: Mutex::new(HashSet::new()) }
    }

    /// Register a spawned subprocess
    pub fn register(&self, pid: u32) {
        self.pids.lock().unwrap().insert(pid);
    }

    /// Unregister a subprocess (after wait/reap)
    pub fn unregister(&self, pid: u32) {
        self.pids.lock().unwrap().remove(&pid);
    }

    /// Get all registered PIDs (for shutdown)
    pub fn all_pids(&self) -> Vec<u32> {
        self.pids.lock().unwrap().iter().copied().collect()
    }

    /// Kill all registered subprocesses (graceful then forced)
    pub fn kill_all(&self) {
        for pid in self.all_pids() {
            kill_pid_gracefully(pid);
        }
    }
}

// Global instance per swarm run
lazy_static! {
    pub static ref PROCESS_REGISTRY: ProcessRegistry = ProcessRegistry::new();
}
```

### Integration Points

1. **Engine spawn**: Register PID immediately after `Command::spawn()`
2. **Engine completion**: Unregister PID after `child.wait()`
3. **Shutdown handler**: Call `PROCESS_REGISTRY.kill_all()` on Ctrl+C
4. **Timeout handler**: Already tracked, just ensure unregister after kill

### Why Not Just Process Groups?

Process groups solve parent-child isolation but have limitations:
- A swarm instance doesn't know the PIDs in other instances' process groups
- `kill(-pgid)` requires knowing the PGID, which we already have (the child's PID when using `setpgid(0,0)`)
- The registry provides an explicit list for logging, debugging, and selective termination
- Registry survives across the swarm lifecycle even if individual engines are replaced

## Current Architecture

```
swarm process
├─ Agent A thread (Rust thread)
│   └─ claude subprocess [spawned via Command::new()]
├─ Agent B thread
│   └─ claude subprocess
└─ Agent C thread
    └─ codex subprocess
        ├─ stdout reader thread
        └─ stderr reader thread
```

**Key files:**
- `src/engine/claude.rs:120-122` - Timeout kill logic (missing wait)
- `src/engine/codex.rs:182-184` - Timeout kill logic (missing wait)
- `src/tui/process.rs:11-41` - TUI process tree kill (Unix only)
- `src/shutdown.rs` - Graceful shutdown handler (doesn't kill subprocesses)
- `src/runner.rs:350` - Agent thread spawning

## Proposed Changes

### 1. Fix Zombie Processes (Critical)

**Files:** `src/engine/claude.rs`, `src/engine/codex.rs`

Add `child.wait()` after every `child.kill()`:

```rust
// Current (buggy)
if elapsed >= timeout_duration {
    let _ = child.kill();
    return EngineResult::failure(...);
}

// Proposed (fixed)
if elapsed >= timeout_duration {
    let _ = child.kill();
    let _ = child.wait(); // Reap zombie
    return EngineResult::failure(...);
}
```

### 2. Add Process Group Management for All Modes

**Files:** `src/engine/claude.rs`, `src/engine/codex.rs`, new `src/process_group.rs`

Create a new process group for each subprocess on Unix:

```rust
// New helper module: src/process_group.rs
#[cfg(unix)]
pub fn spawn_in_new_process_group(cmd: &mut Command) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0); // Create new process group
            Ok(())
        })
    }
    .spawn()
}

#[cfg(windows)]
pub fn spawn_in_new_process_group(cmd: &mut Command) -> io::Result<Child> {
    cmd.spawn() // Windows handles this differently via CREATE_NEW_PROCESS_GROUP
}
```

Update engine spawn calls to use this helper.

### 3. Implement Signal Escalation

**Files:** `src/engine/claude.rs`, `src/engine/codex.rs`

Replace immediate SIGKILL with graceful escalation:

```rust
fn kill_subprocess(child: &mut Child) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        let pid = Pid::from_raw(child.id() as i32);

        // 1. Try SIGTERM first (graceful)
        let _ = kill(pid, Signal::SIGTERM);

        // 2. Wait briefly for graceful exit
        std::thread::sleep(Duration::from_millis(100));

        // 3. Check if still running
        if child.try_wait().ok().flatten().is_none() {
            // 4. Force kill if still alive
            let _ = child.kill(); // SIGKILL
        }

        // 5. Always reap
        let _ = child.wait();
    }

    #[cfg(windows)]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}
```

### 4. Propagate Shutdown to Running Subprocesses

**Files:** `src/engine/claude.rs`, `src/engine/codex.rs`, `src/shutdown.rs`

Add shutdown check in engine polling loop and kill subprocess when triggered:

```rust
// In engine execute() polling loop
loop {
    // Check for shutdown signal
    if shutdown::requested() {
        kill_subprocess(&mut child);
        return EngineResult::failure("Shutdown requested", 130);
    }

    // Existing timeout check
    if elapsed >= timeout_duration {
        kill_subprocess(&mut child);
        return EngineResult::failure("Timeout", 124);
    }

    // Existing try_wait logic
    match child.try_wait() { ... }
}
```

### 5. Fix Codex Streaming Thread Cleanup

**File:** `src/engine/codex.rs`

Join streaming threads before returning on timeout:

```rust
if elapsed >= timeout_duration {
    kill_subprocess(&mut child);

    // Join threads to prevent resource leak
    let stdout_result = stdout_handle.join().unwrap_or_default();
    let stderr_result = stderr_handle.join().unwrap_or_default();

    return EngineResult::failure(...);
}
```

### 6. Integrate Process Registry in Engines

**Files:** `src/engine/claude.rs`, `src/engine/codex.rs`, `src/process_registry.rs`

Register PIDs on spawn, unregister on completion:

```rust
// In engine execute()
let mut child = spawn_in_new_process_group(&mut cmd)?;
let pid = child.id();
PROCESS_REGISTRY.register(pid);  // Track this subprocess

// ... polling loop ...

// On any exit path (success, timeout, shutdown, error):
let _ = child.wait();
PROCESS_REGISTRY.unregister(pid);  // Remove from tracking
```

Wire registry to shutdown handler:

```rust
// In src/shutdown.rs
pub fn handle_shutdown() {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);

    // Kill all subprocesses owned by this swarm instance
    PROCESS_REGISTRY.kill_all();
}
```

### 7. Unify TUI and Plain Text Process Management

**Files:** `src/tui/process.rs`, `src/runner.rs`

Extract shared `kill_process_tree()` to a common module and use it in both modes:

```rust
// New: src/process.rs
pub fn kill_process_tree(pid: u32) {
    #[cfg(unix)]
    {
        use std::process::Command;

        // SIGTERM to process group
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{}", pid)])
            .status();

        std::thread::sleep(Duration::from_millis(100));

        // SIGKILL to process group
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{}", pid)])
            .status();

        // SIGKILL to children (belt and suspenders)
        let _ = Command::new("pkill")
            .args(["-KILL", "-P", &pid.to_string()])
            .status();
    }

    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
}
```

## Implementation Plan

### Phase 1: Critical Fixes (Immediate)
1. Add `child.wait()` after all `child.kill()` calls in both claude.rs and codex.rs
2. Fix Codex streaming thread joins on timeout

### Phase 2: Process Registry
3. Create `src/process_registry.rs` with thread-safe PID tracking
4. Register PIDs on spawn in both claude and codex engines
5. Unregister PIDs after wait/reap
6. Wire registry to shutdown handler

### Phase 3: Process Group Management
7. Create `src/process_group.rs` helper module
8. Update engine spawn calls to use process groups (both engines)
9. Extract `kill_process_tree()` to shared module

### Phase 4: Signal Propagation
10. Add shutdown check to engine polling loops (both engines)
11. Implement graceful signal escalation (SIGTERM → SIGKILL)
12. Call `PROCESS_REGISTRY.kill_all()` from shutdown handler

### Phase 5: Testing
13. Add integration tests for subprocess cleanup
14. Test multi-instance isolation (two swarm runs, kill one, other survives)
15. Verify no zombie processes after timeout
16. Verify clean shutdown on Ctrl+C

## Testing Plan

### Manual Tests
1. Run swarm with long-running tasks, press Ctrl+C → verify all claude/codex processes terminate
2. Set short `--agent-timeout`, wait for timeout → verify no zombie processes
3. Kill swarm process with SIGTERM → verify subprocess tree dies
4. Run `ps aux | grep -E 'claude|codex'` after various exit scenarios → should show no orphans
5. **Multi-instance test**: Run two swarm instances (different projects), kill one with Ctrl+C → verify only that instance's subprocesses die, other instance continues normally

### Automated Tests
```rust
#[test]
fn test_engine_timeout_no_zombie() {
    // Spawn engine with 1-second timeout
    // Run long-running subprocess
    // Wait for timeout
    // Assert: ps shows no zombie with our test subprocess PID
}

#[test]
fn test_shutdown_kills_subprocess() {
    // Start engine execution
    // Set shutdown flag
    // Assert: subprocess terminates within 500ms
    // Assert: engine returns shutdown error
}

#[test]
fn test_multi_instance_isolation() {
    // Create two ProcessRegistry instances (simulating two swarm runs)
    // Register different PIDs in each
    // Call kill_all() on registry A
    // Assert: only PIDs from registry A are targeted
    // Assert: registry B PIDs are untouched
}

#[test]
fn test_process_registry_thread_safety() {
    // Spawn multiple threads registering/unregistering PIDs
    // Concurrent kill_all() call
    // Assert: no race conditions, no panics
}
```

## Acceptance Criteria
- [ ] No zombie processes after engine timeout (both claude and codex)
- [ ] Ctrl+C terminates all running claude/codex subprocesses within 1 second
- [ ] Parent crash/kill terminates subprocess tree (Unix)
- [ ] Multi-instance isolation: killing one swarm instance doesn't affect another
- [ ] Process registry correctly tracks all spawned PIDs
- [ ] `cargo test` passes with new subprocess cleanup tests
- [ ] `cargo clippy` reports no new warnings
- [ ] Codex engine stdout/stderr threads are joined on all exit paths

## Risks
- **Signal escalation timing**: 100ms may be too short for claude/codex to clean up gracefully. May need tuning or configuration.
- **Process group edge cases**: Some subprocesses may spawn their own children outside the process group (e.g., claude spawning MCP servers).
- **Windows compatibility**: `setpgid` is Unix-only; Windows behavior may differ.
- **Registry synchronization**: If a subprocess dies unexpectedly (e.g., OOM kill), its PID may remain in the registry until the next spawn/reap cycle. Periodic cleanup may be needed.
- **PID reuse**: On long-running systems, PIDs can be reused. Registry should unregister PIDs promptly after reaping to avoid stale entries.

## Dependencies
- May need `nix` crate for clean signal handling (optional; can use libc directly)
- `lazy_static` or `once_cell` for global process registry (likely already in deps)
- No other new dependencies required

## Metrics
- Monitor for orphaned `claude`/`codex` processes after swarm sessions
- Track timeout-related zombie accumulation over long runs
- Log registry size at shutdown (indicates cleanup effectiveness)
- Track cases where PIDs remain after kill (indicates cleanup failures)
