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

    // Auto-heal the active profile symlink if it was pointing to an old-style format.
    if let Err(e) = ProfileManager::auto_heal_symlink() {
        eprintln!("⚠️  Warning: Failed to auto-heal active profile symlink: {}", e);
    }

    match cli.command {
        Some(Commands::Create { name, from, from_base }) => {
            ProfileManager::create(&name, from_base, from.as_deref())
        }
        Some(Commands::Migrate) => ProfileManager::migrate(),
        Some(Commands::List) => ProfileManager::list(),
        Some(Commands::Status) => ProfileManager::status(),
        Some(Commands::SetDefault { name }) => ProfileManager::set_default(&name),
        Some(Commands::Use { name }) => {
            ProfileManager::use_profile(&name)
        }
        Some(Commands::Delete { name, force }) => ProfileManager::delete(&name, force),
        Some(Commands::Edit { name }) => ProfileManager::edit(&name),
        None => {
            // No subcommand → activate default profile
            ProfileManager::use_default()
        }
    }
}
