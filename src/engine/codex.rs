use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::config::EngineType;
use crate::process::kill_process_tree;
use crate::process_group::spawn_in_new_process_group;
use crate::process_registry::PROCESS_REGISTRY;
use crate::shutdown;

use super::{Engine, EngineResult};
use super::util::{build_agent_prompt, resolve_cli_path, WAIT_LOG_INTERVAL_SECS};

/// Codex CLI engine.
pub struct CodexEngine {
    /// Path to codex CLI binary.
    cli_path: String,
    /// Timeout in seconds (0 = no timeout).
    timeout_secs: u64,
}

impl CodexEngine {
    /// Create a new Codex engine with default timeout.
    /// Resolves the full path to codex using `which` for better portability.
    pub fn new() -> Self {
        let cli_path = resolve_cli_path("codex").unwrap_or_else(|| "codex".to_string());
        Self { cli_path, timeout_secs: 0 }
    }

    /// Create with custom CLI path.
    pub fn with_path(cli_path: impl Into<String>) -> Self {
        Self {
            cli_path: cli_path.into(),
            timeout_secs: 0,
        }
    }

    /// Create with timeout.
    pub fn with_timeout(timeout_secs: u64) -> Self {
        let cli_path = resolve_cli_path("codex").unwrap_or_else(|| "codex".to_string());
        Self { cli_path, timeout_secs }
    }
}

impl Default for CodexEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for CodexEngine {
    fn execute(
        &self,
        agent_name: &str,
        task_description: &str,
        working_dir: &Path,
        _turn_number: usize,
        team_dir: Option<&str>,
    ) -> EngineResult {
        // For valid agents, wrap in agent prompt; otherwise use raw prompt
        let prompt = match build_agent_prompt(agent_name, task_description, team_dir) {
            Ok(Some(p)) => p,
            Ok(None) => task_description.to_string(), // Non-agent (e.g., ScrumMaster)
            Err(e) => return EngineResult::failure(e, 1),
        };

        // Create debug file for streaming JSONL output
        let debug_file = team_dir.and_then(|dir| {
            let debug_path = Path::new(dir).join("loop").join(format!("codex-debug-{}.jsonl", agent_name));
            match File::create(&debug_path) {
                Ok(f) => {
                    eprintln!("[{}] Debug output: {}", agent_name, debug_path.display());
                    Some(f)
                }
                Err(e) => {
                    eprintln!("[{}] Warning: could not create debug file {}: {}", agent_name, debug_path.display(), e);
                    None
                }
            }
        });

        // Codex uses "exec" subcommand with stdin for prompts
        // Add --json flag for JSONL streaming output when debug file is available
        let mut cmd = Command::new(&self.cli_path);
        cmd.arg("exec");
        if debug_file.is_some() {
            cmd.arg("--json");  // Stream JSONL events for debugging
        }
        cmd.arg("--dangerously-bypass-approvals-and-sandbox")
            .arg("-")  // Read prompt from stdin
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match spawn_in_new_process_group(&mut cmd) {
            Ok(c) => c,
            Err(e) => return EngineResult::failure(format!("failed to spawn codex: {}", e), 1),
        };
        let pid = child.id();
        PROCESS_REGISTRY.register(pid);

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }

        // Take stdout and stderr for streaming
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Spawn thread to stream stdout to both debug file and buffer
        let stdout_handle = thread::spawn(move || {
            let mut output = String::new();
            if let Some(stdout) = stdout {
                let reader = BufReader::new(stdout);
                let mut debug_file = debug_file;
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            // Write to debug file if available
                            if let Some(ref mut f) = debug_file {
                                let _ = writeln!(f, "{}", line);
                                let _ = f.flush();
                            }
                            // Accumulate for result
                            output.push_str(&line);
                            output.push('\n');
                        }
                        Err(_) => break,
                    }
                }
            }
            output
        });

        // Spawn thread to capture stderr
        let stderr_handle = thread::spawn(move || {
            let mut output = String::new();
            if let Some(stderr) = stderr {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            output.push_str(&line);
                            output.push('\n');
                        }
                        Err(_) => break,
                    }
                }
            }
            output
        });

        let start = std::time::Instant::now();
        let log_interval = Duration::from_secs(WAIT_LOG_INTERVAL_SECS);
        let mut next_log = log_interval;
        let timeout = if self.timeout_secs > 0 {
            Some(Duration::from_secs(self.timeout_secs))
        } else {
            None
        };

        // Wait for completion, logging periodically
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process exited, collect output from threads
                    let stdout_output = stdout_handle.join().unwrap_or_default();
                    let stderr_output = stderr_handle.join().unwrap_or_default();
                    let exit_code = status.code().unwrap_or(1);
                    let _ = child.wait();
                    PROCESS_REGISTRY.unregister(pid);

                    let result = if status.success() {
                        EngineResult::success(stdout_output)
                    } else {
                        EngineResult::failure(stderr_output, exit_code)
                    };
                    PROCESS_REGISTRY.unregister(pid);
                    return result;
                }
                Ok(None) => {
                    let elapsed = start.elapsed();

                    if shutdown::requested() {
                        kill_process_tree(pid);
                        let _ = child.wait();
                        let _ = stdout_handle.join();
                        let _ = stderr_handle.join();
                        PROCESS_REGISTRY.unregister(pid);
                        return EngineResult::failure("Shutdown requested", 130);
                    }

                    // Check for timeout
                    if let Some(timeout_duration) = timeout {
                        if elapsed >= timeout_duration {
                            let _ = child.kill();
                            let _ = child.wait();
                            let _ = stdout_handle.join();
                            let _ = stderr_handle.join();
                            PROCESS_REGISTRY.unregister(pid);
                            let mins = elapsed.as_secs() / 60;
                            PROCESS_REGISTRY.unregister(pid);
                            return EngineResult::failure(
                                format!("agent timed out after {} minutes (pid {})", mins, pid),
                                124, // Standard timeout exit code
                            );
                        }
                    }

                    if elapsed >= next_log {
                        let mins = elapsed.as_secs() / 60;
                        let timeout_msg = if let Some(t) = timeout {
                            format!(", timeout in {} min", (t.as_secs() - elapsed.as_secs()) / 60)
                        } else {
                            String::new()
                        };
                        eprintln!("[{}] Still executing... ({} min elapsed, pid {}{})", agent_name, mins, pid, timeout_msg);
                        next_log += log_interval;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    let _ = child.wait();
                    PROCESS_REGISTRY.unregister(pid);
                    return EngineResult::failure(format!("failed to wait for codex: {}", e), 1);
                }
            }
        }
    }

    fn engine_type(&self) -> EngineType {
        EngineType::Codex
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codex_engine_type() {
        let engine = CodexEngine::new();
        assert_eq!(engine.engine_type(), EngineType::Codex);
    }

    #[test]
    fn test_codex_engine_with_timeout() {
        let engine = CodexEngine::with_timeout(1800);
        assert_eq!(engine.timeout_secs, 1800);
        assert_eq!(engine.engine_type(), EngineType::Codex);
    }

    #[cfg(unix)]
    #[test]
    fn test_codex_engine_shutdown_requested() {
        use std::fs;
        use std::fs::File;
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        use tempfile::TempDir;

        let _cwd_guard = crate::testutil::CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = crate::shutdown::test_lock();
        crate::shutdown::reset();

        let cwd = std::env::current_dir().expect("current dir");
        let temp = TempDir::new_in(cwd).expect("temp dir");
        let script_path = temp.path().join("fake-codex.sh");
        let mut file = File::create(&script_path).expect("create script");
        writeln!(file, "#!/bin/sh").expect("write shebang");
        writeln!(file, "cat >/dev/null").expect("write stdin drain");
        writeln!(file, "sleep 5").expect("write sleep");
        drop(file);

        let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod");

        crate::shutdown::request();
        let engine = CodexEngine::with_path(script_path.to_string_lossy().to_string());
        let result = engine.execute("Aaron", "test shutdown", temp.path(), 0, None);
        crate::shutdown::reset();

        assert!(!result.success);
        assert_eq!(result.exit_code, 130, "unexpected result: {:?}", result);
        assert_eq!(result.error.as_deref(), Some("Shutdown requested"));
    }
}
