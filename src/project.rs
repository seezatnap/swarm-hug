use swarm::config::Config;

pub(crate) fn project_name_for_config(config: &Config) -> String {
    config.project.clone().unwrap_or_else(|| "default".to_string())
}
