use clap::{Parser, Subcommand};

/// Profile manager for pi
///
/// Manage independent config profiles stored in ~/.pi-manager/profiles/.
/// Each profile is a complete pi agent directory that gets symlinked
/// into ~/.pi/agent when activated. After activating a profile, just
/// run `pi` directly — no wrapper needed.
///
/// Profiles can inherit from a parent profile via a pim.json manifest.
/// The parent provides common files (e.g., rtk.ts, web-research) and
/// the child overlays profile-specific additions on top.
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

        /// Copy settings from the current ~/.pi/agent config
        #[arg(long)]
        from_base: bool,

        /// Copy settings from an existing profile
        #[arg(long)]
        from: Option<String>,

        /// Inherit from a parent profile (e.g. --inherits default).
        /// Common files from the parent are merged in at activation time.
        #[arg(long)]
        inherits: Option<String>,
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

    /// Activate a profile by pointing ~/.pi/agent at it.
    /// After this, just run `pi` as usual.
    Use {
        /// Profile name
        name: String,
    },

    /// Delete a profile
    Delete {
        /// Profile name
        name: String,

        /// Skip confirmation prompt
        #[arg(long, short = 'f')]
        force: bool,
    },
}
