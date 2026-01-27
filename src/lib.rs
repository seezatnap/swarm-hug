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
//! - `.swarm-hug/assignments.toml` - Agent-to-team assignments

pub mod agent;
pub mod chat;
pub mod color;
pub mod config;
pub mod heartbeat;
pub mod tui;
pub mod engine;
pub mod lifecycle;
pub mod log;
pub mod planning;
pub mod prompt;
pub mod shutdown;
pub mod task;
pub mod team;
#[doc(hidden)]
pub mod testutil;
pub mod worktree;
