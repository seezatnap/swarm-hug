use std::collections::HashSet;

use swarm::config::Config;
use swarm::team::Assignments;

pub(crate) fn project_name_for_config(config: &Config) -> String {
    config.project.clone().unwrap_or_else(|| "default".to_string())
}

pub(crate) fn release_assignments_for_project(
    project_name: &str,
    initials: &[char],
) -> Result<usize, String> {
    let mut assignments = Assignments::load()?;

    if initials.is_empty() {
        let released = assignments.project_agents(project_name).len();
        if released > 0 {
            assignments.release_project(project_name);
            assignments.save()?;
        }
        return Ok(released);
    }

    let mut released = 0usize;
    let mut seen = HashSet::new();

    for initial in initials {
        let upper = initial.to_ascii_uppercase();
        if !seen.insert(upper) {
            continue;
        }
        if assignments.get_project(upper) == Some(project_name) {
            assignments.release(upper);
            released += 1;
        }
    }

    if released > 0 {
        assignments.save()?;
    }

    Ok(released)
}
