use std::fs;
use std::path::Path;

use swarm::config::Config;
use swarm::team::{self, Team};

/// Initialize a new swarm repo.
pub fn cmd_init(config: &Config) -> Result<(), String> {
    println!("Initializing swarm repo...");

    // Create .swarm-hug root directory
    team::init_root()?;
    println!("  Created .swarm-hug/");
    println!("  Created .swarm-hug/.gitignore");

    // If a project is specified, initialize that project's directory
    if let Some(ref project_name) = config.project {
        let project = Team::new(project_name);
        project.init()?;
        println!("  Created project: {}", project_name);
        println!("    - {}", project.tasks_path().display());
        println!("    - {}", project.chat_path().display());
        println!("    - {}", project.loop_dir().display());
        println!("    - {}", project.worktrees_dir().display());
    } else {
        init_default_files(config)?;
    }

    println!("\nSwarm repo initialized.");
    println!("  Use 'swarm project init <name>' to create projects.");
    println!("  Use 'swarm --project <name> run' to run sprints for a project.");
    Ok(())
}

fn init_default_files(config: &Config) -> Result<(), String> {
    let tasks_path = Path::new(&config.files_tasks);
    if !tasks_path.exists() {
        ensure_parent_dir(tasks_path)?;
        let default_tasks = "# Tasks\n\n- [ ] Add your tasks here\n";
        fs::write(tasks_path, default_tasks)
            .map_err(|e| format!("failed to create {}: {}", config.files_tasks, e))?;
        println!("  Created {}", config.files_tasks);
    } else {
        println!("  Task file already exists: {}", config.files_tasks);
    }

    let chat_path = Path::new(&config.files_chat);
    if !chat_path.exists() {
        ensure_parent_dir(chat_path)?;
        fs::write(chat_path, "")
            .map_err(|e| format!("failed to create {}: {}", config.files_chat, e))?;
        println!("  Created {}", config.files_chat);
    } else {
        println!("  Chat file already exists: {}", config.files_chat);
    }

    if config.files_log_dir.is_empty() {
        return Err("log dir path is empty".to_string());
    }

    fs::create_dir_all(&config.files_log_dir)
        .map_err(|e| format!("failed to create log dir {}: {}", config.files_log_dir, e))?;
    println!("  Created log directory: {}", config.files_log_dir);

    if config.files_worktrees_dir.is_empty() {
        return Err("worktrees dir path is empty".to_string());
    }

    fs::create_dir_all(&config.files_worktrees_dir)
        .map_err(|e| {
            format!(
                "failed to create worktrees dir {}: {}",
                config.files_worktrees_dir, e
            )
        })?;
    println!("  Created worktrees directory: {}", config.files_worktrees_dir);

    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directory {}: {}", parent.display(), e))?;
        }
    }
    Ok(())
}
