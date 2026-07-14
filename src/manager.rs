use crate::paths;
use anyhow::{bail, Context, Result};
use dialoguer::{theme::ColorfulTheme, MultiSelect};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const MCP_CONFIG_FILE: &str = "mcp.json";

/// A profile manifest — a lightweight JSON file that selects resources
/// from the global pool and declares per-profile configuration.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileManifest {
    #[serde(default)]
    select: Selection,
    #[serde(default)]
    settings: serde_json::Value,
    #[serde(default)]
    mcp_servers: serde_json::Value,
    #[serde(default)]
    mcp_settings: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Selection {
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    prompts: Vec<String>,
}

pub struct ProfileManager;

impl ProfileManager {
    // ─── Public API ──────────────────────────────────────────────

    /// Create a new profile manifest.
    ///
    /// * `name` — profile name
    /// * `from` — copy selections from another profile manifest
    /// * `from_base` — copy selections from the currently active profile
    pub fn create(name: &str, from_base: bool, from: Option<&str>) -> Result<()> {
        let manifest_path = paths::profile_manifest(name);
        if manifest_path.exists() {
            bail!("Profile '{}' already exists at {}", name, manifest_path.display());
        }
        if paths::profile_dir(name).is_dir() {
            bail!(
                "A directory for profile '{}' already exists at {}. Use 'pim migrate' first.",
                name,
                paths::profile_dir(name).display()
            );
        }

        let manifest = if let Some(src) = from {
            let src_path = paths::profile_manifest(src);
            let content = fs::read_to_string(&src_path)
                .with_context(|| format!("Failed to read profile '{}'", src))?;
            serde_json::from_str::<ProfileManifest>(&content)
                .with_context(|| format!("Failed to parse profile '{}'", src))?
        } else if from_base {
            match Self::get_active() {
                Some(ref name) => {
                    let src_path = paths::profile_manifest(name);
                    if src_path.exists() {
                        let content = fs::read_to_string(&src_path)?;
                        serde_json::from_str(&content)?
                    } else {
                        bail!("Active profile '{}' is not a JSON manifest (old format?). Use 'pim migrate' first.", name)
                    }
                }
                None => bail!("No active profile to copy from"),
            }
        } else {
            ProfileManifest::default()
        };

        fs::create_dir_all(paths::profiles_root())?;
        let json = serde_json::to_string_pretty(&manifest)
            .context("Failed to serialize manifest")?;
        fs::write(&manifest_path, &json)
            .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

        println!("✅ Created profile '{}'", name);
        Ok(())
    }

    /// List all available profiles.
    pub fn list() -> Result<()> {
        let root = paths::profiles_root();
        if !root.exists() {
            println!("No profiles found. Create one with: pim create <name>");
            return Ok(());
        }

        let default = Self::get_default();
        let active = Self::get_active();
        let mut found = false;

        let mut manifests: Vec<String> = Vec::new();
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json")
                    && entry.file_type().map(|t| t.is_file()).unwrap_or(false)
                    && entry.file_name() != "pim.json"
                {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        manifests.push(stem.to_string());
                    }
                }
            }
        }

        let mut old_dirs: Vec<String> = Vec::new();
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && !path.is_symlink() {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        if name != ".active" {
                            old_dirs.push(name.to_string());
                        }
                    }
                }
            }
        }

        manifests.sort();
        old_dirs.sort();

        for name in &manifests {
            let is_active = active.as_deref() == Some(name.as_str());
            let is_default = default.as_deref() == Some(name.as_str());
            let mut markers = Vec::new();
            if is_active { markers.push("active"); }
            if is_default { markers.push("default"); }
            let suffix = if markers.is_empty() { String::new() } else { format!(" ◀ {}", markers.join(", ")) };
            println!("  {}{}", name, suffix);
            found = true;
        }

        for name in &old_dirs {
            let is_active = active.as_deref() == Some(name.as_str());
            let is_default = default.as_deref() == Some(name.as_str());
            let mut markers = Vec::new();
            if is_active { markers.push("active"); }
            if is_default { markers.push("default"); }
            let suffix = if markers.is_empty() {
                String::new()
            } else {
                format!(" ◀ {} (old format, run pim migrate)", markers.join(", "))
            };
            println!("  {}{}", name, suffix);
            found = true;
        }

        if !found {
            println!("No profiles found. Create one with: pim create <name>");
        }

        Ok(())
    }

    /// Show current status.
    pub fn status() -> Result<()> {
        let active = Self::get_active();
        let default = Self::get_default();

        match &active {
            Some(name) => {
                let manifest = paths::profile_manifest(name);
                if manifest.exists() {
                    let ext_count = Self::count_selected(name, "extensions").unwrap_or(0);
                    let skill_count = Self::count_selected(name, "skills").unwrap_or(0);
                    println!("Active profile: {} ({} extensions, {} skills)", name, ext_count, skill_count);
                } else {
                    println!("Active profile: {} (pre-migration format)", name);
                }
            }
            None => {
                let agent = paths::agent_dir();
                if agent.is_dir() && !agent.is_symlink() {
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
        let manifest = paths::profile_manifest(name);
        let old_dir = paths::profile_dir(name);
        if !manifest.exists() && !old_dir.is_dir() {
            bail!("Profile '{}' does not exist", name);
        }
        let root = paths::pi_manager_root();
        fs::create_dir_all(&root)?;
        fs::write(paths::default_file(), name)
            .with_context(|| format!("Failed to write default profile '{}'", name))?;
        println!("✅ Default profile set to '{}'", name);
        Ok(())
    }

    /// Activate a profile by building its active view and pointing `~/.pi/agent` at it.
    pub fn use_profile(name: &str) -> Result<()> {
        fs::create_dir_all(paths::pool_extensions_dir())?;
        fs::create_dir_all(paths::pool_skills_dir())?;
        fs::create_dir_all(paths::pool_prompts_dir())?;

        // Handle old-style profile directory transparently
        let old_dir = paths::profile_dir(name);
        if old_dir.is_dir() && !old_dir.is_symlink() {
            Self::migrate_one(name)?;
        }

        let manifest_path = paths::profile_manifest(name);
        if !manifest_path.exists() {
            bail!("Profile '{}' does not exist at {}", name, manifest_path.display());
        }

        let content = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read profile '{}'", name))?;
        let manifest: ProfileManifest = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse profile '{}'", name))?;

        let active_dir = paths::active_dir(name);
        if active_dir.exists() {
            fs::remove_dir_all(&active_dir).context("Failed to remove previous active view")?;
        }

        Self::build_from_manifest(&manifest, &active_dir)?;

        fs::create_dir_all(paths::data_dir(name))?;
        Self::link_data_dir(name, &active_dir)?;

        Self::set_agent_symlink(&active_dir)?;

        let ext_count = manifest.select.extensions.len();
        let skill_count = manifest.select.skills.len();
        let prompt_count = manifest.select.prompts.len();
        println!("✅ Activated profile '{}' — {} extensions, {} skills, {} prompts", name, ext_count, skill_count, prompt_count);
        Ok(())
    }

    /// Activate the default profile.
    pub fn use_default() -> Result<()> {
        match Self::get_default() {
            Some(name) => Self::use_profile(&name),
            None => { println!("No default profile set."); Self::status() }
        }
    }

    /// Delete a profile and its data.
    pub fn delete(name: &str, force: bool) -> Result<()> {
        let manifest = paths::profile_manifest(name);
        let old_dir = paths::profile_dir(name);
        let data = paths::data_dir(name);

        if !manifest.exists() && !old_dir.is_dir() {
            bail!("Profile '{}' does not exist", name);
        }

        let is_active = Self::get_active().as_deref() == Some(name);
        let is_default = Self::get_default().as_deref() == Some(name);

        if !force {
            eprintln!("Are you sure you want to delete profile '{}'? [y/N] ", name);
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim().to_lowercase() != "y" {
                println!("Deletion cancelled.");
                return Ok(());
            }
        }

        if manifest.exists() {
            fs::remove_file(&manifest).context("Failed to remove manifest")?;
        }
        if old_dir.is_dir() {
            fs::remove_dir_all(&old_dir).context("Failed to remove old-style profile directory")?;
        }
        if data.exists() {
            fs::remove_dir_all(&data).context("Failed to remove profile data")?;
        }

        let active_dir = paths::active_dir(name);
        if active_dir.exists() {
            fs::remove_dir_all(&active_dir).ok();
        }

        if is_active {
            let agent = paths::agent_dir();
            if agent.is_symlink() {
                fs::remove_file(&agent).ok();
            }
        }

        if is_default {
            let def_file = paths::default_file();
            if def_file.exists() {
                fs::remove_file(&def_file).ok();
            }
        }

        println!("✅ Deleted profile '{}'", name);
        Ok(())
    }

    /// Edit a profile's selections interactively.
    pub fn edit(name: &str) -> Result<()> {
        let manifest_path = paths::profile_manifest(name);
        if !manifest_path.exists() {
            bail!("Profile '{}' does not exist at {}", name, manifest_path.display());
        }

        let content = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read profile '{}'", name))?;
        let mut manifest: ProfileManifest = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse profile '{}'", name))?;

        println!("✏️  Editing selections for profile '{}'", name);

        // 1. Extensions
        let all_extensions = Self::list_pool_items("extensions");
        if all_extensions.is_empty() {
            println!("ℹ️  No extensions found in global pool.");
        } else {
            let defaults: Vec<bool> = all_extensions
                .iter()
                .map(|item| manifest.select.extensions.contains(item))
                .collect();

            let selections = MultiSelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Select Extensions (Space to toggle, Enter to confirm)")
                .items(&all_extensions)
                .defaults(&defaults)
                .interact_opt()?;

            if let Some(indices) = selections {
                manifest.select.extensions = indices
                    .into_iter()
                    .map(|idx| all_extensions[idx].clone())
                    .collect();
            }
        }

        // 2. Skills
        let all_skills = Self::list_pool_items("skills");
        if all_skills.is_empty() {
            println!("ℹ️  No skills found in global pool.");
        } else {
            let defaults: Vec<bool> = all_skills
                .iter()
                .map(|item| manifest.select.skills.contains(item))
                .collect();

            let selections = MultiSelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Select Skills (Space to toggle, Enter to confirm)")
                .items(&all_skills)
                .defaults(&defaults)
                .interact_opt()?;

            if let Some(indices) = selections {
                manifest.select.skills = indices
                    .into_iter()
                    .map(|idx| all_skills[idx].clone())
                    .collect();
            }
        }

        // 3. Prompts
        let all_prompts = Self::list_pool_items("prompts");
        if all_prompts.is_empty() {
            println!("ℹ️  No prompts found in global pool.");
        } else {
            let defaults: Vec<bool> = all_prompts
                .iter()
                .map(|item| manifest.select.prompts.contains(item))
                .collect();

            let selections = MultiSelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Select Prompts (Space to toggle, Enter to confirm)")
                .items(&all_prompts)
                .defaults(&defaults)
                .interact_opt()?;

            if let Some(indices) = selections {
                manifest.select.prompts = indices
                    .into_iter()
                    .map(|idx| all_prompts[idx].clone())
                    .collect();
            }
        }

        // Write manifest back
        let json = serde_json::to_string_pretty(&manifest)
            .context("Failed to serialize manifest")?;
        fs::write(&manifest_path, &json)
            .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

        println!("✅ Profile '{}' updated successfully.", name);

        // Auto-rebuild active view if active
        let active = Self::get_active();
        if active.as_deref() == Some(name) {
            println!("⚙️  Rebuilding active view to apply changes...");
            Self::use_profile(name)?;
        }

        Ok(())
    }

    fn list_pool_items(subdir: &str) -> Vec<String> {
        let path = paths::pool_dir().join(subdir);
        let mut items = Vec::new();
        if let Ok(entries) = fs::read_dir(&path) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    items.push(name.to_string());
                }
            }
        }
        items.sort();
        items
    }

    /// Migrate all old-style profiles to the new format.
    pub fn migrate() -> Result<()> {
        let root = paths::profiles_root();
        if !root.exists() {
            println!("No profiles to migrate.");
            return Ok(());
        }

        let mut migrated = 0;
        let mut skipped = 0;
        let active = Self::get_active();

        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && !path.is_symlink() {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        if name == ".active" { continue; }
                        if paths::profile_manifest(name).exists() {
                            skipped += 1;
                            continue;
                        }
                        match Self::migrate_one(name) {
                            Ok(()) => {
                                migrated += 1;
                                println!("  ✔ Migrated '{}'", name);
                                if active.as_deref() == Some(name) {
                                    if let Err(e) = Self::use_profile(name) {
                                        eprintln!("  ⚠ Failed to activate migrated active profile '{}': {}", name, e);
                                    }
                                }
                            }
                            Err(e) => { eprintln!("  ✘ Failed to migrate '{}': {}", name, e); }
                        }
                    }
                }
            }
        }

        println!("Migration complete: {} migrated, {} already up-to-date", migrated, skipped);
        Ok(())
    }

    // ─── Profile migration ──────────────────────────────────────

    /// Migrate a single old-style profile directory to the new format.
    fn migrate_one(name: &str) -> Result<()> {
        let old_dir = paths::profile_dir(name);
        if !old_dir.is_dir() {
            bail!("Profile directory '{}' does not exist", old_dir.display());
        }

        fs::create_dir_all(paths::pool_extensions_dir())?;
        fs::create_dir_all(paths::pool_skills_dir())?;
        fs::create_dir_all(paths::pool_prompts_dir())?;

        let mut manifest = ProfileManifest::default();

        // Migrate extensions
        let old_ext = old_dir.join("extensions");
        if old_ext.exists() {
            for entry in fs::read_dir(&old_ext)? {
                let entry = entry?;
                let item_name = entry.file_name().to_string_lossy().to_string();
                let src = entry.path();
                let dst = paths::pool_extensions_dir().join(&item_name);
                if !dst.exists() {
                    Self::copy_item(&src, &dst)?;
                }
                manifest.select.extensions.push(item_name);
            }
        }

        // Migrate skills
        let old_skills = old_dir.join("skills");
        if old_skills.exists() {
            for entry in fs::read_dir(&old_skills)? {
                let entry = entry?;
                let item_name = entry.file_name().to_string_lossy().to_string();
                let src = entry.path();
                let dst = paths::pool_skills_dir().join(&item_name);
                if !dst.exists() {
                    Self::copy_item(&src, &dst)?;
                }
                manifest.select.skills.push(item_name);
            }
        }

        // Migrate prompts
        let old_prompts = old_dir.join("prompts");
        if old_prompts.exists() {
            for entry in fs::read_dir(&old_prompts)? {
                let entry = entry?;
                let item_name = entry.file_name().to_string_lossy().to_string();
                let src = entry.path();
                let dst = paths::pool_prompts_dir().join(&item_name);
                if !dst.exists() {
                    Self::copy_item(&src, &dst)?;
                }
                manifest.select.prompts.push(item_name);
            }
        }

        // Migrate settings.json
        let settings_path = old_dir.join("settings.json");
        if settings_path.exists() {
            let content = fs::read_to_string(&settings_path)?;
            manifest.settings = serde_json::from_str(&content).unwrap_or(serde_json::Value::Null);
        }

        // Migrate mcp.json
        let mcp_path = old_dir.join(MCP_CONFIG_FILE);
        if mcp_path.exists() {
            let content = fs::read_to_string(&mcp_path)?;
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                manifest.mcp_servers = val.get("mcpServers").cloned().unwrap_or(serde_json::Value::Null);
                manifest.mcp_settings = val.get("settings").cloned().unwrap_or(serde_json::Value::Null);
            }
        }

        // Move state files to data dir
        let data_dir = paths::data_dir(name);
        fs::create_dir_all(&data_dir)?;

        for item in &["auth.json", "models.json", "trust.json", "APPEND_SYSTEM.md"] {
            let src = old_dir.join(item);
            if src.exists() {
                let dst = data_dir.join(item);
                if fs::rename(&src, &dst).is_err() {
                    Self::copy_item(&src, &dst)?;
                    fs::remove_file(&src).ok();
                }
            }
        }

        // Move sessions/
        let old_sessions = old_dir.join("sessions");
        if old_sessions.exists() {
            let data_sessions = data_dir.join("sessions");
            fs::create_dir_all(&data_sessions)?;
            if let Ok(entries) = fs::read_dir(&old_sessions) {
                for entry in entries.flatten() {
                    let src = entry.path();
                    let dst = data_sessions.join(entry.file_name());
                    if fs::rename(&src, &dst).is_err() {
                        Self::copy_item(&src, &dst)?;
                        fs::remove_file(&src).ok();
                    }
                }
            }
            fs::remove_dir(&old_sessions).ok();
        }

        // Write manifest
        fs::create_dir_all(paths::profiles_root())?;
        let json = serde_json::to_string_pretty(&manifest)?;
        fs::write(paths::profile_manifest(name), &json)?;

        // Remove old directory
        if old_dir.exists() {
            fs::remove_dir_all(&old_dir)?;
        }

        Ok(())
    }

    // ─── Helpers ─────────────────────────────────────────────────

    /// Build the effective agent directory from a manifest.
    fn build_from_manifest(manifest: &ProfileManifest, dst: &Path) -> Result<()> {
        for sub in &["extensions", "skills", "prompts"] {
            fs::create_dir_all(dst.join(sub))
                .with_context(|| format!("Failed to create {sub} directory"))?;
        }

        let pool = paths::pool_dir();

        for item in &manifest.select.extensions {
            let src = pool.join("extensions").join(item);
            let link = dst.join("extensions").join(item);
            if src.exists() {
                Self::symlink_item(&src, &link)?;
            } else {
                eprintln!("  ⚠ Extension '{}' not found in pool", item);
            }
        }

        for item in &manifest.select.skills {
            let src = pool.join("skills").join(item);
            let link = dst.join("skills").join(item);
            if src.exists() {
                Self::symlink_item(&src, &link)?;
            } else {
                eprintln!("  ⚠ Skill '{}' not found in pool", item);
            }
        }

        for item in &manifest.select.prompts {
            let src = pool.join("prompts").join(item);
            let link = dst.join("prompts").join(item);
            if src.exists() {
                Self::symlink_item(&src, &link)?;
            } else {
                eprintln!("  ⚠ Prompt '{}' not found in pool", item);
            }
        }

        if !manifest.settings.is_null() {
            let settings_str = serde_json::to_string_pretty(&manifest.settings)?;
            fs::write(dst.join("settings.json"), &settings_str)
                .context("Failed to write settings.json")?;
        }

        if !manifest.mcp_servers.is_null() || !manifest.mcp_settings.is_null() {
            let mut mcp = serde_json::Map::new();
            if !manifest.mcp_servers.is_null() {
                mcp.insert("mcpServers".to_string(), manifest.mcp_servers.clone());
            }
            if !manifest.mcp_settings.is_null() {
                mcp.insert("settings".to_string(), manifest.mcp_settings.clone());
            }
            let mcp_str = serde_json::to_string_pretty(&mcp)?;
            fs::write(dst.join(MCP_CONFIG_FILE), &mcp_str)
                .context("Failed to write mcp.json")?;
        }

        Ok(())
    }

    /// Link runtime state from the data directory into the active view.
    fn link_data_dir(name: &str, active_dir: &Path) -> Result<()> {
        let data = paths::data_dir(name);
        if !data.exists() {
            return Ok(());
        }

        for item in &["auth.json", "models.json", "trust.json", "APPEND_SYSTEM.md"] {
            let src = data.join(item);
            if src.exists() {
                Self::symlink_item(&src, &active_dir.join(item))?;
            }
        }

        let src_sessions = data.join("sessions");
        if src_sessions.exists() {
            let dst_sessions = active_dir.join("sessions");
            fs::create_dir_all(&dst_sessions)?;
            if let Ok(entries) = fs::read_dir(&src_sessions) {
                for entry in entries.flatten() {
                    let src = entry.path();
                    let dst = dst_sessions.join(entry.file_name());
                    if !dst.exists() {
                        Self::symlink_item(&src, &dst)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Copy a file or directory from src to dst.
    fn copy_item(src: &Path, dst: &Path) -> Result<()> {
        if src.is_dir() {
            fs::create_dir_all(dst)?;
            for entry in fs::read_dir(src)? {
                let entry = entry?;
                Self::copy_item(&entry.path(), &dst.join(entry.file_name()))?;
            }
        } else if src.is_symlink() {
            let target = fs::read_link(src)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, dst)?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&target, dst)?;
        } else {
            fs::copy(src, dst)?;
        }
        Ok(())
    }

    /// Create a symlink from `link` pointing to `target`.
    fn symlink_item(target: &Path, link: &Path) -> Result<()> {
        if link.exists() || link.is_symlink() {
            if link.is_dir() && !link.is_symlink() {
                fs::remove_dir_all(link).ok();
            } else {
                fs::remove_file(link).ok();
            }
        }
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent)?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link)
            .with_context(|| format!("Failed to symlink {} → {}", link.display(), target.display()))?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target, link)
            .with_context(|| format!("Failed to symlink {} → {}", link.display(), target.display()))?;
        Ok(())
    }

    /// Get the name of the default profile.
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
        if paths::profile_manifest(&name).exists() || paths::profile_dir(&name).is_dir() {
            Some(name)
        } else {
            None
        }
    }

    /// Get the name of the currently active profile by reading the `~/.pi/agent` symlink.
    pub fn get_active() -> Option<String> {
        let agent = paths::agent_dir();
        if !agent.is_symlink() {
            return None;
        }
        let target = fs::read_link(&agent).ok()?;
        let active_root = paths::active_root();
        let profiles_root = paths::profiles_root();

        if target.starts_with(&active_root) {
            target.components().next_back()
                .and_then(|c| c.as_os_str().to_str().map(|s| s.to_string()))
        } else if target.starts_with(&profiles_root) {
            target.file_name()?.to_str().map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Auto-heals the agent symlink if it points to the old-style profiles path or the old .merged path.
    pub fn auto_heal_symlink() -> Result<()> {
        let agent = paths::agent_dir();
        if !agent.is_symlink() {
            return Ok(());
        }
        let target = match fs::read_link(&agent) {
            Ok(t) => t,
            Err(_) => return Ok(()),
        };
        let profiles_root = paths::profiles_root();
        let old_merged_root = paths::pi_manager_root().join(".merged");

        if target.starts_with(&profiles_root) || target.starts_with(&old_merged_root) {
            if let Some(name) = target.file_name().and_then(|s| s.to_str()) {
                let manifest_exists = paths::profile_manifest(name).exists();
                let dir_exists = paths::profile_dir(name).is_dir();
                if manifest_exists || dir_exists {
                    println!("⚙️  Auto-healing active profile symlink for '{}'...", name);
                    Self::use_profile(name)?;
                }
            }
        }
        Ok(())
    }

    /// Count selected items in a profile manifest.
    fn count_selected(name: &str, category: &str) -> Result<usize> {
        let manifest_path = paths::profile_manifest(name);
        if !manifest_path.exists() {
            return Ok(0);
        }
        let content = fs::read_to_string(&manifest_path)?;
        let manifest: ProfileManifest = serde_json::from_str(&content)?;
        match category {
            "extensions" => Ok(manifest.select.extensions.len()),
            "skills" => Ok(manifest.select.skills.len()),
            "prompts" => Ok(manifest.select.prompts.len()),
            _ => Ok(0),
        }
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

        let abs_target = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());

        #[cfg(unix)]
        std::os::unix::fs::symlink(&abs_target, &agent).with_context(|| {
            format!("Failed to create symlink: {} → {}", agent.display(), abs_target.display())
        })?;

        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&abs_target, &agent).with_context(|| {
            format!("Failed to create symlink: {} → {}", agent.display(), abs_target.display())
        })?;

        Ok(())
    }
}

impl Default for ProfileManifest {
    fn default() -> Self {
        ProfileManifest {
            select: Selection::default(),
            settings: serde_json::Value::Null,
            mcp_servers: serde_json::Value::Null,
            mcp_settings: serde_json::Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn sandbox() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let home = tmp.path().to_path_buf();
        (tmp, home)
    }

    fn create_old_profile(home: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = home.join(".pi-manager").join("profiles").join(name);
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

    // ─── ProfileManifest ────────────────────────────────────────

    #[test]
    fn test_manifest_default_is_empty() {
        let m = ProfileManifest::default();
        assert!(m.select.extensions.is_empty());
        assert!(m.select.skills.is_empty());
        assert!(m.select.prompts.is_empty());
        assert!(m.settings.is_null());
    }

    #[test]
    fn test_manifest_serialize_deserialize() {
        let json = r#"{
            "select": {
                "extensions": ["rtk", "searxng"],
                "skills": ["web-research"]
            },
            "settings": { "theme": "dark" }
        }"#;
        let manifest: ProfileManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.select.extensions, vec!["rtk", "searxng"]);
        assert!(manifest.mcp_servers.is_null());
    }

    #[test]
    fn test_manifest_with_mcp() {
        let json = r#"{
            "select": { "extensions": [] },
            "mcpServers": { "fs": { "command": "npx" } },
            "mcpSettings": { "toolPrefix": "mcp" }
        }"#;
        let manifest: ProfileManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.mcp_servers["fs"]["command"], "npx");
    }

    // ─── copy_item ───────────────────────────────────────────────

    #[test]
    fn test_copy_item_file() {
        let (_tmp, home) = sandbox();
        let src = home.join("src.txt");
        let dst = home.join("dst.txt");
        fs::write(&src, "hello").unwrap();
        ProfileManager::copy_item(&src, &dst).unwrap();
        assert_eq!(read_file(&dst), "hello");
    }

    #[test]
    fn test_copy_item_directory() {
        let (_tmp, home) = sandbox();
        let src = home.join("src_dir");
        let dst = home.join("dst_dir");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub").join("f.txt"), "content").unwrap();
        ProfileManager::copy_item(&src, &dst).unwrap();
        assert_eq!(read_file(&dst.join("sub").join("f.txt")), "content");
    }

    // ─── symlink_item ───────────────────────────────────────────

    #[test]
    fn test_symlink_item_file() {
        let (_tmp, home) = sandbox();
        let target = home.join("target.txt");
        let link = home.join("link.txt");
        fs::write(&target, "hello").unwrap();
        ProfileManager::symlink_item(&target, &link).unwrap();
        assert!(link.is_symlink());
        assert_eq!(read_file(&link), "hello");
    }

    // ─── build_from_manifest ────────────────────────────────────

    #[test]
    fn test_build_empty_manifest_creates_dirs() {
        let (_tmp, home) = sandbox();
        let manifest = ProfileManifest::default();
        let dst = home.join("active");
        ProfileManager::build_from_manifest(&manifest, &dst).unwrap();
        assert!(dst.join("extensions").is_dir());
        assert!(dst.join("skills").is_dir());
    }

    #[test]
    fn test_build_manifest_produces_settings() {
        let json = r#"{
            "select": { "extensions": [], "skills": [] },
            "settings": { "theme": "dark" }
        }"#;
        let manifest: ProfileManifest = serde_json::from_str(json).unwrap();
        let (_tmp, home) = sandbox();
        let dst = home.join("active");
        ProfileManager::build_from_manifest(&manifest, &dst).unwrap();
        assert!(dst.join("settings.json").exists());
        let content = read_file(&dst.join("settings.json"));
        let s: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(s["theme"], "dark");
    }

    #[test]
    fn test_build_manifest_with_mcp() {
        let json = r#"{
            "select": { "extensions": [], "skills": [] },
            "mcpServers": { "fs": { "command": "npx" } },
            "mcpSettings": { "toolPrefix": "mcp" }
        }"#;
        let manifest: ProfileManifest = serde_json::from_str(json).unwrap();
        let (_tmp, home) = sandbox();
        let dst = home.join("active");
        ProfileManager::build_from_manifest(&manifest, &dst).unwrap();
        assert!(dst.join("mcp.json").exists());
    }

    // ─── Tests that modify HOME (must run sequentially) ────────
    //
    // These tests set $HOME to a tempdir so paths.rs resolves inside it.
    // A static Mutex prevents parallel execution (which would race on HOME).

    use std::sync::Mutex;
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    /// Run a closure with HOME temporarily pointing to `home`.
    fn with_home<T>(home: &Path, f: impl FnOnce() -> T) -> T {
        let _guard = HOME_LOCK.lock().unwrap();
        let old_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", home);
        let result = f();
        if let Some(h) = old_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
        result
    }

    #[test]
    fn test_migrate_one_basic() {
        let (_tmp, home) = sandbox();
        let name = "test-profile";
        create_old_profile(&home, name, &[
            ("extensions/rtk.ts", "// extension"),
            ("skills/web-research/SKILL.md", "# skill"),
            ("settings.json", r#"{"theme": "dark"}"#),
            ("auth.json", r#"{"apiKey": "sk-..."}"#),
        ]);

        with_home(&home, || {
            fs::create_dir_all(home.join(".pi-manager").join("pool").join("extensions")).unwrap();
            fs::create_dir_all(home.join(".pi-manager").join("pool").join("skills")).unwrap();

            let result = ProfileManager::migrate_one(name);
            assert!(result.is_ok(), "migrate_one failed: {:?}", result.err());
        });

        let mpath = home.join(".pi-manager").join("profiles").join(format!("{}.json", name));
        assert!(mpath.exists(), "manifest should exist");
        let content = read_file(&mpath);
        let manifest: ProfileManifest = serde_json::from_str(&content).unwrap();
        assert_eq!(manifest.select.extensions, vec!["rtk.ts"]);
        assert_eq!(manifest.select.skills, vec!["web-research"]);

        assert!(home.join(".pi-manager").join("pool").join("extensions").join("rtk.ts").exists());
        assert!(home.join(".pi-manager").join("data").join(name).join("auth.json").exists());
        assert!(!home.join(".pi-manager").join("profiles").join(name).exists());
    }

    #[test]
    fn test_migrate_one_with_mcp() {
        let (_tmp, home) = sandbox();
        let name = "test-mcp";
        let mcp_json = r#"{"mcpServers":{"fs":{"command":"npx"}},"settings":{"toolPrefix":"mcp"}}"#;
        create_old_profile(&home, name, &[
            ("mcp.json", mcp_json),
            ("settings.json", r#"{"theme":"light"}"#),
            ("auth.json", "{}"),
        ]);

        with_home(&home, || {
            fs::create_dir_all(home.join(".pi-manager").join("pool").join("extensions")).unwrap();
            let result = ProfileManager::migrate_one(name);
            assert!(result.is_ok(), "migrate_one failed: {:?}", result.err());
        });

        let content = read_file(&home.join(".pi-manager").join("profiles").join("test-mcp.json"));
        let manifest: ProfileManifest = serde_json::from_str(&content).unwrap();
        assert_eq!(manifest.mcp_servers["fs"]["command"], "npx");
        assert_eq!(manifest.mcp_settings["toolPrefix"], "mcp");
    }

    #[test]
    fn test_get_active_no_symlink() {
        let (_tmp, home) = sandbox();
        with_home(&home, || {
            assert!(ProfileManager::get_active().is_none());
        });
    }

    #[test]
    fn test_get_default_no_file() {
        let (_tmp, home) = sandbox();
        with_home(&home, || {
            assert!(ProfileManager::get_default().is_none());
        });
    }

    #[test]
    fn test_create_empty_profile() {
        let (_tmp, home) = sandbox();
        with_home(&home, || {
            fs::create_dir_all(home.join(".pi-manager").join("profiles")).unwrap();
            let result = ProfileManager::create("empty", false, None);
            assert!(result.is_ok(), "create failed: {:?}", result.err());
        });

        let mpath = home.join(".pi-manager").join("profiles").join("empty.json");
        assert!(mpath.exists());
        let manifest: ProfileManifest = serde_json::from_str(&read_file(&mpath)).unwrap();
        assert!(manifest.select.extensions.is_empty());
    }

    #[test]
    fn test_auto_heal_symlink() {
        let (_tmp, home) = sandbox();
        let name = "test-heal";
        create_old_profile(&home, name, &[
            ("settings.json", r#"{"theme": "dark"}"#),
        ]);

        with_home(&home, || {
            let agent = paths::agent_dir();
            fs::create_dir_all(agent.parent().unwrap()).unwrap();
            let old_dir = paths::profile_dir(name);
            #[cfg(unix)]
            std::os::unix::fs::symlink(&old_dir, &agent).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(&old_dir, &agent).unwrap();

            let result = ProfileManager::auto_heal_symlink();
            assert!(result.is_ok(), "auto_heal failed: {:?}", result.err());

            assert!(agent.is_symlink());
            let target = fs::read_link(&agent).unwrap();
            assert_eq!(target, paths::active_dir(name));
        });
    }

    #[test]
    fn test_auto_heal_broken_symlink() {
        let (_tmp, home) = sandbox();
        let name = "test-heal-broken";
        
        with_home(&home, || {
            fs::create_dir_all(paths::profiles_root()).unwrap();
            fs::write(paths::profile_manifest(name), r#"{"select":{"extensions":[],"skills":[]}}"#).unwrap();

            let agent = paths::agent_dir();
            fs::create_dir_all(agent.parent().unwrap()).unwrap();
            let old_dir = paths::profile_dir(name);
            #[cfg(unix)]
            std::os::unix::fs::symlink(&old_dir, &agent).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(&old_dir, &agent).unwrap();

            let result = ProfileManager::auto_heal_symlink();
            assert!(result.is_ok(), "auto_heal failed: {:?}", result.err());

            assert!(agent.is_symlink());
            let target = fs::read_link(&agent).unwrap();
            assert_eq!(target, paths::active_dir(name));
            assert!(paths::active_dir(name).exists());
        });
    }

    #[test]
    fn test_auto_heal_no_op_for_valid_active_link() {
        let (_tmp, home) = sandbox();
        let name = "test-valid";
        with_home(&home, || {
            fs::create_dir_all(paths::profiles_root()).unwrap();
            fs::write(paths::profile_manifest(name), r#"{"select":{"extensions":[],"skills":[]}}"#).unwrap();

            ProfileManager::use_profile(name).unwrap();

            let agent = paths::agent_dir();
            let target_before = fs::read_link(&agent).unwrap();

            ProfileManager::auto_heal_symlink().unwrap();

            let target_after = fs::read_link(&agent).unwrap();
            assert_eq!(target_before, target_after);
        });
    }
}
