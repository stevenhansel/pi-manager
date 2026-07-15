mod cli;
mod manager;
mod paths;
mod schema;
mod tui;

use clap::Parser;
use cli::{Cli, Commands};
use manager::ProfileManager;

fn main() {
    if let Err(err) = run() {
        eprintln!("❌ Error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    // Intercept raw args for launcher mode before clap parsing.
    let raw: Vec<String> = std::env::args().collect();
    let args: Vec<&str> = raw.iter().map(String::as_str).collect();

    // `pim` (no args) → launch pi with default or show status
    if args.len() == 1 {
        if let Some(default) = ProfileManager::get_default() {
            return ProfileManager::launch_pi(&default, &[]);
        }
        return ProfileManager::status();
    }

    let first = args[1];

    if first.starts_with('-') {
        // `pim <flags>` or `pim -- <pi-args>`
        if cli::is_reserved(first) {
            // `--help`, `--version` → let clap handle it
            let cli = Cli::parse_from(raw);
            return match cli.command {
                Commands::List => ProfileManager::list(),
                Commands::Status => ProfileManager::status(),
                _ => Ok(()), // help/version handled by clap itself
            };
        }
        // Everything else passes through to pi if default exists
        if let Some(default) = ProfileManager::get_default() {
            let pi_start = if first == "--" { 2 } else { 1 };
            let pi_args: Vec<String> = args[pi_start..].iter().map(ToString::to_string).collect();
            return ProfileManager::launch_pi(&default, &pi_args);
        }
    } else if cli::is_reserved(first) {
        // `pim list`, `pim set-default <name>`, etc. → clap management commands
        let cli = Cli::parse_from(raw);
        return match cli.command {
            Commands::Create {
                name,
                from,
                from_base,
            } => ProfileManager::create(&name, from_base, from.as_deref()),
            Commands::List => ProfileManager::list(),
            Commands::Status => ProfileManager::status(),
            Commands::SetDefault { name } => ProfileManager::set_default(&name),
            Commands::Delete { name, force } => ProfileManager::delete(&name, force),
            Commands::Edit { name } => ProfileManager::edit(&name),
        };
    } else {
        // `pim <profile> [-- <pi-args>]` or `pim unknown-cmd`
        let profiles = ProfileManager::list_profile_names();
        if profiles.iter().any(|p| p.as_str() == first) {
            let pi_start = if args.len() > 2 && args[2] == "--" {
                3
            } else {
                2
            };
            let pi_args: Vec<String> = args[pi_start..].iter().map(ToString::to_string).collect();
            return ProfileManager::launch_pi(first, &pi_args);
        }
    }

    // Fallback: let clap try to parse (will show error for unknown commands)
    let cli = Cli::parse_from(raw);
    match cli.command {
        Commands::Create {
            name,
            from,
            from_base,
        } => ProfileManager::create(&name, from_base, from.as_deref()),
        Commands::List => ProfileManager::list(),
        Commands::Status => ProfileManager::status(),
        Commands::SetDefault { name } => ProfileManager::set_default(&name),
        Commands::Delete { name, force } => ProfileManager::delete(&name, force),
        Commands::Edit { name } => ProfileManager::edit(&name),
    }
}
