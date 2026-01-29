use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use crate::process::kill_process_tree;

use super::message::TuiMessage;
use super::run::run_tui;
use super::tail::tail_chat_to_tui;

#[cfg(unix)]
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(unix)]
const GRACEFUL_SHUTDOWN_POLL: Duration = Duration::from_millis(100);

fn graceful_stop_child(child_pid: u32, child: &mut std::process::Child) {
    // If the child already exited, there's nothing to do.
    if let Ok(Some(_)) = child.try_wait() {
        return;
    }

    #[cfg(unix)]
    {
        // Ask the swarm subprocess to shut down cleanly so it can terminate agents.
        unsafe {
            // Use the process group so only the swarm subprocess gets the signal.
            let pgid = -(child_pid as i32);
            libc::kill(pgid, libc::SIGINT);
        }

        let deadline = Instant::now() + GRACEFUL_SHUTDOWN_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    thread::sleep(GRACEFUL_SHUTDOWN_POLL);
                }
                Err(_) => break,
            }
        }
    }

    // Fall back to a hard kill if graceful shutdown doesn't complete in time.
    kill_process_tree(child_pid);
    let _ = child.kill();
    let _ = child.wait();
}

/// Run the TUI with a subprocess that does the actual work.
///
/// This spawns the swarm command as a subprocess to avoid stdout corruption.
/// The TUI only shows the chat file content (which the subprocess writes to).
pub fn run_tui_with_subprocess(
    chat_path: &str,
    args: Vec<String>,
    skip_chat_reset: bool,
) -> io::Result<()> {
    use std::process::{Command, Stdio};

    let (tx, rx) = mpsc::channel();
    let tx_clone = tx.clone();
    let chat_path = chat_path.to_string();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_for_tail = Arc::clone(&stop_flag);
    let stop_for_proc = Arc::clone(&stop_flag);

    let exe_path = std::env::current_exe()
        .map_err(|e| io::Error::other(format!("failed to get exe path: {}", e)))?;

    let proc_handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));

        // On Unix, create a new process group so we can kill all children
        #[cfg(unix)]
        let child_result = {
            use std::os::unix::process::CommandExt;
            let mut cmd = Command::new(&exe_path);
            cmd.args(&args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .env("SWARM_NO_TAIL", "1"); // TUI handles display, subprocess shouldn't tail
            if skip_chat_reset {
                cmd.env("SWARM_SKIP_CHAT_RESET", "1");
            }
            unsafe {
                cmd.pre_exec(|| {
                    // Create new process group with this process as leader
                    libc::setpgid(0, 0);
                    Ok(())
                })
                .spawn()
            }
        };

        #[cfg(not(unix))]
        let child_result = {
            let mut cmd = Command::new(&exe_path);
            cmd.args(&args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .env("SWARM_NO_TAIL", "1"); // TUI handles display, subprocess shouldn't tail
            if skip_chat_reset {
                cmd.env("SWARM_SKIP_CHAT_RESET", "1");
            }
            cmd.spawn()
        };

        let mut child = match child_result {
            Ok(c) => c,
            Err(e) => {
                let _ = tx_clone.send(TuiMessage::AppendLine(format!(
                    "\u{274c} Failed to start subprocess: {}",
                    e
                )));
                let _ = tx_clone.send(TuiMessage::WorkComplete);
                return;
            }
        };

        let child_pid = child.id();

        loop {
            if stop_for_proc.load(Ordering::SeqCst) {
                graceful_stop_child(child_pid, &mut child);
                break;
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        let _ = tx_clone.send(TuiMessage::AppendLine(
                            "\u{2705} Work complete! Press 'q' to exit.".to_string(),
                        ));
                    } else {
                        let _ = tx_clone.send(TuiMessage::AppendLine(format!(
                            "\u{274c} Process exited with status: {}",
                            status
                        )));
                    }
                    let _ = tx_clone.send(TuiMessage::WorkComplete);
                    break;
                }
                Ok(None) => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    let _ = tx_clone.send(TuiMessage::AppendLine(format!(
                        "\u{274c} Error waiting for process: {}",
                        e
                    )));
                    let _ = tx_clone.send(TuiMessage::WorkComplete);
                    break;
                }
            }
        }
    });

    let tx_for_tail = tx.clone();
    let tail_handle = thread::spawn(move || {
        tail_chat_to_tui(&chat_path, tx_for_tail, stop_for_tail);
    });

    let result = run_tui(rx);

    stop_flag.store(true, Ordering::SeqCst);

    let _ = proc_handle.join();
    let _ = tail_handle.join();

    result
}
