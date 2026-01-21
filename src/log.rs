//! Per-agent logging with rotation.
//!
//! Provides file-based logging for agents with automatic rotation when
//! log files exceed a configurable line limit.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::Local;

/// Default maximum number of lines before rotation.
pub const DEFAULT_MAX_LINES: usize = 1000;

/// A logger for a specific agent.
pub struct AgentLogger {
    /// Path to the log file.
    pub path: PathBuf,
    /// Maximum lines before rotation.
    pub max_lines: usize,
    /// Agent initial (for logging context).
    pub initial: char,
    /// Agent name (for logging context).
    pub name: String,
}

impl AgentLogger {
    /// Create a new agent logger.
    ///
    /// # Arguments
    /// * `log_dir` - Directory for log files
    /// * `initial` - Agent initial (e.g., 'A')
    /// * `name` - Agent name (e.g., "Aaron")
    pub fn new(log_dir: &Path, initial: char, name: &str) -> Self {
        let path = log_file_path(log_dir, initial);
        Self {
            path,
            max_lines: DEFAULT_MAX_LINES,
            initial,
            name: name.to_string(),
        }
    }

    /// Create a logger with a custom max lines setting.
    pub fn with_max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = max_lines;
        self
    }

    /// Write a log entry.
    ///
    /// Format: `YYYY-MM-DD HH:MM:SS | <AgentName> | <message>`
    pub fn log(&self, message: &str) -> io::Result<()> {
        self.ensure_dir()?;

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let line = format!("{} | {} | {}\n", timestamp, self.name, message);

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        file.write_all(line.as_bytes())?;
        file.flush()?;

        // Check if rotation is needed
        self.rotate_if_needed()?;

        Ok(())
    }

    /// Write a separator for a new run/session.
    pub fn log_session_start(&self) -> io::Result<()> {
        self.ensure_dir()?;

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let separator = format!(
            "\n======================================================================\n\
             === Agent {} ({}) - Session Started at {} ===\n\
             ======================================================================\n\n",
            self.name, self.initial, timestamp
        );

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        file.write_all(separator.as_bytes())?;
        file.flush()?;

        Ok(())
    }

    /// Ensure the log directory exists.
    fn ensure_dir(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// Check and rotate log if it exceeds max lines.
    fn rotate_if_needed(&self) -> io::Result<()> {
        if !self.path.exists() {
            return Ok(());
        }

        let line_count = count_lines(&self.path)?;
        if line_count <= self.max_lines {
            return Ok(());
        }

        rotate_log(&self.path)?;
        Ok(())
    }

    /// Get the current line count of the log file.
    pub fn line_count(&self) -> io::Result<usize> {
        if !self.path.exists() {
            return Ok(0);
        }
        count_lines(&self.path)
    }

    /// Read all lines from the log file.
    pub fn read_all(&self) -> io::Result<Vec<String>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        reader.lines().collect()
    }

    /// Read the last N lines from the log file.
    pub fn read_recent(&self, n: usize) -> io::Result<Vec<String>> {
        let all_lines = self.read_all()?;
        let start = all_lines.len().saturating_sub(n);
        Ok(all_lines[start..].to_vec())
    }
}

/// Get the log file path for an agent.
pub fn log_file_path(log_dir: &Path, initial: char) -> PathBuf {
    log_dir.join(format!("agent-{}.log", initial))
}

/// Count lines in a file.
pub fn count_lines(path: &Path) -> io::Result<usize> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    Ok(reader.lines().count())
}

/// Rotate a log file.
///
/// Creates a timestamped backup and clears the original file.
pub fn rotate_log(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let timestamp = Local::now().format("%Y%m%d-%H%M%S");
    let backup_name = format!(
        "{}.{}.bak",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("log"),
        timestamp
    );
    let backup_path = path.with_file_name(backup_name);

    // Move current log to backup
    fs::rename(path, &backup_path)?;

    // Create empty new log file
    File::create(path)?;

    Ok(())
}

/// Rotate all log files in a directory that exceed the max line count.
pub fn rotate_logs_in_dir(log_dir: &Path, max_lines: usize) -> io::Result<()> {
    if !log_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("log") {
            let line_count = count_lines(&path)?;
            if line_count > max_lines {
                rotate_log(&path)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "swarm-log-test-{}-{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_log_file_path() {
        let dir = Path::new("/tmp/loop");
        assert_eq!(log_file_path(dir, 'A'), PathBuf::from("/tmp/loop/agent-A.log"));
        assert_eq!(log_file_path(dir, 'B'), PathBuf::from("/tmp/loop/agent-B.log"));
    }

    #[test]
    fn test_agent_logger_new() {
        let dir = temp_dir();
        let logger = AgentLogger::new(&dir, 'A', "Aaron");

        assert_eq!(logger.initial, 'A');
        assert_eq!(logger.name, "Aaron");
        assert_eq!(logger.max_lines, DEFAULT_MAX_LINES);
        assert_eq!(logger.path, dir.join("agent-A.log"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_agent_logger_with_max_lines() {
        let dir = temp_dir();
        let logger = AgentLogger::new(&dir, 'B', "Betty").with_max_lines(500);

        assert_eq!(logger.max_lines, 500);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_agent_logger_log() {
        let dir = temp_dir();
        let logger = AgentLogger::new(&dir, 'A', "Aaron");

        logger.log("Starting task").unwrap();
        logger.log("Task complete").unwrap();

        let content = fs::read_to_string(&logger.path).unwrap();
        assert!(content.contains("Aaron"));
        assert!(content.contains("Starting task"));
        assert!(content.contains("Task complete"));

        // Check format includes timestamp and separator
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in &lines {
            assert!(line.contains(" | Aaron | "));
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_agent_logger_session_start() {
        let dir = temp_dir();
        let logger = AgentLogger::new(&dir, 'C', "Carlos");

        logger.log_session_start().unwrap();

        let content = fs::read_to_string(&logger.path).unwrap();
        assert!(content.contains("======"));
        assert!(content.contains("Carlos"));
        assert!(content.contains("Session Started"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_count_lines() {
        let dir = temp_dir();
        let path = dir.join("test.log");

        fs::write(&path, "line1\nline2\nline3\n").unwrap();
        assert_eq!(count_lines(&path).unwrap(), 3);

        fs::write(&path, "").unwrap();
        assert_eq!(count_lines(&path).unwrap(), 0);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_rotate_log() {
        let dir = temp_dir();
        let path = dir.join("test.log");

        fs::write(&path, "line1\nline2\nline3\n").unwrap();
        rotate_log(&path).unwrap();

        // Original file should be empty/recreated
        assert_eq!(fs::read_to_string(&path).unwrap(), "");

        // Backup file should exist
        let backups: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().to_string_lossy().contains(".bak"))
            .collect();
        assert_eq!(backups.len(), 1);

        let backup_content = fs::read_to_string(backups[0].path()).unwrap();
        assert_eq!(backup_content, "line1\nline2\nline3\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_rotate_log_nonexistent() {
        let dir = temp_dir();
        let path = dir.join("nonexistent.log");

        // Should not fail on nonexistent file
        rotate_log(&path).unwrap();

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_agent_logger_rotation() {
        let dir = temp_dir();
        let logger = AgentLogger::new(&dir, 'A', "Aaron").with_max_lines(5);

        // Write more than max_lines
        for i in 0..10 {
            logger.log(&format!("Line {}", i)).unwrap();
        }

        // After rotation, the current log should have fewer lines
        let line_count = logger.line_count().unwrap();
        assert!(line_count <= 5, "Expected <= 5 lines, got {}", line_count);

        // A backup should exist
        let backups: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().to_string_lossy().contains(".bak"))
            .collect();
        assert!(!backups.is_empty(), "Expected backup file to exist");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_agent_logger_read_recent() {
        let dir = temp_dir();
        let logger = AgentLogger::new(&dir, 'B', "Betty");

        for i in 0..10 {
            logger.log(&format!("Message {}", i)).unwrap();
        }

        let recent = logger.read_recent(3).unwrap();
        assert_eq!(recent.len(), 3);
        assert!(recent[0].contains("Message 7"));
        assert!(recent[1].contains("Message 8"));
        assert!(recent[2].contains("Message 9"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_rotate_logs_in_dir() {
        let dir = temp_dir();

        // Create multiple log files
        let log1 = dir.join("agent-A.log");
        let log2 = dir.join("agent-B.log");
        let other = dir.join("other.txt");

        // Write lines to each
        let mut content1 = String::new();
        for i in 0..20 {
            content1.push_str(&format!("Line A {}\n", i));
        }
        fs::write(&log1, &content1).unwrap();

        let mut content2 = String::new();
        for i in 0..5 {
            content2.push_str(&format!("Line B {}\n", i));
        }
        fs::write(&log2, &content2).unwrap();

        fs::write(&other, "Not a log file").unwrap();

        // Rotate with max 10 lines
        rotate_logs_in_dir(&dir, 10).unwrap();

        // log1 should be rotated (had 20 lines)
        assert_eq!(fs::read_to_string(&log1).unwrap(), "");

        // log2 should be unchanged (had 5 lines)
        assert_eq!(fs::read_to_string(&log2).unwrap(), content2);

        // other.txt should be unchanged
        assert_eq!(fs::read_to_string(&other).unwrap(), "Not a log file");

        // Backup for log1 should exist
        let backups: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().to_string_lossy().contains("agent-A.log") &&
                       e.path().to_string_lossy().contains(".bak"))
            .collect();
        assert_eq!(backups.len(), 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_logger_creates_directory() {
        let dir = temp_dir();
        let nested = dir.join("deep").join("nested").join("loop");
        let logger = AgentLogger::new(&nested, 'Z', "Zane");

        logger.log("Test message").unwrap();

        assert!(nested.exists());
        assert!(logger.path.exists());

        fs::remove_dir_all(&dir).ok();
    }
}
