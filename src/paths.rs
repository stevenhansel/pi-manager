use std::path::PathBuf;

/// Root directory for pi-manager's own data.
pub fn pi_manager_root() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".pim")
}

/// Root directory for all profile directories.
pub fn profiles_root() -> PathBuf {
    pi_manager_root().join("profiles")
}

/// Path to a profile's pi agent directory.
pub fn profile_dir(name: &str) -> PathBuf {
    profiles_root().join(name)
}

/// Path to a profile's manifest file (inside the profile directory).
pub fn profile_manifest(name: &str) -> PathBuf {
    profile_dir(name).join("manifest.json")
}

/// Path to a profile's config directory (inside the profile directory).
#[allow(dead_code)]
pub fn profile_config_dir(name: &str) -> PathBuf {
    profile_dir(name).join("config")
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

pub fn pool_mcp_dir() -> PathBuf {
    pool_dir().join("mcp")
}

/// Path to the global pim configuration file.
pub fn pim_config() -> PathBuf {
    pi_manager_root().join("pim.json")
}
