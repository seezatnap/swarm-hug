//! Prompt template loading and rendering.
//!
//! Prompts are embedded in the binary at compile time but can be overridden
//! by placing custom prompts in `.swarm-hug/prompts/` or setting SWARM_PROMPTS_DIR.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Embedded prompts (compiled into the binary).
pub mod embedded {
    pub const AGENT: &str = include_str!("../prompts/agent.md");
    pub const SCRUM_MASTER: &str = include_str!("../prompts/scrum_master.md");
    pub const REVIEW: &str = include_str!("../prompts/review.md");
    pub const PRD_TO_TASKS: &str = include_str!("../prompts/prd_to_tasks.md");
}

/// All available prompt names.
pub const PROMPT_NAMES: &[&str] = &["agent", "scrum_master", "review", "prd_to_tasks"];

/// Get the embedded prompt content by name.
pub fn get_embedded(name: &str) -> Option<&'static str> {
    match name {
        "agent" => Some(embedded::AGENT),
        "scrum_master" => Some(embedded::SCRUM_MASTER),
        "review" => Some(embedded::REVIEW),
        "prd_to_tasks" => Some(embedded::PRD_TO_TASKS),
        _ => None,
    }
}

/// Find the prompts directory for custom overrides.
///
/// Looks for the prompts directory in the following order:
/// 1. SWARM_PROMPTS_DIR environment variable
/// 2. .swarm-hug/prompts (for project-specific customization)
/// 3. ./prompts (relative to current directory)
fn find_prompts_dir() -> Option<PathBuf> {
    // Check environment variable
    if let Ok(dir) = std::env::var("SWARM_PROMPTS_DIR") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Some(path);
        }
    }

    // Check .swarm-hug/prompts for project-specific overrides
    let swarm_prompts = PathBuf::from(".swarm-hug/prompts");
    if swarm_prompts.is_dir() {
        return Some(swarm_prompts);
    }

    // Check relative to current directory
    let cwd_prompts = PathBuf::from("prompts");
    if cwd_prompts.is_dir() {
        return Some(cwd_prompts);
    }

    None
}

/// Load a prompt template, checking for custom overrides first.
///
/// Priority:
/// 1. Custom file in prompts directory (if found)
/// 2. Embedded prompt (compiled into binary)
///
/// Returns None only if the prompt name is unknown.
pub fn load_prompt(name: &str) -> Option<String> {
    // Try to load from file first (custom override)
    if let Some(prompts_dir) = find_prompts_dir() {
        let path = prompts_dir.join(format!("{}.md", name));
        if let Ok(content) = fs::read_to_string(&path) {
            return Some(content);
        }
    }

    // Fall back to embedded prompt
    get_embedded(name).map(|s| s.to_string())
}

/// Load a prompt template, returning an error if not found.
///
/// This should only fail for unknown prompt names since valid prompts
/// are embedded in the binary.
pub fn load_prompt_required(name: &str) -> Result<String, String> {
    load_prompt(name).ok_or_else(|| {
        format!(
            "Unknown prompt '{}'. Valid prompts are: {}",
            name,
            PROMPT_NAMES.join(", ")
        )
    })
}

/// Render a prompt template with variable substitution.
///
/// Variables are specified as `{{variable_name}}` in the template.
pub fn render(template: &str, vars: &HashMap<&str, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }
    result
}

/// Convenience function to load and render a prompt in one call.
///
/// # Errors
/// Returns an error only if the prompt name is unknown.
pub fn load_and_render(name: &str, vars: &HashMap<&str, String>) -> Result<String, String> {
    let template = load_prompt_required(name)?;
    Ok(render(&template, vars))
}

/// Copy all embedded prompts to a target directory for customization.
///
/// Creates the directory if it doesn't exist.
pub fn copy_prompts_to(target_dir: &Path) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(target_dir)
        .map_err(|e| format!("Failed to create prompts directory: {}", e))?;

    let mut created = Vec::new();

    for &name in PROMPT_NAMES {
        let content = get_embedded(name)
            .ok_or_else(|| format!("Missing embedded prompt: {}", name))?;

        let path = target_dir.join(format!("{}.md", name));
        fs::write(&path, content)
            .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;

        created.push(path);
    }

    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_simple() {
        let template = "Hello {{name}}!";
        let mut vars = HashMap::new();
        vars.insert("name", "World".to_string());

        let result = render(template, &vars);
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn test_render_multiple_vars() {
        let template = "Agent {{agent_name}} on branch {{branch}}";
        let mut vars = HashMap::new();
        vars.insert("agent_name", "Aaron".to_string());
        vars.insert("branch", "agent-aaron".to_string());

        let result = render(template, &vars);
        assert_eq!(result, "Agent Aaron on branch agent-aaron");
    }

    #[test]
    fn test_render_missing_var() {
        let template = "Hello {{name}} and {{other}}!";
        let mut vars = HashMap::new();
        vars.insert("name", "World".to_string());

        let result = render(template, &vars);
        assert_eq!(result, "Hello World and {{other}}!");
    }

    #[test]
    fn test_get_embedded_valid() {
        assert!(get_embedded("agent").is_some());
        assert!(get_embedded("scrum_master").is_some());
        assert!(get_embedded("review").is_some());
        assert!(get_embedded("prd_to_tasks").is_some());
    }

    #[test]
    fn test_get_embedded_invalid() {
        assert!(get_embedded("nonexistent").is_none());
    }

    #[test]
    fn test_load_prompt_uses_embedded() {
        // Even without a prompts directory, embedded prompts work
        let prompt = load_prompt("agent");
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("{{agent_name}}"));
    }

    #[test]
    fn test_load_prompt_required_valid() {
        let result = load_prompt_required("agent");
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_prompt_required_invalid() {
        let result = load_prompt_required("nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown prompt"));
    }
}
