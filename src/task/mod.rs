//! Task file (TASKS.md) parser and writer.
//!
//! Supports the checklist format:
//! - `- [ ] Task description` (unassigned)
//! - `- [A] Task description` (assigned to Aaron)
//! - `- [x] Task description (A)` (completed by Aaron)

mod assign;
mod model;
mod parse;

#[cfg(test)]
mod tests;

pub use model::{Task, TaskList, TaskStatus};
