use std::path::PathBuf;

/// Root directory for pi-manager's own data.
pub fn pi_manager_root() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".pi-manager")
}

/// Directory where profile manifests (JSON) are stored.
pub fn profiles_root() -> PathBuf {
    pi_manager_root().join("profiles")
}

/// Path to a profile manifest JSON file.
pub fn profile_manifest(name: &str) -> PathBuf {
    profiles_root().join(format!("{}.json", name))
}

/// Old-style profile directory (pre-migration).
pub fn profile_dir(name: &str) -> PathBuf {
    profiles_root().join(name)
}

/// Global pool directory for reusable resources.
pub fn pool_dir() -> PathBuf {
    pi_manager_root().join("pool")
}

pub fn pool_extensions_dir() -> PathBuf {
    pool_dir().join("extensions")
}

pub fn pool_skills_dir() -> PathBuf {
    pool_dir().join("skills")
}

pub fn pool_prompts_dir() -> PathBuf {
    pool_dir().join("prompts")
}

/// Directory for profile-specific runtime state (auth, sessions, etc.)
pub fn data_dir(name: &str) -> PathBuf {
    pi_manager_root().join("data").join(name)
}

/// Root directory for active profile views.
pub fn active_root() -> PathBuf {
    pi_manager_root().join(".active")
}

/// Path to an active view of a profile (the effective agent dir).
pub fn active_dir(name: &str) -> PathBuf {
    active_root().join(name)
}

/// The actual pi agent directory (`~/.pi/agent`).
/// When managed by pi-manager, this is a symlink pointing to an active view.
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
