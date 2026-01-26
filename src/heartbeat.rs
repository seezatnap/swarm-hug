//! Heartbeat logging for long-running agent tasks.
//!
//! Emits periodic "agent activity" messages to chat while a task is running.

use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

use crate::chat;

const DEFAULT_HEARTBEAT_INTERVAL_SECS: u64 = 300;

/// Default heartbeat interval (5 minutes).
pub fn default_interval() -> Duration {
    Duration::from_secs(DEFAULT_HEARTBEAT_INTERVAL_SECS)
}

/// A guard that logs heartbeat messages until dropped or stopped.
pub struct HeartbeatGuard {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl HeartbeatGuard {
    /// Start a heartbeat logger for a running task.
    pub fn start<P: AsRef<Path>>(
        path: P,
        agent_name: &str,
        task_description: &str,
        interval: Duration,
    ) -> Self {
        if interval.is_zero() {
            return Self {
                stop: Arc::new(AtomicBool::new(true)),
                handle: None,
            };
        }

        let chat_path = path.as_ref().to_path_buf();
        let agent_name = agent_name.to_string();
        let task_description = task_description.to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        let handle = thread::spawn(move || {
            let start = Instant::now();
            let mut next_log = interval;
            let tick = interval.min(Duration::from_millis(100));

            loop {
                if stop_clone.load(Ordering::SeqCst) {
                    break;
                }

                let elapsed = start.elapsed();
                if elapsed >= next_log {
                    let msg = format_heartbeat_message(&task_description, elapsed);
                    if let Err(e) = chat::write_heartbeat(&chat_path, &agent_name, &msg) {
                        eprintln!("warning: failed to write heartbeat: {}", e);
                    }
                    next_log += interval;
                }

                thread::sleep(tick);
            }
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    /// Stop the heartbeat logger and wait for it to finish.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for HeartbeatGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn format_heartbeat_message(task_description: &str, elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs == 0 {
        let ms = elapsed.as_millis();
        format!(
            "Still working on \"{}\" ({} ms elapsed)",
            task_description, ms
        )
    } else if secs < 60 {
        format!(
            "Still working on \"{}\" ({} sec elapsed)",
            task_description, secs
        )
    } else {
        let mins = secs / 60;
        format!(
            "Still working on \"{}\" ({} min elapsed)",
            task_description, mins
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;

    use tempfile::NamedTempFile;

    #[test]
    fn heartbeat_logs_and_stops() {
        let tmp = NamedTempFile::new().unwrap();
        let interval = Duration::from_millis(100);

        let guard = HeartbeatGuard::start(
            tmp.path(),
            "Aaron",
            "Test task",
            interval,
        );

        thread::sleep(interval * 4);
        guard.stop();

        let content = fs::read_to_string(tmp.path()).unwrap();
        let heartbeat_count = content
            .lines()
            .filter(|line| chat::is_heartbeat_line(line))
            .count();
        assert!(heartbeat_count >= 2, "expected multiple heartbeats");

        thread::sleep(interval * 2);
        let content_after = fs::read_to_string(tmp.path()).unwrap();
        let heartbeat_count_after = content_after
            .lines()
            .filter(|line| chat::is_heartbeat_line(line))
            .count();
        assert_eq!(heartbeat_count, heartbeat_count_after);
    }
}
