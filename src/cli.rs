use clap::{Parser, Subcommand};

/// Launcher and profile manager for pi (AI coding agent).
///
/// USAGE:
///   pim                          Launch pi with default profile
///   pim <profile>                Launch pi with a specific profile
///   pim <profile> -- <args>      Launch pi with profile and pass args
///   pim set-default <name>       Build/refresh profile + set as default
///   pim edit <name>              Edit profile selections (interactive)
///   pim list                     List profiles
///   pim create <name>            Create a new profile
///   pim create <name> --from <x> Create from existing profile
///   pim delete <name>            Delete a profile
///   pim status                   Show current status
///   pim migrate                  Migrate old-style profiles
///
/// Each profile has its own extensions, skills, prompts, MCP servers,
/// auth tokens, sessions, and config files. Profiles are fully isolated.
/// Multiple profiles can run simultaneously in different terminals.
#[derive(Parser)]
#[command(name = "pim", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new profile
    Create {
        /// Profile name
        name: String,

        /// Copy selections from an existing profile
        #[arg(long)]
        from: Option<String>,

        /// Copy selections from the currently active profile
        #[arg(long)]
        from_base: bool,
    },

    /// List all profiles (shows active and default markers)
    List,

    /// Show current status (active profile, default profile)
    Status,

    /// Set a default profile (launched when running `pim` with no args)
    SetDefault {
        /// Profile name
        name: String,
    },

    /// Delete a profile and its data
    Delete {
        /// Profile name
        name: String,

        /// Skip confirmation prompt
        #[arg(long, short = 'f')]
        force: bool,
    },

    /// Edit a profile's selections (extensions, skills, prompts) interactively
    Edit {
        /// Profile name
        name: String,
    },

    /// Migrate old-style profiles (directories) to the new JSON manifest format
    Migrate,
}

/// Names of reserved subcommands that can never be profile names.
pub const RESERVED_COMMANDS: &[&str] = &[
    "edit",
    "list",
    "create",
    "delete",
    "status",
    "migrate",
    "set-default",
    "help",
    "--help",
    "-h",
    "--version",
    "-V",
];

/// Returns true if `arg` is a reserved management command or flag.
pub fn is_reserved(s: &str) -> bool {
    RESERVED_COMMANDS.contains(&s)
}
