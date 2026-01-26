use std::env;

use super::types::{Config, EngineType};

pub(super) fn apply_env(config: &mut Config) {
    if let Ok(val) = env::var("SWARM_AGENTS_MAX_COUNT") {
        if let Ok(n) = val.parse() {
            config.agents_max_count = n;
        }
    }
    if let Ok(val) = env::var("SWARM_AGENTS_TASKS_PER_AGENT") {
        if let Ok(n) = val.parse() {
            config.agents_tasks_per_agent = n;
        }
    }
    if let Ok(val) = env::var("SWARM_AGENT_TIMEOUT") {
        if let Ok(n) = val.parse() {
            config.agent_timeout_secs = n;
        }
    }
    if let Ok(val) = env::var("SWARM_FILES_TASKS") {
        config.files_tasks = val;
    }
    if let Ok(val) = env::var("SWARM_FILES_CHAT") {
        config.files_chat = val;
    }
    if let Ok(val) = env::var("SWARM_FILES_LOG_DIR") {
        config.files_log_dir = val;
    }
    if let Ok(val) = env::var("SWARM_ENGINE_TYPE") {
        if let Some(engines) = EngineType::parse_list(&val) {
            config.engine_types = engines;
        }
    }
    if let Ok(val) = env::var("SWARM_ENGINE_STUB_MODE") {
        config.engine_stub_mode = val == "true" || val == "1";
    }
    if let Ok(val) = env::var("SWARM_SPRINTS_MAX") {
        if let Ok(n) = val.parse() {
            config.sprints_max = n;
        }
    }
}
