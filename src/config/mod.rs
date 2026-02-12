//! Configuration loading for swarm.
//!
//! Supports swarm.toml, CLI flags, and environment variables.
//! Precedence (highest to lowest): CLI flags > env vars > config file > defaults.

mod cli;
mod env;
mod toml;
mod types;

pub use cli::{parse_args, CliArgs, Command};
pub use types::{Config, ConfigError, EngineType, DEFAULT_AGENT_TIMEOUT_SECS};

#[cfg(test)]
mod tests;
