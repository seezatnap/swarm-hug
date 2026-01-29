/// Kill a process and all its children (process group).
#[cfg(unix)]
pub fn kill_process_tree(pid: u32) {
    use std::thread;
    use std::time::Duration;

    let pgid = -(pid as i32);

    // First try SIGTERM to the process group (negative PID).
    unsafe {
        libc::kill(pgid, libc::SIGTERM);
    }

    // Give processes a moment to clean up.
    thread::sleep(Duration::from_millis(100));

    // Then SIGKILL to make sure everything is dead.
    unsafe {
        libc::kill(pgid, libc::SIGKILL);
    }

    // Also kill direct children that might have escaped.
    let _ = std::process::Command::new("pkill")
        .args(["-KILL", "-P", &pid.to_string()])
        .status();
}

/// Kill a process tree on Windows using taskkill.
#[cfg(windows)]
pub fn kill_process_tree(pid: u32) {
    use std::process::Command;

    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

#[cfg(test)]
mod tests {
    use super::kill_process_tree;

    #[cfg(unix)]
    #[test]
    fn kill_process_tree_terminates_process_group() {
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        let mut cmd = Command::new("sleep");
        cmd.arg("10")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }

        let mut child = cmd.spawn().expect("spawn sleep");
        let pid = child.id();

        kill_process_tree(pid);

        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if start.elapsed() > Duration::from_secs(2) {
                        panic!("process still running after kill_process_tree");
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(err) => panic!("try_wait failed: {}", err),
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn kill_process_tree_terminates_process() {
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        let mut child = Command::new("cmd")
            .args(["/C", "ping 127.0.0.1 -n 6 > NUL"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ping");

        let pid = child.id();
        kill_process_tree(pid);

        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if start.elapsed() > Duration::from_secs(2) {
                        panic!("process still running after kill_process_tree");
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(err) => panic!("try_wait failed: {}", err),
            }
        }
    }
}
