//! Test utilities for swarm.
//!
//! Provides shared test helpers, particularly for tests that need to change
//! the current working directory. Since Rust's test runner executes tests
//! in parallel, we need a global mutex to prevent race conditions when
//! changing the process-wide working directory.

#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
use tempfile::TempDir;

/// Global mutex for tests that change the current working directory.
///
/// The current working directory is a process-wide property, so tests that
/// change it must be serialized to avoid race conditions. Use `with_temp_cwd`
/// for tests that need to operate in a temporary directory.
#[cfg(test)]
pub static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Execute a closure in a temporary directory, returning to the original
/// directory afterward.
///
/// This function:
/// 1. Acquires the global CWD_LOCK to prevent parallel directory changes
/// 2. Saves the current working directory
/// 3. Creates a new temporary directory and changes to it
/// 4. Executes the provided closure
/// 5. Restores the original working directory
///
/// # Panics
///
/// Panics if the current directory cannot be determined, the temp directory
/// cannot be created, or the directory changes fail.
///
/// # Example
///
/// ```ignore
/// use swarm::testutil::with_temp_cwd;
///
/// #[test]
/// fn test_something() {
///     with_temp_cwd(|| {
///         // Current directory is now a fresh temp directory
///         std::fs::write("test.txt", "hello").unwrap();
///         assert!(std::path::Path::new("test.txt").exists());
///     });
///     // Back to original directory, temp directory has been cleaned up
/// }
/// ```
#[cfg(test)]
pub fn with_temp_cwd<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original = std::env::current_dir().expect("failed to get current directory");
    let temp = TempDir::new().expect("failed to create temp directory");
    std::env::set_current_dir(temp.path()).expect("failed to change to temp directory");
    let result = f();
    std::env::set_current_dir(original).expect("failed to restore original directory");
    result
}
