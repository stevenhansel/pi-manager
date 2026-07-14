use crate::paths;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Manifest file stored in each profile directory.
const MANIFEST_FILE: &str = "pim.json";
const MCP_CONFIG_FILE: &str = "mcp.json";

/// Optional manifest inside a profile directory.
#[derive(Deserialize)]
struct Manifest {
    /// Name of the parent profile to inherit from.
    #[serde(default)]
    inherits: Option<String>,
}

pub struct ProfileManager;

impl ProfileManager {
    // ─── Public API ──────────────────────────────────────────────

    /// Create a new profile directory under `~/.pi-manager/profiles/<name>/`.
    ///
    /// * `from_base` — copy contents from the current `~/.pi/agent` (if it exists)
    /// * `from` — copy contents from another existing profile
    /// * `inherits` — set a parent profile to inherit from (writes `pim.json`)
    pub fn create(
        name: &str,
        from_base: bool,
        from: Option<&str>,
        inherits: Option<&str>,
    ) -> Result<()> {
        let profile = paths::profile_dir(name);
        if profile.exists() {
            bail!("Profile '{}' already exists at {}", name, profile.display());
        }

        // Validate inherits target exists
        if let Some(parent) = inherits {
            let parent_dir = paths::profile_dir(parent);
            if !parent_dir.exists() {
                bail!(
                    "Parent profile '{}' does not exist at {}",
                    parent,
                    parent_dir.display()
                );
            }
        }

        // Determine source directory
        let source_dir = if let Some(src) = from {
            let src_path = paths::profile_dir(src);
            if !src_path.exists() {
                bail!(
                    "Source profile '{}' does not exist at {}",
                    src,
                    src_path.display()
                );
            }
            Some(src_path)
        } else if from_base {
            let agent = paths::agent_dir();
            let agent_display = agent.display().to_string();
            let actual = if agent.is_symlink() {
                agent.read_link().ok().filter(|p| p.exists())
            } else if agent.is_dir() {
                Some(agent)
            } else {
                None
            };
            match actual {
                Some(dir) => Some(dir.to_path_buf()),
                None => {
                    bail!(
                        "No pi config found at {} to copy from. \
                         Run `pi` first, or create an empty profile.",
                        agent_display
                    );
                }
            }
        } else {
            None
        };

        // Create the profile directory and subdirectories
        fs::create_dir_all(&profile)
            .with_context(|| format!("Failed to create profile directory: {}", profile.display()))?;

        for sub in &["extensions", "skills", "prompts", "config"] {
            fs::create_dir_all(profile.join(sub))
                .with_context(|| format!("Failed to create {sub} directory"))?;
        }

        // Copy from source if requested
        if let Some(src) = source_dir {
            Self::copy_dir_contents(&src, &profile)?;
        }

        // Write pim.json if inherits is set
        if let Some(parent) = inherits {
            let manifest = serde_json::json!({ "inherits": parent });
            let manifest_str =
                serde_json::to_string_pretty(&manifest).context("Failed to serialize manifest")?;
            fs::write(profile.join(MANIFEST_FILE), &manifest_str)
                .with_context(|| format!("Failed to write {}", MANIFEST_FILE))?;
        }

        println!(
            "✅ Created profile '{}' at {}",
            name,
            profile.display()
        );
        Ok(())
    }

    /// List all available profiles, marking the active one and the default.
    pub fn list() -> Result<()> {
        let root = paths::profiles_root();
        if !root.exists() {
            println!("No profiles found. Create one with: pim create <name>");
            return Ok(());
        }

        let default = Self::get_default();
        let active = Self::get_active();

        let mut found = false;
        let mut entries: Vec<_> = fs::read_dir(&root)
            .context("Failed to read profiles directory")?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in &entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_active = active.as_deref() == Some(&name);
            let is_default = default.as_deref() == Some(&name);

            let mut markers = Vec::new();
            if is_active {
                markers.push("active");
            }
            if is_default {
                markers.push("default");
            }

            // Show inheritance info
            let inherits = Self::read_inherits(&entry.path());
            let suffix = if let Some(parent) = &inherits {
                if markers.is_empty() {
                    format!(" (inherits {})", parent)
                } else {
                    format!(" ◀ {}, inherits {}", markers.join(", "), parent)
                }
            } else if !markers.is_empty() {
                format!(" ◀ {}", markers.join(", "))
            } else {
                String::new()
            };

            println!("  {}{}", name, suffix);
            found = true;
        }

        if !found {
            println!("No profiles found. Create one with: pim create <name>");
        }

        Ok(())
    }

    /// Show the current status: active profile and default profile.
    pub fn status() -> Result<()> {
        let active = Self::get_active();
        let default = Self::get_default();

        match &active {
            Some(name) => {
                let inherits = Self::read_inherits(&paths::profile_dir(name));
                match &inherits {
                    Some(parent) => println!("Active profile: {} (inherits {})", name, parent),
                    None => println!("Active profile: {}", name),
                }
            }
            None => {
                let agent = paths::agent_dir();
                if agent.is_dir() {
                    println!("~/.pi/agent is a regular directory (not managed by pim)");
                } else if !agent.exists() {
                    println!("~/.pi/agent does not exist (pi not configured yet)");
                } else {
                    println!("~/.pi/agent exists but is not a pim profile symlink");
                }
            }
        }

        match &default {
            Some(name) => println!("Default profile: {}", name),
            None => println!("No default profile set"),
        }

        Ok(())
    }

    /// Set the default profile.
    pub fn set_default(name: &str) -> Result<()> {
        let profile = paths::profile_dir(name);
        if !profile.exists() {
            bail!("Profile '{}' does not exist at {}", name, profile.display());
        }

        let root = paths::pi_manager_root();
        fs::create_dir_all(&root)?;

        fs::write(paths::default_file(), name)
            .with_context(|| format!("Failed to write default profile '{}'", name))?;

        println!("✅ Default profile set to '{}'", name);
        Ok(())
    }

    /// Activate a profile by pointing `~/.pi/agent` at it (via symlink).
    /// If the profile has inheritance, a merged view is built first.
    /// Does NOT launch pi — the user runs `pi` directly.
    pub fn use_profile(name: &str) -> Result<()> {
        let profile = paths::profile_dir(name);
        if !profile.exists() {
            bail!(
                "Profile '{}' does not exist at {}. Create it with: pim create {}",
                name,
                profile.display(),
                name
            );
        }

        // Handle the case where ~/.pi/agent is a real directory (first-time migration)
        Self::ensure_agent_is_symlinkable()?;

        // Determine the target directory — either the profile itself or a merged view
        let target = if Self::read_inherits(&profile).is_some() {
            Self::build_merged_profile(&paths::pi_manager_root(), name)?
        } else {
            profile
        };

        // Create or update the symlink
        Self::set_agent_symlink(&target)?;

        println!(
            "✅ Activated profile '{}' — just run `pi` to use it",
            name
        );
        Ok(())
    }

    /// Activate the default profile. If no default is set, show status.
    pub fn use_default() -> Result<()> {
        match Self::get_default() {
            Some(name) => Self::use_profile(&name),
            None => {
                println!("No default profile set.");
                Self::status()
            }
        }
    }

    /// Delete a profile. If it's active, the symlink is removed first.
    /// Also cleans up any merged view.
    pub fn delete(name: &str, force: bool) -> Result<()> {
        let profile = paths::profile_dir(name);
        if !profile.exists() {
            bail!("Profile '{}' does not exist at {}", name, profile.display());
        }

        let is_active = Self::get_active().as_deref() == Some(name);
        let is_default = Self::get_default().as_deref() == Some(name);

        if !force {
            eprintln!(
                "Are you sure you want to delete profile '{}' at {}? [y/N] ",
                name,
                profile.display()
            );
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim().to_lowercase() != "y" {
                println!("Deletion cancelled.");
                return Ok(());
            }
        }

        fs::remove_dir_all(&profile)
            .with_context(|| format!("Failed to delete profile: {}", profile.display()))?;

        // Clean up merged view if it exists
        let merged = paths::merged_dir(name);
        if merged.exists() {
            fs::remove_dir_all(&merged).ok();
        }

        // Remove symlink if this profile was active
        if is_active {
            let agent = paths::agent_dir();
            if agent.is_symlink() {
                fs::remove_file(&agent).ok();
            }
            println!("✅ Removed active symlink (profile '{}' was active)", name);
        }

        // Clear default if it was the default
        if is_default {
            let def_file = paths::default_file();
            if def_file.exists() {
                fs::remove_file(&def_file).ok();
            }
            println!("✅ Cleared default (profile '{}' was default)", name);
        }

        println!("✅ Deleted profile '{}'", name);
        Ok(())
    }

    // ─── Helpers ────────────────────────────────────────────────

    /// Get the name of the default profile, if any.
    pub fn get_default() -> Option<String> {
        let def_file = paths::default_file();
        if !def_file.exists() {
            return None;
        }
        let name = fs::read_to_string(&def_file).ok()?;
        let name = name.trim().to_string();
        if name.is_empty() {
            return None;
        }
        let profile = paths::profile_dir(&name);
        if profile.exists() { Some(name) } else { None }
    }

    /// Get the name of the currently active profile by reading the
    /// `~/.pi/agent` symlink target.
    pub fn get_active() -> Option<String> {
        let agent = paths::agent_dir();
        if !agent.is_symlink() {
            return None;
        }
        let target = fs::read_link(&agent).ok()?;
        let profiles_root = paths::profiles_root();
        let merged_root = paths::merged_root();

        if target.starts_with(&profiles_root) {
            target.file_name()?.to_str().map(|s| s.to_string())
        } else if target.starts_with(&merged_root) {
            target.file_name()?.to_str().map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Read the inherits field from a profile's pim.json, if present.
    fn read_inherits(profile_dir: &Path) -> Option<String> {
        let manifest_path = profile_dir.join(MANIFEST_FILE);
        if !manifest_path.exists() {
            return None;
        }
        let content = fs::read_to_string(&manifest_path).ok()?;
        let manifest: Manifest = serde_json::from_str(&content).ok()?;
        manifest.inherits.filter(|s| !s.is_empty())
    }

    /// Resolve the full inheritance chain for a profile, from leaf to root.
    /// Returns `[leaf, parent, grandparent, ...]`.
    /// `profiles_root` is the parent directory containing the profile dirs.
    /// Detects cycles and missing parents.
    fn resolve_chain(profiles_root: &Path, name: &str) -> Result<Vec<String>> {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut current = name.to_string();

        loop {
            if !seen.insert(current.clone()) {
                bail!("Circular inheritance detected involving profile '{}'", current);
            }
            chain.push(current.clone());

            let dir = profiles_root.join(&current);
            match Self::read_inherits(&dir) {
                Some(parent) => {
                    let parent_dir = profiles_root.join(&parent);
                    if !parent_dir.exists() {
                        bail!(
                            "Profile '{}' inherits from '{}' which does not exist",
                            current,
                            parent
                        );
                    }
                    current = parent;
                }
                None => break,
            }
        }

        Ok(chain)
    }

    /// Build a merged profile view at `~/.pi-manager/.merged/<name>/`.
    /// Starts from the root ancestor and overlays each profile in the
    /// inheritance chain on top (child files override parent files).
    fn build_merged_profile(pi_manager_root: &Path, name: &str) -> Result<PathBuf> {
        let profiles_root = pi_manager_root.join("profiles");
        let merged_dir = pi_manager_root.join(".merged").join(name);

        let chain = Self::resolve_chain(&profiles_root, name)?;

        // Remove any previous merged view for a clean rebuild
        if merged_dir.exists() {
            fs::remove_dir_all(&merged_dir)
                .context("Failed to remove previous merged view")?;
        }

        // Walk the chain from root-most ancestor to the leaf profile
        // Each layer overlays on top of the previous
        for profile_name in chain.iter().rev() {
            let src = profiles_root.join(profile_name);
            // First ancestor creates the directory; subsequent layers merge into it
            if !merged_dir.exists() {
                fs::create_dir_all(&merged_dir)
                    .context("Failed to create merged directory")?;
            }
            Self::merge_into(&src, &merged_dir)?;
        }

        println!(
            "  Merged inheritance chain: {}",
            chain.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ← ")
        );

        Ok(merged_dir)
    }

    /// Merge the contents of `src` into `dst`, overwriting existing files.
    /// Directories are merged recursively; files from `src` always win.
    fn merge_into(src: &Path, dst: &Path) -> Result<()> {
        for entry in fs::read_dir(src).context("Failed to read source directory")? {
            let entry = entry?;
            let entry_path = entry.path();
            let file_name = entry.file_name();

            // Skip pim.json, sessions, npm, bin
            if file_name == MANIFEST_FILE
                || file_name == "sessions"
                || file_name == "npm"
                || file_name == "bin"
            {
                continue;
            }

            let dest_path = dst.join(&file_name);

            if entry_path.is_dir() {
                fs::create_dir_all(&dest_path)
                    .with_context(|| format!("Failed to create {}", dest_path.display()))?;
                Self::merge_into(&entry_path, &dest_path)?;
            } else if file_name == MCP_CONFIG_FILE {
                // Deep-merge mcp.json: combine mcpServers + settings
                // instead of child fully replacing parent
                Self::merge_mcp_json(&entry_path, &dest_path)?;
            } else if entry_path.is_symlink() {
                // Re-create symlink in merged dir
                let target = fs::read_link(&entry_path)?;
                // Remove existing file/symlink if present
                if dest_path.exists() || dest_path.is_symlink() {
                    fs::remove_file(&dest_path).ok();
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(&target, &dest_path)
                    .with_context(|| format!("Failed to symlink {}", dest_path.display()))?;
                #[cfg(windows)]
                std::os::windows::fs::symlink_file(&target, &dest_path)
                    .with_context(|| format!("Failed to symlink {}", dest_path.display()))?;
            } else {
                // Regular file — copy, overwriting any previous version
                fs::copy(&entry_path, &dest_path)
                    .with_context(|| {
                        format!(
                            "Failed to copy {} to {}",
                            entry_path.display(),
                            dest_path.display()
                        )
                    })?;
            }
        }
        Ok(())
    }

    /// Deep-merge a child profile's `mcp.json` into an existing `mcp.json`
    /// in the merged view. `src` is the child's file, `dest` is the
    /// current merged file (which may already contain the parent's servers).
    ///
    /// Merge rules:
    /// - `mcpServers`: combined — child entries override/add; parent entries
    ///   not present in child are preserved.
    /// - `settings`: merged — child wins on conflicting keys; parent keys
    ///   not in child are preserved.
    /// - Other top-level keys: child wins (full replace).
    fn merge_mcp_json(src: &Path, dest: &Path) -> Result<()> {
        let src_content = fs::read_to_string(src)
            .with_context(|| format!("Failed to read {}", src.display()))?;
        let src_value: serde_json::Value = serde_json::from_str(&src_content)
            .with_context(|| format!("Failed to parse {}", src.display()))?;

        if !dest.exists() {
            // First layer in the merge chain — just copy
            fs::write(dest, &src_content)
                .with_context(|| format!("Failed to write {}", dest.display()))?;
            return Ok(());
        }

        // Read existing merged file
        let dest_content = fs::read_to_string(dest)
            .with_context(|| format!("Failed to read existing {}", dest.display()))?;
        let mut dest_value: serde_json::Value = serde_json::from_str(&dest_content)
            .with_context(|| format!("Failed to parse existing {}", dest.display()))?;

        // Merge mcpServers: child entries override/add, parent entries preserved
        if let (Some(src_servers), Some(dest_servers)) = (
            src_value.get("mcpServers").and_then(|v| v.as_object()),
            dest_value.get_mut("mcpServers").and_then(|v| v.as_object_mut()),
        ) {
            for (key, val) in src_servers {
                dest_servers.insert(key.clone(), val.clone());
            }
        } else if src_value.get("mcpServers").is_some()
            && dest_value.get("mcpServers").is_none()
        {
            dest_value["mcpServers"] = src_value["mcpServers"].clone();
        }

        // Merge settings: child wins on conflicting keys
        if let (Some(src_settings), Some(dest_settings)) = (
            src_value.get("settings").and_then(|v| v.as_object()),
            dest_value.get_mut("settings").and_then(|v| v.as_object_mut()),
        ) {
            for (key, val) in src_settings {
                dest_settings.insert(key.clone(), val.clone());
            }
        } else if src_value.get("settings").is_some()
            && dest_value.get("settings").is_none()
        {
            dest_value["settings"] = src_value["settings"].clone();
        }

        // Other top-level keys from child override
        if let Some(obj) = src_value.as_object() {
            let merged_servers = dest_value.get("mcpServers").is_some();
            let merged_settings = dest_value.get("settings").is_some();
            for (key, val) in obj {
                if key == "mcpServers" && merged_servers {
                    continue; // already merged above
                }
                if key == "settings" && merged_settings {
                    continue; // already merged above
                }
                dest_value[key] = val.clone();
            }
        }

        let merged =
            serde_json::to_string_pretty(&dest_value).context("Failed to serialize mcp.json")?;
        fs::write(dest, &merged)
            .with_context(|| format!("Failed to write {}", dest.display()))?;

        Ok(())
    }

    /// Ensure `~/.pi/agent` can be replaced by a symlink.
    /// If it's a real directory, move it into a profile.
    fn ensure_agent_is_symlinkable() -> Result<()> {
        let agent = paths::agent_dir();

        if !agent.exists() {
            if let Some(parent) = agent.parent() {
                fs::create_dir_all(parent).ok();
            }
            return Ok(());
        }

        if agent.is_symlink() {
            return Ok(());
        }

        if agent.is_dir() {
            eprintln!(
                "╔══════════════════════════════════════════════════════════╗\n\
                 ║  ~/.pi/agent is a real directory, not a symlink.       ║\n\
                 ║  pim needs to migrate it into a managed profile        ║\n\
                 ║  so it can switch between profiles via symlink.        ║\n\
                 ╚══════════════════════════════════════════════════════════╝"
            );

            let backup_name = "default";
            let backup_path = paths::profile_dir(backup_name);

            if backup_path.exists() {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let backup_name = format!("backup-{}", ts);
                let backup_path = paths::profile_dir(&backup_name);

                eprintln!("Moving existing config to pim profile '{}'...", backup_name);
                fs::create_dir_all(&backup_path).with_context(|| {
                    format!("Failed to create backup directory: {}", backup_path.display())
                })?;
                Self::copy_dir_contents(&agent, &backup_path)?;
                eprintln!("  → Backed up as profile '{}'", backup_name);
            } else {
                eprintln!("Moving existing config to pim profile 'default'...");
                fs::create_dir_all(&backup_path).with_context(|| {
                    format!("Failed to create backup directory: {}", backup_path.display())
                })?;
                Self::copy_dir_contents(&agent, &backup_path)?;
            }

            fs::remove_dir_all(&agent).context("Failed to remove old ~/.pi/agent directory")?;
            eprintln!("  ✔  Migrated. pim will now manage ~/.pi/agent as a symlink.");
            return Ok(());
        }

        Ok(())
    }

    /// Set `~/.pi/agent` as a symlink pointing to the given directory.
    fn set_agent_symlink(target: &Path) -> Result<()> {
        let agent = paths::agent_dir();

        if agent.exists() || agent.is_symlink() {
            if agent.is_dir() && !agent.is_symlink() {
                fs::remove_dir_all(&agent)
                    .context("Failed to remove existing ~/.pi/agent directory")?;
            } else {
                fs::remove_file(&agent).context("Failed to remove existing ~/.pi/agent")?;
            }
        }

        if let Some(parent) = agent.parent() {
            fs::create_dir_all(parent)?;
        }

        let abs_target = target
            .canonicalize()
            .unwrap_or_else(|_| target.to_path_buf());

        #[cfg(unix)]
        std::os::unix::fs::symlink(&abs_target, &agent).with_context(|| {
            format!(
                "Failed to create symlink: {} → {}",
                agent.display(),
                abs_target.display()
            )
        })?;

        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&abs_target, &agent).with_context(|| {
            format!(
                "Failed to create symlink: {} → {}",
                agent.display(),
                abs_target.display()
            )
        })?;

        Ok(())
    }

    /// Recursively copy contents of src into dst, preserving structure.
    fn copy_dir_contents(src: &Path, dst: &Path) -> Result<()> {
        for entry in fs::read_dir(src).context("Failed to read source directory")? {
            let entry = entry?;
            let entry_path = entry.path();
            let file_name = entry.file_name();

            if file_name == "sessions" || file_name == "npm" || file_name == "bin" {
                continue;
            }

            let dest_path = dst.join(&file_name);

            if entry_path.is_dir() {
                fs::create_dir_all(&dest_path)
                    .with_context(|| format!("Failed to create {}", dest_path.display()))?;
                Self::copy_dir_contents(&entry_path, &dest_path)?;
            } else if entry_path.is_symlink() {
                let target = fs::read_link(&entry_path)?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(&target, &dest_path)
                    .with_context(|| format!("Failed to symlink {}", dest_path.display()))?;
                #[cfg(windows)]
                std::os::windows::fs::symlink_file(&target, &dest_path)
                    .with_context(|| format!("Failed to symlink {}", dest_path.display()))?;
            } else {
                fs::copy(&entry_path, &dest_path)
                    .with_context(|| {
                        format!(
                            "Failed to copy {} to {}",
                            entry_path.display(),
                            dest_path.display()
                        )
                    })?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: create a temp directory and return (TempDir, profile_root, merged_root).
    /// Writes a fake profiles_root()/merged_root() by pointing the HOME env var.
    fn sandbox() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let home = tmp.path().canonicalize().unwrap_or_else(|_| tmp.path().to_path_buf());
        // Override HOME so paths::*_root() resolves inside the sandbox
        // We can't easily mock paths::*, so we build directory structure manually
        // matching what paths::profile_dir / paths::merged_dir would return.
        (tmp, home)
    }

    fn profile_dir(home: &Path, name: &str) -> PathBuf {
        home.join(".pi-manager").join("profiles").join(name)
    }

    fn merged_dir(home: &Path, name: &str) -> PathBuf {
        home.join(".pi-manager").join(".merged").join(name)
    }

    fn create_profile(home: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = profile_dir(home, name);
        fs::create_dir_all(&dir).unwrap();
        for (path, content) in files {
            let full = dir.join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full, content).unwrap();
        }
        dir
    }



    fn read_file(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn json_value(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    // ─── read_inherits ────────────────────────────────────────────

    #[test]
    fn test_read_inherits_no_pim_json() {
        let (_tmp, home) = sandbox();
        let dir = create_profile(&home, "test", &[]);
        assert_eq!(ProfileManager::read_inherits(&dir), None);
    }

    #[test]
    fn test_read_inherits_with_inherits() {
        let (_tmp, home) = sandbox();
        let dir = create_profile(&home, "test", &[("pim.json", r#"{"inherits": "default"}"#)]);
        assert_eq!(ProfileManager::read_inherits(&dir), Some("default".to_string()));
    }

    #[test]
    fn test_read_inherits_empty_inherits() {
        let (_tmp, home) = sandbox();
        let dir = create_profile(&home, "test", &[("pim.json", r#"{"inherits": ""}"#)]);
        assert_eq!(ProfileManager::read_inherits(&dir), None);
    }

    #[test]
    fn test_read_inherits_missing_inherits_field() {
        let (_tmp, home) = sandbox();
        let dir = create_profile(&home, "test", &[("pim.json", r#"{}"#)]);
        assert_eq!(ProfileManager::read_inherits(&dir), None);
    }

    #[test]
    fn test_read_inherits_invalid_json() {
        let (_tmp, home) = sandbox();
        let dir = create_profile(&home, "test", &[("pim.json", "not json")]);
        assert_eq!(ProfileManager::read_inherits(&dir), None);
    }

    // ─── resolve_chain ────────────────────────────────────────────

    fn profiles_root(home: &Path) -> PathBuf {
        home.join(".pi-manager").join("profiles")
    }

    #[test]
    fn test_resolve_chain_no_inherits() {
        let (_tmp, home) = sandbox();
        create_profile(&home, "leaf", &[]);
        let chain = ProfileManager::resolve_chain(&profiles_root(&home), "leaf").unwrap();
        assert_eq!(chain, vec!["leaf"]);
    }

    #[test]
    fn test_resolve_chain_single_inherit() {
        let (_tmp, home) = sandbox();
        create_profile(&home, "parent", &[]);
        create_profile(&home, "child", &[("pim.json", r#"{"inherits": "parent"}"#)]);
        let chain = ProfileManager::resolve_chain(&profiles_root(&home), "child").unwrap();
        assert_eq!(chain, vec!["child", "parent"]);
    }

    #[test]
    fn test_resolve_chain_two_level_inherit() {
        let (_tmp, home) = sandbox();
        create_profile(&home, "grandparent", &[]);
        create_profile(&home, "parent", &[("pim.json", r#"{"inherits": "grandparent"}"#)]);
        create_profile(&home, "child", &[("pim.json", r#"{"inherits": "parent"}"#)]);
        let chain = ProfileManager::resolve_chain(&profiles_root(&home), "child").unwrap();
        assert_eq!(chain, vec!["child", "parent", "grandparent"]);
    }

    #[test]
    fn test_resolve_chain_missing_parent() {
        let (_tmp, home) = sandbox();
        create_profile(&home, "child", &[("pim.json", r#"{"inherits": "nonexistent"}"#)]);
        let result = ProfileManager::resolve_chain(&profiles_root(&home), "child");
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("nonexistent"));
    }

    #[test]
    fn test_resolve_chain_circular() {
        let (_tmp, home) = sandbox();
        create_profile(&home, "a", &[("pim.json", r#"{"inherits": "b"}"#)]);
        create_profile(&home, "b", &[("pim.json", r#"{"inherits": "a"}"#)]);
        let result = ProfileManager::resolve_chain(&profiles_root(&home), "a");
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("Circular"));
    }

    #[test]
    fn test_resolve_chain_self_inherit() {
        let (_tmp, home) = sandbox();
        create_profile(&home, "self", &[("pim.json", r#"{"inherits": "self"}"#)]);
        let result = ProfileManager::resolve_chain(&profiles_root(&home), "self");
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("Circular"));
    }

    // ─── merge_mcp_json ────────────────────────────────────────────

    #[test]
    fn test_merge_mcp_json_no_dest() {
        let (_tmp, home) = sandbox();
        let merged = merged_dir(&home, "test");
        fs::create_dir_all(&merged).unwrap();

        let src = merged.join("mcp.json");
        let dest = merged.join("mcp_merged.json"); // different name so "no dest"
        fs::write(&src, r#"{"mcpServers": {"srv": {"command": "echo"}}}"#).unwrap();

        ProfileManager::merge_mcp_json(&src, &dest).unwrap();

        let val = json_value(&read_file(&dest));
        assert_eq!(val["mcpServers"]["srv"]["command"], "echo");
    }

    #[test]
    fn test_merge_mcp_json_servers_combined() {
        let (_tmp, home) = sandbox();
        let merged = merged_dir(&home, "test");
        fs::create_dir_all(&merged).unwrap();

        // Parent has server_a
        let dest = merged.join("mcp.json");
        fs::write(
            &dest,
            r#"{"mcpServers": {"server_a": {"command": "parent_a", "args": ["x"]}}}"#,
        )
        .unwrap();

        // Child has server_b (new) and overrides server_a
        let src = merged.join("mcp_child.json");
        fs::write(
            &src,
            r#"{"mcpServers": {"server_a": {"command": "child_a"}, "server_b": {"command": "child_b"}}}"#,
        )
        .unwrap();

        ProfileManager::merge_mcp_json(&src, &dest).unwrap();

        let val = json_value(&read_file(&dest));
        // server_a should be child version
        assert_eq!(val["mcpServers"]["server_a"]["command"], "child_a");
        // server_b should be added
        assert_eq!(val["mcpServers"]["server_b"]["command"], "child_b");
        // server_b should NOT have parent's args
        assert!(val["mcpServers"]["server_b"].get("args").is_none());
    }

    #[test]
    fn test_merge_mcp_json_settings_merged() {
        let (_tmp, home) = sandbox();
        let merged = merged_dir(&home, "test");
        fs::create_dir_all(&merged).unwrap();

        let dest = merged.join("mcp.json");
        fs::write(
            &dest,
            r#"{"settings": {"maxRetries": 5, "timeout": 30}}"#,
        )
        .unwrap();

        let src = merged.join("mcp_child.json");
        fs::write(
            &src,
            r#"{"settings": {"maxRetries": 3, "toolPrefix": "mcp"}}"#,
        )
        .unwrap();

        ProfileManager::merge_mcp_json(&src, &dest).unwrap();

        let val = json_value(&read_file(&dest));
        // Child wins on maxRetries
        assert_eq!(val["settings"]["maxRetries"], 3);
        // Parent's timeout preserved
        assert_eq!(val["settings"]["timeout"], 30);
        // Child's new key added
        assert_eq!(val["settings"]["toolPrefix"], "mcp");
    }

    #[test]
    fn test_merge_mcp_json_child_no_settings() {
        let (_tmp, home) = sandbox();
        let merged = merged_dir(&home, "test");
        fs::create_dir_all(&merged).unwrap();

        let dest = merged.join("mcp.json");
        fs::write(
            &dest,
            r#"{"mcpServers": {"srv": {"command": "x"}}, "settings": {"timeout": 30}}"#,
        )
        .unwrap();

        let src = merged.join("mcp_child.json");
        // Child has mcpServers but no settings
        fs::write(
            &src,
            r#"{"mcpServers": {"srv": {"command": "child"}}}"#,
        )
        .unwrap();

        ProfileManager::merge_mcp_json(&src, &dest).unwrap();

        let val = json_value(&read_file(&dest));
        // Child overrode the server
        assert_eq!(val["mcpServers"]["srv"]["command"], "child");
        // Parent's settings preserved
        assert_eq!(val["settings"]["timeout"], 30);
    }

    #[test]
    fn test_merge_mcp_json_child_no_servers() {
        let (_tmp, home) = sandbox();
        let merged = merged_dir(&home, "test");
        fs::create_dir_all(&merged).unwrap();

        let dest = merged.join("mcp.json");
        fs::write(
            &dest,
            r#"{"mcpServers": {"srv": {"command": "x"}}, "settings": {"timeout": 30}}"#,
        )
        .unwrap();

        let src = merged.join("mcp_child.json");
        // Child has settings but no mcpServers
        fs::write(
            &src,
            r#"{"settings": {"autoUpdate": true}}"#,
        )
        .unwrap();

        ProfileManager::merge_mcp_json(&src, &dest).unwrap();

        let val = json_value(&read_file(&dest));
        // Parent's servers preserved
        assert_eq!(val["mcpServers"]["srv"]["command"], "x");
        // Child's settings merged in
        assert_eq!(val["settings"]["autoUpdate"], true);
        // Parent's settings preserved
        assert_eq!(val["settings"]["timeout"], 30);
    }

    #[test]
    fn test_merge_mcp_json_other_keys_child_wins() {
        let (_tmp, home) = sandbox();
        let merged = merged_dir(&home, "test");
        fs::create_dir_all(&merged).unwrap();

        let dest = merged.join("mcp.json");
        fs::write(&dest, r#"{"customKey": "parent", "otherKey": "keep"}"#).unwrap();

        let src = merged.join("mcp_child.json");
        fs::write(&src, r#"{"customKey": "child"}"#).unwrap();

        ProfileManager::merge_mcp_json(&src, &dest).unwrap();

        let val = json_value(&read_file(&dest));
        // Child wins on customKey
        assert_eq!(val["customKey"], "child");
        // Parent's otherKey preserved
        assert_eq!(val["otherKey"], "keep");
    }

    // ─── merge_into ────────────────────────────────────────────────

    #[test]
    fn test_merge_into_skips_pim_json() {
        let (_tmp, home) = sandbox();
        let src = profile_dir(&home, "src");
        let dst = profile_dir(&home, "dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();

        // pim.json should NOT be copied into dst
        fs::write(src.join("pim.json"), r#"{"inherits": "x"}"#).unwrap();
        fs::write(src.join("settings.json"), "{}").unwrap();

        ProfileManager::merge_into(&src, &dst).unwrap();

        assert!(dst.join("settings.json").exists(), "settings.json should be copied");
        assert!(!dst.join("pim.json").exists(), "pim.json should NOT be copied");
    }

    #[test]
    fn test_merge_into_skips_sessions_npm_bin() {
        let (_tmp, home) = sandbox();
        let src = profile_dir(&home, "src");
        let dst = profile_dir(&home, "dst");
        fs::create_dir_all(&dst).unwrap();

        // Create a source with sessions/, npm/, bin/ and a real file
        fs::create_dir_all(src.join("sessions")).unwrap();
        fs::create_dir_all(src.join("npm")).unwrap();
        fs::create_dir_all(src.join("bin")).unwrap();
        fs::create_dir_all(src.join("extensions")).unwrap();
        fs::write(src.join("sessions").join("session.log"), "data").unwrap();
        fs::write(src.join("npm").join("package.json"), "{}").unwrap();
        fs::write(src.join("bin").join("tool"), "x").unwrap();
        fs::write(src.join("settings.json"), "{}").unwrap();
        fs::write(src.join("extensions").join("ext.ts"), "// code").unwrap();

        ProfileManager::merge_into(&src, &dst).unwrap();

        assert!(dst.join("settings.json").exists(), "settings.json should be copied");
        assert!(dst.join("extensions").join("ext.ts").exists(), "extensions should be copied");
        assert!(!dst.join("sessions").exists(), "sessions should NOT be copied");
        assert!(!dst.join("npm").exists(), "npm should NOT be copied");
        assert!(!dst.join("bin").exists(), "bin should NOT be copied");
    }

    #[test]
    fn test_merge_into_subdirectories() {
        let (_tmp, home) = sandbox();
        let src = profile_dir(&home, "src");
        let dst = profile_dir(&home, "dst");
        fs::create_dir_all(&dst).unwrap();

        fs::create_dir_all(src.join("extensions")).unwrap();
        fs::create_dir_all(src.join("skills").join("nested")).unwrap();
        fs::write(src.join("extensions").join("ext.ts"), "// ext").unwrap();
        fs::write(src.join("skills").join("nested").join("SKILL.md"), "# skill").unwrap();

        ProfileManager::merge_into(&src, &dst).unwrap();

        assert!(dst.join("extensions").join("ext.ts").exists());
        assert!(dst.join("skills").join("nested").join("SKILL.md").exists());
    }

    #[test]
    fn test_merge_into_child_overrides_file() {
        let (_tmp, home) = sandbox();
        let src = profile_dir(&home, "src");
        let dst = profile_dir(&home, "dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();

        fs::write(dst.join("settings.json"), r#"{"theme": "light"}"#).unwrap();
        fs::write(src.join("settings.json"), r#"{"theme": "dark"}"#).unwrap();

        ProfileManager::merge_into(&src, &dst).unwrap();

        // The src (child) should override
        let content = read_file(&dst.join("settings.json"));
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(val["theme"], "dark");
    }

    // ─── build_merged_profile (end-to-end) ───────────────────────

    #[test]
    fn test_build_merged_no_inherits() {
        let (_tmp, home) = sandbox();
        let dir = profile_dir(&home, "single");
        fs::create_dir_all(dir.join("extensions")).unwrap();
        fs::write(dir.join("settings.json"), r#"{"theme": "dark"}"#).unwrap();

        let merged = ProfileManager::build_merged_profile(&home.join(".pi-manager"), "single").unwrap();

        assert!(merged.exists(), "merged dir should exist");
        assert_eq!(merged, merged_dir(&home, "single"));
        assert!(merged.join("settings.json").exists());
        assert_eq!(read_file(&merged.join("settings.json")), r#"{"theme": "dark"}"#);
    }

    #[test]
    fn test_build_merged_with_inheritance() {
        let (_tmp, home) = sandbox();
        // Parent: has server_a, settings
        create_profile(
            &home,
            "base",
            &[
                (
                    "mcp.json",
                    r#"{"mcpServers": {"server_a": {"command": "a"}}, "settings": {"timeout": 30}}"#,
                ),
                ("settings.json", r#"{"theme": "light"}"#),
            ],
        );
        // Child: inherits base, adds server_b, overrides server_a, adds toolPrefix
        create_profile(
            &home,
            "child",
            &[
                ("pim.json", r#"{"inherits": "base"}"#),
                (
                    "mcp.json",
                    r#"{"mcpServers": {"server_b": {"command": "b"}, "server_a": {"command": "a_v2"}}, "settings": {"toolPrefix": "mcp"}}"#,
                ),
                ("settings.json", r#"{"theme": "dark"}"#),
            ],
        );

        let merged = ProfileManager::build_merged_profile(&home.join(".pi-manager"), "child").unwrap();

        // Check mcp.json merge
        let mcp = json_value(&read_file(&merged.join("mcp.json")));
        // server_a from child overrides
        assert_eq!(mcp["mcpServers"]["server_a"]["command"], "a_v2");
        // server_b added by child
        assert_eq!(mcp["mcpServers"]["server_b"]["command"], "b");
        // settings merged: timeout from parent, toolPrefix from child
        assert_eq!(mcp["settings"]["timeout"], 30);
        assert_eq!(mcp["settings"]["toolPrefix"], "mcp");

        // Check settings.json: child overrides parent
        let settings = json_value(&read_file(&merged.join("settings.json")));
        assert_eq!(settings["theme"], "dark");
    }

    #[test]
    fn test_build_merged_cleans_previous() {
        let (_tmp, home) = sandbox();
        create_profile(&home, "p", &[("settings.json", "{}")]);

        // Build once
        let merged = ProfileManager::build_merged_profile(&home.join(".pi-manager"), "p").unwrap();
        assert!(merged.join("settings.json").exists());

        // Add a stale file that simulates an old merge
        fs::write(merged.join("stale.txt"), "should be gone").unwrap();

        // Build again — should clean the stale file
        let merged2 = ProfileManager::build_merged_profile(&home.join(".pi-manager"), "p").unwrap();
        assert_eq!(merged, merged2);
        assert!(!merged2.join("stale.txt").exists(), "stale file should be removed");
        assert!(merged2.join("settings.json").exists(), "real files should be re-copied");
    }

    #[test]
    fn test_build_merged_prints_chain() {
        let (_tmp, home) = sandbox();
        create_profile(&home, "base", &[]);
        create_profile(&home, "middle", &[("pim.json", r#"{"inherits": "base"}"#)]);
        create_profile(&home, "top", &[("pim.json", r#"{"inherits": "middle"}"#)]);

        let merged = ProfileManager::build_merged_profile(&home.join(".pi-manager"), "top").unwrap();
        assert!(merged.exists());
        // We can't easily capture stdout here, but we can verify the merged view exists
        assert!(merged_dir(&home, "top").exists());
    }
}

