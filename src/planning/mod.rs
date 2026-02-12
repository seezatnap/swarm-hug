//! LLM-assisted sprint planning module.
//!
//! Provides intelligent task assignment, post-sprint review, and PRD-to-tasks
//! conversion capabilities using the engine abstraction. Can use any engine (claude, codex, stub).

mod assign;
mod parse;
mod prd;
mod review;

pub use assign::{
    generate_scrum_master_prompt, parse_llm_assignments, run_llm_assignment, PlanningResult,
};
pub use prd::{convert_prd_to_tasks, generate_prd_prompt, parse_prd_response, PrdConversionResult};
pub use review::{
    format_follow_up_tasks, generate_review_prompt, parse_review_response, run_sprint_review,
};
