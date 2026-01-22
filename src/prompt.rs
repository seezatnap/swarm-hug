//! Prompt template loading and rendering.
//!
//! Loads prompt templates from the `prompts/` directory and renders them
//! with variable substitution.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Default prompts directory relative to the binary.
const DEFAULT_PROMPTS_DIR: &str = "prompts";

/// Find the prompts directory.
///
/// Looks for the prompts directory in the following order:
/// 1. SWARM_PROMPTS_DIR environment variable
/// 2. ./prompts (relative to current directory)
/// 3. Alongside the executable
fn find_prompts_dir() -> Option<PathBuf> {
    // Check environment variable
    if let Ok(dir) = std::env::var("SWARM_PROMPTS_DIR") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Some(path);
        }
    }

    // Check relative to current directory
    let cwd_prompts = PathBuf::from(DEFAULT_PROMPTS_DIR);
    if cwd_prompts.is_dir() {
        return Some(cwd_prompts);
    }

    // Check alongside executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let exe_prompts = exe_dir.join(DEFAULT_PROMPTS_DIR);
            if exe_prompts.is_dir() {
                return Some(exe_prompts);
            }
        }
    }

    None
}

/// Load a prompt template from file.
///
/// Returns None if the file cannot be read.
pub fn load_prompt(name: &str) -> Option<String> {
    let prompts_dir = find_prompts_dir()?;
    let path = prompts_dir.join(format!("{}.md", name));
    fs::read_to_string(&path).ok()
}

/// Load a prompt template from file, returning an error if not found.
///
/// # Errors
/// Returns an error message if the prompt file cannot be found or read.
pub fn load_prompt_required(name: &str) -> Result<String, String> {
    match find_prompts_dir() {
        Some(prompts_dir) => {
            let path = prompts_dir.join(format!("{}.md", name));
            fs::read_to_string(&path).map_err(|e| {
                format!(
                    "Failed to read required prompt '{}' from {}: {}",
                    name,
                    path.display(),
                    e
                )
            })
        }
        None => Err(format!(
            "Prompts directory not found. Create a 'prompts/' directory with {}.md, \
             or set SWARM_PROMPTS_DIR environment variable.",
            name
        )),
    }
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

/// Convenience function to load and render a required prompt in one call.
///
/// # Errors
/// Returns an error if the prompt file cannot be found or read.
pub fn load_and_render(name: &str, vars: &HashMap<&str, String>) -> Result<String, String> {
    let template = load_prompt_required(name)?;
    Ok(render(&template, vars))
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
        vars.insert("branch", "agent/aaron".to_string());

        let result = render(template, &vars);
        assert_eq!(result, "Agent Aaron on branch agent/aaron");
    }

    #[test]
    fn test_render_missing_var() {
        let template = "Hello {{name}} and {{other}}!";
        let mut vars = HashMap::new();
        vars.insert("name", "World".to_string());

        let result = render(template, &vars);
        assert_eq!(result, "Hello World and {{other}}!");
    }
}
