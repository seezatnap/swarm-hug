//! Swarm: Multi-agent sprint-based orchestration system.
//!
//! A Rust implementation of agent orchestration with git worktrees,
//! task management, and sprint-based execution.
//!
//! ## Multi-Team Support
//!
//! All configuration and artifacts live in `.swarm-hug/`:
//! - `.swarm-hug/<team>/` - Per-team directory
//! - `.swarm-hug/<team>/tasks.md` - Team's task list
//! - `.swarm-hug/<team>/chat.md` - Team's chat log
//! - `.swarm-hug/<team>/loop/` - Team's agent logs
//! - `.swarm-hug/<team>/worktrees/` - Team's git worktrees
//! - `.swarm-hug/<team>/runs/<target>/` - Runtime-local sprint state (ignored by git)

pub mod agent;
pub mod chat;
pub mod color;
pub mod config;
pub mod engine;
pub mod heartbeat;
pub mod lifecycle;
pub mod log;
pub mod merge_agent;
pub mod planning;
pub mod process;
pub mod process_group;
pub mod process_registry;
pub mod prompt;
pub mod run_context;
pub mod run_hash;
pub mod shutdown;
pub mod task;
pub mod team;
#[doc(hidden)]
pub mod testutil;
pub mod tui;
pub mod worktree;
