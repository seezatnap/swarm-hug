use std::fs;
use std::path::Path;

use swarm::chat;
use swarm::color::{self, emoji};
use swarm::config::Config;
use swarm::task::TaskList;

/// Show task status.
pub fn cmd_status(config: &Config) -> Result<(), String> {
    // Load and parse tasks
    let content = fs::read_to_string(&config.files_tasks)
        .map_err(|e| format!("failed to read {}: {}", config.files_tasks, e))?;
    let task_list = TaskList::parse(&content);

    println!("{} {} ({}):", emoji::TASK, color::label("Task Status"), config.files_tasks);
    println!("  Unassigned: {}", color::number(task_list.unassigned_count()));
    println!("  Assigned:   {}", color::warning(&task_list.assigned_count().to_string()));
    println!("  Completed:  {}", color::completed(&task_list.completed_count().to_string()));
    println!("  Assignable: {}", color::number(task_list.assignable_count()));
    println!("  Total:      {}", color::number(task_list.tasks.len()));

    // Show recent chat lines
    println!("\n{} {} ({}):", emoji::THINKING, color::label("Recent Chat"), config.files_chat);
    if Path::new(&config.files_chat).exists() {
        match chat::read_recent(&config.files_chat, 5) {
            Ok(lines) => {
                if lines.is_empty() {
                    println!("  (no messages)");
                } else {
                    for line in lines {
                        println!("  {}", color::chat_line(&line));
                    }
                }
            }
            Err(e) => println!("  (error reading chat: {})", e),
        }
    } else {
        println!("  (file not found)");
    }

    Ok(())
}
