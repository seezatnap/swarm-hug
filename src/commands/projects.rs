use std::fs;

use swarm::config::{self, Config};
use swarm::engine;
use swarm::planning;
use swarm::team::{self, Team};

/// Task completion counts for a project.
struct TaskCounts {
    completed: usize,
    total: usize,
}

/// Count completed and total tasks in a tasks.md file.
fn count_tasks(team: &Team) -> TaskCounts {
    let tasks_path = team.tasks_path();
    let content = match fs::read_to_string(&tasks_path) {
        Ok(c) => c,
        Err(_) => return TaskCounts { completed: 0, total: 0 },
    };

    let mut completed = 0;
    let mut total = 0;

    for line in content.lines() {
        let trimmed = line.trim();
        // Match task lines: "- [x]", "- [ ]", "- [A]", etc.
        if trimmed.starts_with("- [") && trimmed.len() > 4 && trimmed.chars().nth(4) == Some(']') {
            total += 1;
            if trimmed.starts_with("- [x]") {
                completed += 1;
            }
        }
    }

    TaskCounts { completed, total }
}

/// List all projects and their task status.
pub fn cmd_projects(_config: &Config) -> Result<(), String> {
    if !team::root_exists() {
        println!("No .swarm-hug/ directory found. Run 'swarm init' first.");
        return Ok(());
    }

    let projects = team::list_teams()?;

    if projects.is_empty() {
        println!("No projects found. Use 'swarm project init <name>' to create one.");
        return Ok(());
    }

    // Collect projects with task counts
    let mut project_data: Vec<(Team, TaskCounts)> = projects
        .into_iter()
        .map(|p| {
            let counts = count_tasks(&p);
            (p, counts)
        })
        .collect();

    // Sort by most incomplete first (incomplete = total - completed)
    // Complete projects (incomplete == 0) go to the bottom
    project_data.sort_by(|a, b| {
        let incomplete_a = a.1.total.saturating_sub(a.1.completed);
        let incomplete_b = b.1.total.saturating_sub(b.1.completed);
        incomplete_b.cmp(&incomplete_a)
    });

    println!("Projects:");
    for (p, counts) in &project_data {
        // Format task completion status
        let task_status = if counts.total == 0 {
            String::new()
        } else if counts.completed == counts.total {
            format!(" [{}/{} Tasks Complete \u{2713}]", counts.completed, counts.total)
        } else {
            format!(" [{}/{} Tasks Complete]", counts.completed, counts.total)
        };

        println!("  {}{}", p.name, task_status);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::with_temp_cwd;

    #[test]
    fn test_count_tasks_missing_file() {
        with_temp_cwd(|| {
            let team = Team::new("nonexistent");
            let counts = count_tasks(&team);
            assert_eq!(counts.completed, 0);
            assert_eq!(counts.total, 0);
        });
    }

    #[test]
    fn test_count_tasks_empty_file() {
        with_temp_cwd(|| {
            let team = Team::new("test-project");
            team.init().unwrap();
            fs::write(team.tasks_path(), "# Tasks\n\n").unwrap();

            let counts = count_tasks(&team);
            assert_eq!(counts.completed, 0);
            assert_eq!(counts.total, 0);
        });
    }

    #[test]
    fn test_count_tasks_all_pending() {
        with_temp_cwd(|| {
            let team = Team::new("test-project");
            team.init().unwrap();
            fs::write(team.tasks_path(), "# Tasks\n\n- [ ] Task 1\n- [ ] Task 2\n- [ ] Task 3\n").unwrap();

            let counts = count_tasks(&team);
            assert_eq!(counts.completed, 0);
            assert_eq!(counts.total, 3);
        });
    }

    #[test]
    fn test_count_tasks_all_completed() {
        with_temp_cwd(|| {
            let team = Team::new("test-project");
            team.init().unwrap();
            fs::write(team.tasks_path(), "# Tasks\n\n- [x] Task 1\n- [x] Task 2\n").unwrap();

            let counts = count_tasks(&team);
            assert_eq!(counts.completed, 2);
            assert_eq!(counts.total, 2);
        });
    }

    #[test]
    fn test_count_tasks_mixed() {
        with_temp_cwd(|| {
            let team = Team::new("test-project");
            team.init().unwrap();
            fs::write(team.tasks_path(), "# Tasks\n\n- [x] Done 1\n- [ ] Pending\n- [x] Done 2\n- [ ] Another pending\n").unwrap();

            let counts = count_tasks(&team);
            assert_eq!(counts.completed, 2);
            assert_eq!(counts.total, 4);
        });
    }

    #[test]
    fn test_count_tasks_with_agent_assigned() {
        with_temp_cwd(|| {
            let team = Team::new("test-project");
            team.init().unwrap();
            // Agent-assigned tasks like "- [A]" count as total but not completed
            fs::write(team.tasks_path(), "# Tasks\n\n- [x] Done\n- [A] Assigned to A\n- [B] Assigned to B\n- [ ] Pending\n").unwrap();

            let counts = count_tasks(&team);
            assert_eq!(counts.completed, 1);
            assert_eq!(counts.total, 4);
        });
    }

    #[test]
    fn test_count_tasks_with_sections() {
        with_temp_cwd(|| {
            let team = Team::new("test-project");
            team.init().unwrap();
            let content = r#"# Tasks

## Section 1
- [x] Done 1
- [ ] Pending 1

## Section 2
- [x] Done 2
- [x] Done 3
- [A] In progress
"#;
            fs::write(team.tasks_path(), content).unwrap();

            let counts = count_tasks(&team);
            assert_eq!(counts.completed, 3);
            assert_eq!(counts.total, 5);
        });
    }
}
