use swarm::agent;
use swarm::config::Config;

/// List agent names and initials.
pub fn cmd_agents(_config: &Config) -> Result<(), String> {
    println!("Available Agents:");
    for (i, name) in agent::NAMES.iter().enumerate() {
        let initial = agent::INITIALS[i];
        println!("  {} - {}", initial, name);
    }
    Ok(())
}
