use std::collections::HashSet;
use std::sync::Mutex;

use once_cell::sync::Lazy;

/// Thread-safe registry of subprocess PIDs owned by this swarm instance.
pub struct ProcessRegistry {
    pids: Mutex<HashSet<u32>>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self {
            pids: Mutex::new(HashSet::new()),
        }
    }

    /// Register a spawned subprocess.
    pub fn register(&self, pid: u32) {
        self.pids.lock().unwrap().insert(pid);
    }

    /// Unregister a subprocess (after wait/reap).
    pub fn unregister(&self, pid: u32) {
        self.pids.lock().unwrap().remove(&pid);
    }

    /// Get all registered PIDs (for shutdown).
    pub fn all_pids(&self) -> Vec<u32> {
        self.pids.lock().unwrap().iter().copied().collect()
    }

    /// Kill all registered subprocesses (graceful then forced).
    pub fn kill_all(&self) {
        for pid in self.all_pids() {
            kill_pid_gracefully(pid);
        }
    }
}

impl Default for ProcessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global registry for the current swarm run.
pub static PROCESS_REGISTRY: Lazy<ProcessRegistry> = Lazy::new(ProcessRegistry::new);

#[cfg(unix)]
fn kill_pid_gracefully(pid: u32) {
    crate::process::kill_process_tree(pid);
}

#[cfg(windows)]
fn kill_pid_gracefully(pid: u32) {
    use std::process::Command;

    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

#[cfg(test)]
mod tests {
    use super::ProcessRegistry;

    #[test]
    fn register_unregister_tracks_pids() {
        let registry = ProcessRegistry::new();

        registry.register(100);
        registry.register(200);

        let mut pids = registry.all_pids();
        pids.sort_unstable();
        assert_eq!(pids, vec![100, 200]);

        registry.unregister(100);

        let mut pids = registry.all_pids();
        pids.sort_unstable();
        assert_eq!(pids, vec![200]);
    }

    #[test]
    fn kill_all_empty_no_panic() {
        let registry = ProcessRegistry::new();
        registry.kill_all();
    }

    #[cfg(unix)]
    #[test]
    fn kill_all_terminates_process_group() {
        use crate::process_group::spawn_in_new_process_group;
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        let registry = ProcessRegistry::new();

        let mut cmd = Command::new("sleep");
        cmd.arg("10")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut child = spawn_in_new_process_group(&mut cmd).expect("spawn sleep");
        let pid = child.id();
        registry.register(pid);

        registry.kill_all();

        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if start.elapsed() > Duration::from_secs(2) {
                        panic!("process still running after kill_all");
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(err) => panic!("try_wait failed: {}", err),
            }
        }
    }
}
