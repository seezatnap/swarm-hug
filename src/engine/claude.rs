use std::env;
use std::io::Write;
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
use super::util::{build_agent_prompt, output_to_result, resolve_cli_path, WAIT_LOG_INTERVAL_SECS};

#[derive(Debug, Clone)]
struct OpenRouterConfig {
    model: String,
}

/// Claude CLI engine.
pub struct ClaudeEngine {
    /// Path to claude CLI binary.
    cli_path: String,
    /// Timeout in seconds (0 = no timeout).
    timeout_secs: u64,
    /// Optional OpenRouter configuration.
    openrouter: Option<OpenRouterConfig>,
}

impl ClaudeEngine {
    /// Create a new Claude engine with default timeout.
    /// Resolves the full path to claude using `which` for better portability.
    pub fn new() -> Self {
        let cli_path = resolve_cli_path("claude").unwrap_or_else(|| "claude".to_string());
        Self {
            cli_path,
            timeout_secs: 0,
            openrouter: None,
        }
    }

    /// Create with custom CLI path.
    pub fn with_path(cli_path: impl Into<String>) -> Self {
        Self {
            cli_path: cli_path.into(),
            timeout_secs: 0,
            openrouter: None,
        }
    }

    /// Create with timeout.
    pub fn with_timeout(timeout_secs: u64) -> Self {
        let cli_path = resolve_cli_path("claude").unwrap_or_else(|| "claude".to_string());
        Self {
            cli_path,
            timeout_secs,
            openrouter: None,
        }
    }

    /// Enable OpenRouter mode with the given model.
    pub fn with_openrouter_model(mut self, model: impl Into<String>) -> Self {
        self.openrouter = Some(OpenRouterConfig { model: model.into() });
        self
    }
}

impl Default for ClaudeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for ClaudeEngine {
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

        // Use stdin for prompt to avoid "Argument list too long" (E2BIG) errors
        // when prompts exceed the OS argument size limit (~256KB on macOS)
        let mut cmd = Command::new(&self.cli_path);
        cmd.arg("--dangerously-skip-permissions")
            .arg("--print")
            .arg("-p")
            .arg("-")  // Read prompt from stdin
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set CLAUDE_CODE_TASK_LIST_ID to team name when team_dir is provided
        // team_dir is like ".swarm-hug/greenfield", extract just "greenfield"
        if let Some(dir) = team_dir {
            if let Some(team_name) = Path::new(dir).file_name().and_then(|n| n.to_str()) {
                cmd.env("CLAUDE_CODE_TASK_LIST_ID", team_name);
            }
        }

        if let Err(result) = self.apply_openrouter_env(&mut cmd) {
            return result;
        }

        let mut child = match spawn_in_new_process_group(&mut cmd) {
            Ok(c) => c,
            Err(e) => return EngineResult::failure(format!("failed to spawn claude: {}", e), 1),
        };
        let pid = child.id();
        PROCESS_REGISTRY.register(pid);

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }

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
                Ok(Some(_status)) => {
                    match child.wait_with_output() {
                        Ok(output) => {
                            let result = output_to_result(output);
                            PROCESS_REGISTRY.unregister(pid);
                            return result;
                        }
                        Err(e) => {
                            PROCESS_REGISTRY.unregister(pid);
                            return EngineResult::failure(format!("failed to get output: {}", e), 1);
                        }
                    }
                }
                Ok(None) => {
                    // Process still running
                    let elapsed = start.elapsed();

                    if shutdown::requested() {
                        kill_process_tree(pid);
                        let _ = child.wait();
                        PROCESS_REGISTRY.unregister(pid);
                        return EngineResult::failure("Shutdown requested", 130);
                    }

                    // Check for timeout
                    if let Some(timeout_duration) = timeout {
                        if elapsed >= timeout_duration {
                            let _ = child.kill();
                            let _ = child.wait();
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
                    return EngineResult::failure(format!("failed to wait for claude: {}", e), 1);
                }
            }
        }
    }

    fn engine_type(&self) -> EngineType {
        match &self.openrouter {
            Some(config) => EngineType::OpenRouter { model: config.model.clone() },
            None => EngineType::Claude,
        }
    }
}

impl ClaudeEngine {
    fn apply_openrouter_env(&self, cmd: &mut Command) -> Result<(), EngineResult> {
        let config = match &self.openrouter {
            Some(cfg) => cfg,
            None => return Ok(()),
        };

        let model = config.model.trim();
        if model.is_empty() {
            return Err(EngineResult::failure(
                "openrouter engine requires a model (e.g., openrouter_moonshotai/kimi-k2.5)",
                1,
            ));
        }

        let api_key = match env::var("OPENROUTER_API_KEY") {
            Ok(val) => {
                let trimmed = val.trim();
                if trimmed.is_empty() {
                    return Err(EngineResult::failure(
                        "OPENROUTER_API_KEY must be set when using openrouter engines",
                        1,
                    ));
                }
                trimmed.to_string()
            }
            Err(_) => {
                return Err(EngineResult::failure(
                    "OPENROUTER_API_KEY must be set when using openrouter engines",
                    1,
                ))
            }
        };

        cmd.env("ANTHROPIC_BASE_URL", "https://openrouter.ai/api");
        cmd.env("ANTHROPIC_AUTH_TOKEN", api_key);
        cmd.env("ANTHROPIC_API_KEY", "");
        cmd.env("ANTHROPIC_SMALL_FAST_MODEL", model);
        cmd.env("ANTHROPIC_MODEL", model);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(unix)]
    use std::sync::Mutex;

    #[cfg(unix)]
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    #[cfg(unix)]
    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    #[cfg(unix)]
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn test_claude_engine_type() {
        let engine = ClaudeEngine::new();
        assert_eq!(engine.engine_type(), EngineType::Claude);
    }

    #[test]
    fn test_claude_engine_with_timeout() {
        let engine = ClaudeEngine::with_timeout(1800);
        assert_eq!(engine.timeout_secs, 1800);
        assert_eq!(engine.engine_type(), EngineType::Claude);
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_engine_openrouter_missing_api_key() {
        let _shutdown_guard = crate::shutdown::test_lock();
        crate::shutdown::reset();
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _unset = EnvVarGuard::unset("OPENROUTER_API_KEY");
        let engine = ClaudeEngine::with_path("missing-claude")
            .with_openrouter_model("moonshotai/kimi-k2.5");
        let result = engine.execute("ScrumMaster", "openrouter missing key", Path::new("."), 0, None);
        assert!(!result.success, "expected failure");
        assert_eq!(
            result.error.as_deref(),
            Some("OPENROUTER_API_KEY must be set when using openrouter engines")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_engine_openrouter_env_vars() {
        use std::collections::HashMap;
        use std::fs;
        use std::fs::File;
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        use tempfile::TempDir;

        let _shutdown_guard = crate::shutdown::test_lock();
        crate::shutdown::reset();
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _key_guard = EnvVarGuard::set("OPENROUTER_API_KEY", "test-openrouter-key");

        let before_base_url = std::env::var("ANTHROPIC_BASE_URL").ok();
        let before_auth = std::env::var("ANTHROPIC_AUTH_TOKEN").ok();
        let before_api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        let before_small = std::env::var("ANTHROPIC_SMALL_FAST_MODEL").ok();
        let before_model = std::env::var("ANTHROPIC_MODEL").ok();

        let cwd = std::env::current_dir().expect("current dir");
        let temp = TempDir::new_in(cwd).expect("temp dir");
        let script_path = temp.path().join("fake-claude.sh");
        let mut file = File::create(&script_path).expect("create script");
        writeln!(file, "#!/bin/sh").expect("write shebang");
        writeln!(file, "cat >/dev/null").expect("write stdin drain");
        writeln!(file, "echo \"ANTHROPIC_BASE_URL=$ANTHROPIC_BASE_URL\"").expect("write base url");
        writeln!(file, "echo \"ANTHROPIC_AUTH_TOKEN=$ANTHROPIC_AUTH_TOKEN\"").expect("write auth token");
        writeln!(file, "echo \"ANTHROPIC_API_KEY=$ANTHROPIC_API_KEY\"").expect("write api key");
        writeln!(file, "echo \"ANTHROPIC_SMALL_FAST_MODEL=$ANTHROPIC_SMALL_FAST_MODEL\"").expect("write small model");
        writeln!(file, "echo \"ANTHROPIC_MODEL=$ANTHROPIC_MODEL\"").expect("write model");
        drop(file);

        let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod");

        let engine = ClaudeEngine::with_path(script_path.to_string_lossy().to_string())
            .with_openrouter_model("moonshotai/kimi-k2.5");
        let result = engine.execute("Aaron", "openrouter env test", temp.path(), 0, None);
        assert!(result.success, "engine failed: {:?}", result);

        let mut env_map = HashMap::new();
        for line in result.output.lines() {
            if let Some((key, value)) = line.split_once('=') {
                env_map.insert(key.to_string(), value.to_string());
            }
        }

        assert_eq!(
            env_map.get("ANTHROPIC_BASE_URL").map(String::as_str),
            Some("https://openrouter.ai/api")
        );
        assert_eq!(
            env_map.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
            Some("test-openrouter-key")
        );
        assert_eq!(env_map.get("ANTHROPIC_API_KEY").map(String::as_str), Some(""));
        assert_eq!(
            env_map.get("ANTHROPIC_SMALL_FAST_MODEL").map(String::as_str),
            Some("moonshotai/kimi-k2.5")
        );
        assert_eq!(
            env_map.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("moonshotai/kimi-k2.5")
        );

        assert_eq!(std::env::var("ANTHROPIC_BASE_URL").ok(), before_base_url);
        assert_eq!(std::env::var("ANTHROPIC_AUTH_TOKEN").ok(), before_auth);
        assert_eq!(std::env::var("ANTHROPIC_API_KEY").ok(), before_api_key);
        assert_eq!(std::env::var("ANTHROPIC_SMALL_FAST_MODEL").ok(), before_small);
        assert_eq!(std::env::var("ANTHROPIC_MODEL").ok(), before_model);
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_engine_shutdown_requested() {
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
        let script_path = temp.path().join("fake-claude.sh");
        let mut file = File::create(&script_path).expect("create script");
        writeln!(file, "#!/bin/sh").expect("write shebang");
        writeln!(file, "cat >/dev/null").expect("write stdin drain");
        writeln!(file, "sleep 5").expect("write sleep");
        drop(file);

        let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod");

        crate::shutdown::request();
        let engine = ClaudeEngine::with_path(script_path.to_string_lossy().to_string());
        let result = engine.execute("Aaron", "test shutdown", temp.path(), 0, None);
        crate::shutdown::reset();

        assert!(!result.success);
        assert_eq!(result.exit_code, 130, "unexpected result: {:?}", result);
        assert_eq!(result.error.as_deref(), Some("Shutdown requested"));
    }
}
