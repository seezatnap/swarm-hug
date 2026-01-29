//! Graceful shutdown handling for sprint interruption.
//!
//! Provides a way to handle Ctrl+C (SIGINT) gracefully during sprint execution,
//! allowing the system to:
//! - Stop spawning new agent tasks
//! - Wait for currently running agents to complete (with timeout)
//! - Update the task list properly
//! - Release agent assignments
//! - Commit the sprint state
//!
//! # Example
//!
//! ```ignore
//! use swarm::shutdown;
//!
//! // Register the Ctrl+C handler at startup
//! shutdown::register_handler();
//!
//! // Check if shutdown was requested
//! if shutdown::requested() {
//!     println!("Shutdown requested, cleaning up...");
//! }
//! ```

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::process_registry::PROCESS_REGISTRY;

/// Global flag indicating shutdown has been requested.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Counter for how many times Ctrl+C was pressed (for force-quit on repeated presses).
static INTERRUPT_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Maximum number of interrupts before force-quitting.
const MAX_INTERRUPTS: usize = 3;

/// Register the Ctrl+C handler.
///
/// Should be called once at program startup. Sets up the signal handler
/// to set the shutdown flag when Ctrl+C is pressed.
///
/// # Panics
///
/// Panics if the handler cannot be registered (rare, usually only in tests).
pub fn register_handler() -> Result<(), String> {
    ctrlc::set_handler(move || {
        let count = INTERRUPT_COUNT.fetch_add(1, Ordering::SeqCst) + 1;

        if count >= MAX_INTERRUPTS {
            eprintln!("\nForce quit (received {} interrupts)", count);
            std::process::exit(130); // Standard exit code for Ctrl+C
        }

        if count == 1 {
            eprintln!("\n");
            eprintln!("Interrupt received. Gracefully ending sprint...");
            eprintln!("(Press Ctrl+C {} more time(s) to force quit)", MAX_INTERRUPTS - count);
            SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
            PROCESS_REGISTRY.kill_all();
        } else {
            eprintln!("(Press Ctrl+C {} more time(s) to force quit)", MAX_INTERRUPTS - count);
        }
    })
    .map_err(|e| format!("failed to register Ctrl+C handler: {}", e))
}

/// Check if shutdown has been requested.
///
/// Returns `true` if the user pressed Ctrl+C or if `request()` was called.
pub fn requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

/// Programmatically request shutdown.
///
/// Useful for testing or for triggering shutdown from other conditions.
pub fn request() {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

/// Reset the shutdown state.
///
/// Primarily for testing. Clears the shutdown flag and interrupt counter.
pub fn reset() {
    SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    INTERRUPT_COUNT.store(0, Ordering::SeqCst);
}

/// Get the number of interrupts received.
pub fn interrupt_count() -> usize {
    INTERRUPT_COUNT.load(Ordering::SeqCst)
}

/// A cloneable handle to check shutdown status.
///
/// This is useful for passing to threads that need to check for shutdown.
#[derive(Clone)]
pub struct ShutdownSignal {
    flag: Arc<AtomicBool>,
}

impl ShutdownSignal {
    /// Create a new shutdown signal that tracks the global shutdown state.
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a signal linked to the global shutdown flag.
    ///
    /// The signal will return true from `is_shutdown()` if either:
    /// - The global Ctrl+C handler was triggered
    /// - `trigger()` was called on this signal
    pub fn global() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if shutdown is requested (either global or local).
    pub fn is_shutdown(&self) -> bool {
        requested() || self.flag.load(Ordering::SeqCst)
    }

    /// Trigger shutdown on this signal.
    pub fn trigger(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Get a clone of the underlying flag for sharing with threads.
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.flag)
    }
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: We can't easily test the actual Ctrl+C handler in unit tests,
    // but we can test the flag mechanics.

    #[test]
    fn test_shutdown_request_and_check() {
        reset();
        assert!(!requested());

        request();
        assert!(requested());

        reset();
        assert!(!requested());
    }

    #[test]
    fn test_interrupt_count() {
        reset();
        assert_eq!(interrupt_count(), 0);

        // Simulate interrupts by directly manipulating the counter
        INTERRUPT_COUNT.store(2, Ordering::SeqCst);
        assert_eq!(interrupt_count(), 2);

        reset();
        assert_eq!(interrupt_count(), 0);
    }

    #[test]
    fn test_shutdown_signal_local() {
        reset();
        let signal = ShutdownSignal::new();

        assert!(!signal.is_shutdown());

        signal.trigger();
        assert!(signal.is_shutdown());
    }

    #[test]
    fn test_shutdown_signal_global() {
        reset();
        let signal = ShutdownSignal::global();

        assert!(!signal.is_shutdown());

        // Global request should affect the signal
        request();
        assert!(signal.is_shutdown());

        reset();
    }

    #[test]
    fn test_shutdown_signal_clone() {
        reset();
        let signal1 = ShutdownSignal::new();
        let signal2 = signal1.clone();

        assert!(!signal1.is_shutdown());
        assert!(!signal2.is_shutdown());

        signal1.trigger();
        // Both should see the trigger since they share the same Arc
        assert!(signal1.is_shutdown());
        assert!(signal2.is_shutdown());
    }

    #[test]
    fn test_shutdown_signal_thread_safe() {
        reset();
        let signal = ShutdownSignal::new();
        let flag = signal.flag();

        // Simulate what a thread would do
        std::thread::spawn(move || {
            flag.store(true, Ordering::SeqCst);
        })
        .join()
        .unwrap();

        assert!(signal.is_shutdown());
    }
}
