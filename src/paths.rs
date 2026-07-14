use std::path::PathBuf;

/// Root directory for pi-manager's own data.
pub fn pi_manager_root() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".pi-manager")
}

/// Directory where all profile agent directories are stored.
pub fn profiles_root() -> PathBuf {
    pi_manager_root().join("profiles")
}

/// Path to a specific profile's agent directory.
pub fn profile_dir(name: &str) -> PathBuf {
    profiles_root().join(name)
}

/// Root directory for merged profile views (inheritance).
pub fn merged_root() -> PathBuf {
    pi_manager_root().join(".merged")
}

/// Path to a merged view of a profile.
pub fn merged_dir(name: &str) -> PathBuf {
    merged_root().join(name)
}

/// The actual pi agent directory (`~/.pi/agent`).
/// When managed by pi-manager, this is a symlink pointing to a profile dir
/// or a merged view.
pub fn agent_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".pi")
        .join("agent")
}

/// File that stores the default profile name.
pub fn default_file() -> PathBuf {
    pi_manager_root().join("default")
}
