use std::fs;
use std::path::Path;

use swarm::config;
use swarm::prompt;

/// Copy embedded prompts to .swarm-hug/prompts/ for customization.
pub fn cmd_customize_prompts() -> Result<(), String> {
    let target_dir = Path::new(".swarm-hug/prompts");

    if target_dir.exists() {
        println!("Prompts directory already exists: {}", target_dir.display());
        println!("To reset to defaults, remove the directory first:");
        println!("  rm -rf .swarm-hug/prompts");
        return Ok(());
    }

    println!("Copying embedded prompts to {}...", target_dir.display());
    let created = prompt::copy_prompts_to(target_dir)?;

    println!("\nCreated {} prompt file(s):", created.len());
    for path in &created {
        println!("  {}", path.display());
    }

    println!("\nYou can now customize these prompts. They will be used instead of the built-in defaults.");
    println!("Available variables:");
    println!("  agent.md:        {{{{agent_name}}}}, {{{{task_description}}}}, {{{{agent_name_lower}}}}, {{{{agent_initial}}}}, {{{{task_short}}}}");
    println!("  scrum_master.md: {{{{to_assign}}}}, {{{{num_agents}}}}, {{{{tasks_per_agent}}}}, {{{{num_unassigned}}}}, {{{{agent_list}}}}, {{{{task_list}}}}");
    println!("  review.md:       {{{{git_log}}}}, {{{{tasks_content}}}}");

    Ok(())
}

/// Set the co-author email for commits.
pub fn cmd_set_email(cli: &config::CliArgs) -> Result<(), String> {
    let email = cli.email_arg.as_ref()
        .ok_or("Usage: swarm set-email <email>")?;

    // Validate email format (basic check)
    if !email.contains('@') {
        return Err("Invalid email format (must contain @)".to_string());
    }

    // Ensure .swarm-hug directory exists
    let swarm_hug_dir = Path::new(".swarm-hug");
    if !swarm_hug_dir.exists() {
        fs::create_dir_all(swarm_hug_dir)
            .map_err(|e| format!("failed to create .swarm-hug/: {}", e))?;
    }

    // Write email to .swarm-hug/email.txt
    let email_path = swarm_hug_dir.join("email.txt");
    fs::write(&email_path, email)
        .map_err(|e| format!("failed to write {}: {}", email_path.display(), e))?;

    println!("Co-author email set to: {}", email);
    println!("Stored in: {}", email_path.display());
    println!("\nAll commits and merges will now include:");
    println!("  Co-Authored-By: {} <{}>", extract_username(email), email);

    Ok(())
}

/// Extract username from email (part before @).
fn extract_username(email: &str) -> &str {
    email.split('@').next().unwrap_or(email)
}
