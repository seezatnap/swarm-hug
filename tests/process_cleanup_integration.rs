#![cfg(unix)]

use std::collections::HashSet;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use swarm::engine::{ClaudeEngine, Engine};
use swarm::process_registry::PROCESS_REGISTRY;
use swarm::shutdown;

static TEST_LOCK: Mutex<()> = Mutex::new(());

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: String) -> Self {
        let previous = env::var(key).ok();
        env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => env::set_var(self.key, value),
            None => env::remove_var(self.key),
        }
    }
}

struct CleanupGuard;

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        shutdown::reset();
        PROCESS_REGISTRY.kill_all();
    }
}

fn write_sleep_script(dir: &Path, name: &str, seconds: u64) -> PathBuf {
    let path = dir.join(name);
    let mut file = File::create(&path).expect("create script");
    writeln!(file, "#!/bin/sh").expect("write shebang");
    writeln!(file, "cat >/dev/null").expect("write stdin drain");
    writeln!(file, "sleep {}", seconds).expect("write sleep");
    drop(file);

    let mut perms = fs::metadata(&path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).expect("chmod");

    path
}

fn assert_child_reaped(pid: u32, context: &str) {
    let mut status: libc::c_int = 0;
    let result = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
    if result == 0 {
        panic!("{}: subprocess still running (pid {})", context, pid);
    }
    if result == pid as i32 {
        panic!("{}: subprocess left zombie (pid {})", context, pid);
    }
    if result == -1 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ECHILD) {
            return;
        }
        panic!("{}: waitpid failed: {}", context, err);
    }
}

fn parse_pid_from_timeout(message: &str) -> Option<u32> {
    let marker = "pid ";
    let idx = message.rfind(marker)?;
    let rest = &message[idx + marker.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn sorted_pids() -> Vec<u32> {
    let mut pids = PROCESS_REGISTRY.all_pids();
    pids.sort_unstable();
    pids
}

fn wait_for_new_pid(
    existing: &HashSet<u32>,
    rx: &std::sync::mpsc::Receiver<swarm::engine::EngineResult>,
    timeout: Duration,
) -> u32 {
    let start = Instant::now();
    loop {
        match rx.try_recv() {
            Ok(result) => panic!("engine exited before shutdown: {:?}", result),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                panic!("engine thread ended before shutdown")
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        let current = PROCESS_REGISTRY.all_pids();
        if let Some(pid) = current.into_iter().find(|pid| !existing.contains(pid)) {
            return pid;
        }
        if start.elapsed() > timeout {
            panic!("timed out waiting for subprocess to register");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn test_engine_timeout_no_zombie() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _cleanup = CleanupGuard;
    shutdown::reset();

    let cwd = env::current_dir().expect("current dir");
    let temp = TempDir::new_in(&cwd).expect("temp dir");
    let _script_path = write_sleep_script(temp.path(), "claude", 5);

    let original_path = env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", temp.path().display(), original_path);
    let _path_guard = EnvVarGuard::set("PATH", new_path);

    let before = sorted_pids();
    let engine = ClaudeEngine::with_timeout(1);
    let result = engine.execute("Aaron", "timeout test", temp.path(), 0, None);
    let after = sorted_pids();

    assert_eq!(before, after, "process registry should be restored after timeout");
    assert!(!result.success, "expected timeout failure, got {:?}", result);
    assert_eq!(result.exit_code, 124, "unexpected result: {:?}", result);

    let error = result.error.as_deref().expect("expected timeout error");
    assert!(error.contains("timed out"), "unexpected error: {}", error);
    let pid = parse_pid_from_timeout(error).expect("expected pid in timeout error");

    assert_child_reaped(pid, "timeout");
}

#[test]
fn test_shutdown_kills_subprocess() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _cleanup = CleanupGuard;
    shutdown::reset();

    let cwd = env::current_dir().expect("current dir");
    let temp = TempDir::new_in(&cwd).expect("temp dir");
    let script_path = write_sleep_script(temp.path(), "fake-claude.sh", 5);

    let before = sorted_pids();
    let before_set: HashSet<u32> = before.iter().copied().collect();

    let engine = ClaudeEngine::with_path(script_path.to_string_lossy().to_string());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let result = engine.execute("Aaron", "shutdown test", temp.path(), 0, None);
        let _ = tx.send(result);
    });

    let pid = wait_for_new_pid(&before_set, &rx, Duration::from_secs(1));

    shutdown::request();
    let start = Instant::now();
    let result = rx
        .recv_timeout(Duration::from_millis(500))
        .expect("engine did not shut down within 500ms");
    let elapsed = start.elapsed();
    handle.join().expect("engine thread panicked");

    assert!(
        elapsed <= Duration::from_millis(500),
        "shutdown exceeded 500ms: {:?}",
        elapsed
    );
    assert!(!result.success, "expected shutdown failure, got {:?}", result);
    assert_eq!(result.exit_code, 130, "unexpected result: {:?}", result);
    assert_eq!(result.error.as_deref(), Some("Shutdown requested"));

    let after = sorted_pids();
    assert_eq!(before, after, "process registry should be restored after shutdown");

    assert_child_reaped(pid, "shutdown");
}
