use std::io;
use std::process::{Child, Command};

/// Spawn a command in a new process group when supported.
#[cfg(unix)]
pub fn spawn_in_new_process_group(cmd: &mut Command) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;

    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }

    cmd.spawn()
}

/// Spawn a command on Windows (process groups are handled differently).
#[cfg(windows)]
pub fn spawn_in_new_process_group(cmd: &mut Command) -> io::Result<Child> {
    cmd.spawn()
}

#[cfg(test)]
mod tests {
    use super::spawn_in_new_process_group;

    #[cfg(unix)]
    #[test]
    fn spawn_creates_new_process_group() {
        use std::process::{Command, Stdio};

        let mut cmd = Command::new("sleep");
        cmd.arg("10")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut child = spawn_in_new_process_group(&mut cmd).expect("spawn sleep");
        let pid = child.id() as i32;

        let pgid = unsafe { libc::getpgid(pid) };
        assert!(pgid >= 0, "getpgid failed");
        assert_eq!(pgid, pid);

        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(windows)]
    #[test]
    fn spawn_works_on_windows() {
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
