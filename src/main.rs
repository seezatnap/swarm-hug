use std::env;
use std::process;

use swarm::config::{self, Command, Config};
use swarm::shutdown;

mod commands;
mod git;
mod output;
mod project;
mod runner;
mod tail;

use commands::{
    cmd_agents, cmd_customize_prompts, cmd_init, cmd_project_init, cmd_projects, cmd_run,
    cmd_run_tui, cmd_set_email,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = env::args().collect();
    let cli = config::parse_args(args);

    if cli.help {
        output::print_help();
        return;
    }

    if cli.version {
        println!("swarm {}", VERSION);
        return;
    }

    if let Some(unknown) = cli.unknown_command.as_deref() {
        eprintln!("error: unknown command: {}", unknown);
        process::exit(1);
    }

    if let Err(e) = git::ensure_min_git_version() {
        eprintln!("error: {}", e);
        process::exit(1);
    }

    let config = Config::load(&cli);

    // Default command is Run if none specified
    let command = cli.command.clone().unwrap_or(Command::Run);

    // Register Ctrl+C handler for commands that run sprints
    if matches!(command, Command::Run) {
        if let Err(e) = shutdown::register_handler() {
            eprintln!("warning: {}", e);
        }
    }

    let result = match command {
        Command::Init => cmd_init(&config),
        Command::Run => {
            if cli.no_tui {
                cmd_run(&config)
            } else {
                cmd_run_tui(&config)
            }
        }
        Command::Agents => cmd_agents(&config),
        Command::Projects => cmd_projects(&config),
        Command::ProjectInit => cmd_project_init(&config, &cli),
        Command::CustomizePrompts => cmd_customize_prompts(),
        Command::SetEmail => cmd_set_email(&cli),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        process::exit(1);
    }
}
