use std::path::Path;

use swarm::agent;
use swarm::config::Config;
use swarm::team::Assignments;
use swarm::worktree;

use crate::project::{project_name_for_config, release_assignments_for_project};

/// List active worktrees.
pub fn cmd_worktrees(config: &Config) -> Result<(), String> {
    println!("Git Worktrees ({}):", config.files_worktrees_dir);
    let worktrees = worktree::list_worktrees(Path::new(&config.files_worktrees_dir))?;

    if worktrees.is_empty() {
        println!("  (no worktrees)");
    } else {
        for wt in &worktrees {
            println!("  {} ({}) - {}", wt.name, wt.initial, wt.path.display());
        }
    }
    Ok(())
}

/// List worktree branches.
pub fn cmd_worktrees_branch(_config: &Config) -> Result<(), String> {
    println!("Agent Branches:");
    let branches = worktree::list_agent_branches()?;

    if branches.is_empty() {
        println!("  (no agent branches found)");
    } else {
        for b in &branches {
            let status = if b.exists { "active" } else { "missing" };
            let name = agent::name_from_initial(b.initial).unwrap_or("?");
            println!("  {} ({}) - {} [{}]", name, b.initial, b.branch, status);
        }
    }
    Ok(())
}

/// Clean up worktrees and branches.
pub fn cmd_cleanup(config: &Config) -> Result<(), String> {
    println!("Cleaning up worktrees and branches...");
    let team_name = project_name_for_config(config);
    let worktrees_dir = Path::new(&config.files_worktrees_dir);
    let mut errors: Vec<String> = Vec::new();

    // Get agents currently assigned to this team (before we release them)
    let team_agents: Vec<char> = match Assignments::load() {
        Ok(assignments) => assignments.team_agents(&team_name),
        Err(_) => Vec::new(),
    };

    // Also get agents from existing worktrees (in case assignments already released)
    let worktree_agents: Vec<char> = worktree::list_worktrees(worktrees_dir)
        .unwrap_or_default()
        .iter()
        .map(|wt| wt.initial)
        .collect();

    // Combine both lists (union)
    let mut agents_to_cleanup: Vec<char> = team_agents.clone();
    for initial in worktree_agents {
        if !agents_to_cleanup.contains(&initial) {
            agents_to_cleanup.push(initial);
        }
    }

    // Clean up worktrees in the team directory
    if let Err(e) = worktree::cleanup_worktrees_in(worktrees_dir) {
        errors.push(format!("worktree cleanup failed: {}", e));
    } else {
        println!("  Worktrees removed from {}", config.files_worktrees_dir);
    }

    // Delete branches only for this team's agents
    let mut deleted = 0usize;
    for initial in &agents_to_cleanup {
        match worktree::delete_agent_branch(*initial) {
            Ok(true) => {
                let name = agent::name_from_initial(*initial).unwrap_or("?");
                println!("  Deleted branch: agent/{}", name.to_lowercase());
                deleted += 1;
            }
            Ok(false) => {}
            Err(e) => {
                let name = agent::name_from_initial(*initial).unwrap_or("?");
                errors.push(format!("failed to delete branch for {}: {}", name, e));
            }
        }
    }

    // Also clean up team-specific scrummaster branch if it exists
    let scrummaster_branch = format!("agent/scrummaster-{}", team_name);
    if let Ok(true) = worktree::delete_branch(&scrummaster_branch) {
        println!("  Deleted branch: {}", scrummaster_branch);
        deleted += 1;
    }
    // Clean up legacy scrummaster branch too (from before this fix)
    if let Ok(true) = worktree::delete_branch("agent/scrummaster") {
        println!("  Deleted branch: agent/scrummaster (legacy)");
        deleted += 1;
    }

    if deleted > 0 {
        println!("  Deleted {} branch(es) total", deleted);
    }

    // Release agent assignments for this team
    match release_assignments_for_project(&team_name, &[]) {
        Ok(released) => {
            if released > 0 {
                println!("  Released {} agent assignment(s) for team {}", released, team_name);
            }
        }
        Err(e) => errors.push(format!("assignment release failed: {}", e)),
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
