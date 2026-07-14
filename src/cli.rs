use clap::{Parser, Subcommand};

/// Profile manager for pi (AI coding agent).
///
/// Manage independent profiles stored as lightweight JSON manifests in
/// ~/.pi-manager/profiles/. Each profile selects resources (extensions,
/// skills, prompts) from a global pool and declares per-profile settings.
///
/// Activate a profile with `pim use <name>` — this builds the effective
/// ~/.pi/agent directory from the pool + manifest. Then just run `pi`
/// as usual.
#[derive(Parser)]
#[command(name = "pim", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
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

    /// Set a default profile (activated when running `pim` with no args)
    SetDefault {
        /// Profile name
        name: String,
    },

    /// Activate a profile by building its active view and pointing
    /// ~/.pi/agent at it. After this, just run `pi` as usual.
    Use {
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

    /// Migrate old-style profiles (directories) to the new JSON manifest format
    Migrate,
}
