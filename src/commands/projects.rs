use std::fs;

use swarm::agent;
use swarm::config::{self, Config};
use swarm::engine;
use swarm::planning;
use swarm::team::{self, Assignments, Team};

/// List all projects and their assigned agents.
pub fn cmd_projects(_config: &Config) -> Result<(), String> {
    if !team::root_exists() {
        println!("No .swarm-hug/ directory found. Run 'swarm init' first.");
        return Ok(());
    }

    let projects = team::list_teams()?;
    let assignments = Assignments::load()?;

    if projects.is_empty() {
        println!("No projects found. Use 'swarm project init <name>' to create one.");
        return Ok(());
    }

    println!("Projects:");
    for p in &projects {
        let agents = assignments.project_agents(&p.name);
        let agent_str = if agents.is_empty() {
            "(no agents assigned)".to_string()
        } else {
            agents
                .iter()
                .map(|&i| {
                    let name = agent::name_from_initial(i).unwrap_or("?");
                    format!("{} ({})", name, i)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!("  {} - {}", p.name, agent_str);
    }

    // Show available agents
    let available = assignments.next_available(5);
    if !available.is_empty() {
        println!("\nNext available agents:");
        for i in available {
            let name = agent::name_from_initial(i).unwrap_or("?");
            println!("  {} - {}", i, name);
        }
    }

    Ok(())
}

/// Initialize a new project.
pub fn cmd_project_init(config: &Config, cli: &config::CliArgs) -> Result<(), String> {
    let project_name = cli.project_arg.as_ref()
        .ok_or("Usage: swarm project init <name>")?;

    // Validate project name (alphanumeric and hyphens only)
    if !project_name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err("Project name must contain only letters, numbers, hyphens, and underscores".to_string());
    }

    // Initialize root if needed
    team::init_root()?;

    let project = Team::new(project_name);
    if project.exists() {
        println!("Project '{}' already exists.", project_name);
        return Ok(());
    }

    project.init()?;
    println!("Created project: {}", project_name);
    println!("  Directory: {}", project.root.display());

    // Handle --with-prd flag
    if let Some(ref prd_path) = cli.prd_file_arg {
        println!("\nProcessing PRD file: {}", prd_path);

        // Read the PRD file
        let prd_content = fs::read_to_string(prd_path)
            .map_err(|e| format!("Failed to read PRD file '{}': {}", prd_path, e))?;

        // Write the PRD content to specs.md
        let specs_content = format!(
            "# Specifications: {}\n\n{}\n",
            project_name,
            prd_content
        );
        fs::write(project.specs_path(), &specs_content)
            .map_err(|e| format!("Failed to write specs.md: {}", e))?;
        println!("  Specs:     {} (from PRD)", project.specs_path().display());

        // Convert PRD to tasks using the engine
        let log_dir = project.loop_dir();
        let engine = engine::create_engine(config.effective_engine(), log_dir.to_str().unwrap_or(""), config.agent_timeout_secs);

        println!("  Converting PRD to tasks (engine={})...", config.effective_engine().as_str());
        let result = planning::convert_prd_to_tasks(engine.as_ref(), &prd_content, &log_dir);

        if result.success {
            // Write tasks to tasks.md
            let tasks_content = format!("# Tasks\n\n{}\n", result.tasks_markdown);
            fs::write(project.tasks_path(), &tasks_content)
                .map_err(|e| format!("Failed to write tasks.md: {}", e))?;

            // Count tasks generated
            let task_count = result.tasks_markdown.matches("- [ ]").count();
            println!("  Tasks:     {} ({} tasks generated)", project.tasks_path().display(), task_count);
        } else {
            let error = result.error.unwrap_or_else(|| "Unknown error".to_string());
            eprintln!("  Warning: PRD conversion failed: {}", error);
            eprintln!("  Using default tasks.md instead.");
            println!("  Tasks:     {}", project.tasks_path().display());
        }
    } else {
        println!("  Tasks:     {}", project.tasks_path().display());
        println!("  Specs:     {}", project.specs_path().display());
    }

    println!("  Chat:      {}", project.chat_path().display());
    println!("  Logs:      {}", project.loop_dir().display());
    println!("  Worktrees: {}", project.worktrees_dir().display());
    println!("\nTo work on this project, use:");
    println!("  swarm --project {} run", project_name);
    println!("  swarm -p {} status", project_name);

    Ok(())
}
