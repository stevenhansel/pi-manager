mod cli;
mod manager;
mod paths;

use clap::Parser;
use cli::{Cli, Commands};
use manager::ProfileManager;

fn main() {
    if let Err(err) = run() {
        eprintln!("❌ Error: {:#}", err);
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Create { name, from_base, from, inherits }) => {
            ProfileManager::create(&name, from_base, from.as_deref(), inherits.as_deref())
        }
        Some(Commands::List) => ProfileManager::list(),
        Some(Commands::Status) => ProfileManager::status(),
        Some(Commands::SetDefault { name }) => ProfileManager::set_default(&name),
        Some(Commands::Use { name }) => {
            ProfileManager::use_profile(&name)
        }
        Some(Commands::Delete { name, force }) => ProfileManager::delete(&name, force),
        None => {
            // No subcommand → activate default profile
            ProfileManager::use_default()
        }
    }
}
