use std::io;
use std::process::{Child, Command};

/// Spawn a subprocess in a new process group (Unix) for easier cleanup.
#[cfg(unix)]
pub fn spawn_in_new_process_group(cmd: &mut Command) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;

    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    cmd.spawn()
}

/// Spawn a subprocess on Windows (process groups handled differently).
#[cfg(windows)]
pub fn spawn_in_new_process_group(cmd: &mut Command) -> io::Result<Child> {
    cmd.spawn()
}

#[cfg(test)]
mod tests {
    use super::spawn_in_new_process_group;

    #[cfg(unix)]
    #[test]
    fn spawn_in_new_process_group_sets_pgid() {
        use std::io;
        use std::process::{Command, Stdio};

        let mut cmd = Command::new("sleep");
        cmd.arg("5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut child = spawn_in_new_process_group(&mut cmd).expect("spawn sleep");
        let pid = child.id() as i32;

        let pgid = unsafe { libc::getpgid(pid) };
        if pgid < 0 {
            panic!("getpgid failed: {}", io::Error::last_os_error());
        }
        assert_eq!(pgid, pid);

        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(windows)]
    #[test]
    fn spawn_in_new_process_group_spawns() {
        use std::process::{Command, Stdio};

        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "exit", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut child = spawn_in_new_process_group(&mut cmd).expect("spawn cmd");
        let status = child.wait().expect("wait cmd");
        assert!(status.success());
    }
}
