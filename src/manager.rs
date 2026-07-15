use crate::paths;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const MCP_CONFIG_FILE: &str = "mcp.json";
const CONFIG_TEMPLATE_FILE: &str = "config.default.json";

/// Global pim configuration stored at `~/.pim/pim.json`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PimConfig {
    #[serde(default)]
    default_profile: Option<String>,
}

/// A profile manifest — the recipe inside each profile's directory.
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
    /// Per-profile config overrides. Keyed by filename (e.g. "searxng.json").
    /// These take priority over pool defaults (`config.default.json`).
    #[serde(default)]
    configs: HashMap<String, serde_json::Value>,

    /// Per-profile environment variables injected when launching pi.
    #[serde(default)]
    env: HashMap<String, String>,
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
    /// MCP servers selected from pool/mcp/.
    #[serde(default)]
    mcp_servers: Vec<String>,
}

pub struct ProfileManager;

impl ProfileManager {
    // ─── Public API ──────────────────────────────────────────────

    /// Create a new profile manifest.
    pub fn create(name: &str, from_base: bool, from: Option<&str>) -> Result<()> {
        let dir = paths::profile_dir(name);
        let manifest_path = paths::profile_manifest(name);
        if manifest_path.exists() {
            bail!(
                "Profile '{}' already exists at {}",
                name,
                manifest_path.display()
            );
        }
        // Block creation if an old-style manifest file exists (needs migration)
        let old_manifest = paths::profiles_root().join(format!("{name}.json"));
        if old_manifest.is_file() {
            bail!("Profile '{name}' exists in old format. Run 'pim migrate' first.");
        }
        if dir.is_dir() && !dir.join("manifest.json").exists() {
            bail!("A directory for profile '{name}' exists but has no manifest.json.");
        }

        let manifest = if let Some(src) = from {
            let src_path = paths::profile_manifest(src);
            let content = fs::read_to_string(&src_path)
                .with_context(|| format!("Failed to read profile '{src}'"))?;
            serde_json::from_str::<ProfileManifest>(&content)
                .with_context(|| format!("Failed to parse profile '{src}'"))?
        } else if from_base {
            match Self::get_active() {
                Some(ref name) => {
                    let src_path = paths::profile_manifest(name);
                    if src_path.exists() {
                        let content = fs::read_to_string(&src_path)?;
                        serde_json::from_str(&content)?
                    } else {
                        bail!("Active profile '{name}' has no manifest.json")
                    }
                }
                None => bail!("No active profile to copy from"),
            }
        } else {
            ProfileManifest::default()
        };

        fs::create_dir_all(&dir)?;
        let json =
            serde_json::to_string_pretty(&manifest).context("Failed to serialize manifest")?;
        fs::write(&manifest_path, &json)
            .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

        println!("✅ Created profile '{name}'");
        Ok(())
    }

    /// List all available profiles.
    #[allow(clippy::unnecessary_wraps)]
    pub fn list() -> Result<()> {
        let root = paths::profiles_root();
        if !root.exists() {
            println!("No profiles found. Create one with: pim create <name>");
            return Ok(());
        }

        let default = Self::get_default();
        let active = Self::get_active();
        let mut found = false;

        let mut names = Self::list_profile_names();
        names.sort();

        for name in &names {
            let is_active = active.as_deref() == Some(name.as_str());
            let is_default = default.as_deref() == Some(name.as_str());
            let mut markers = Vec::new();
            if is_active {
                markers.push("active");
            }
            if is_default {
                markers.push("default");
            }
            let suffix = if markers.is_empty() {
                String::new()
            } else {
                format!(" ◀ {}", markers.join(", "))
            };
            println!("  {name}{suffix}");
            found = true;
        }

        if !found {
            println!("No profiles found. Create one with: pim create <name>");
        }

        Ok(())
    }

    /// Show current status.
    #[allow(clippy::unnecessary_wraps)]
    pub fn status() -> Result<()> {
        let active = Self::get_active();
        let default = Self::get_default();

        if let Some(name) = &active {
            let manifest_path = paths::profile_manifest(name);
            if manifest_path.exists() {
                let ext_count = Self::count_selected(name, "extensions").unwrap_or(0);
                let skill_count = Self::count_selected(name, "skills").unwrap_or(0);
                let config_count = Self::count_selected(name, "configs").unwrap_or(0);
                println!(
                    "Active profile: {name} ({ext_count} extensions, {skill_count} skills, {config_count} configs)"
                );
            } else {
                println!("Active profile: {name}");
            }
        } else {
            println!("No active profile.");
        }

        match &default {
            Some(name) => println!("Default profile: {name}"),
            None => println!("No default profile set"),
        }

        Ok(())
    }

    /// Set a profile as the default and build/refresh its agent directory.
    ///
    /// The profile directory (`profiles/<name>/`) IS the pi coding agent folder.
    /// Pool items are symlinked in, config defaults are auto-seeded from pool
    /// templates, and runtime state (sessions, auth) already lives here.
    pub fn set_default(name: &str) -> Result<()> {
        Self::build(name)?;

        // Set this as default so `pim` (no args) launches it
        let mut config = Self::read_config();
        config.default_profile = Some(name.to_string());
        Self::write_config(&config)?;

        Ok(())
    }

    /// Build/refresh a profile's agent directory from its manifest.
    /// Creates pool dirs, symlinks items, seeds configs — but does **not**
    /// change the default profile. Use `set_default` when you also want to
    /// persist the profile as the default.
    fn build(name: &str) -> Result<()> {
        fs::create_dir_all(paths::pool_extensions_dir())?;
        fs::create_dir_all(paths::pool_skills_dir())?;
        fs::create_dir_all(paths::pool_prompts_dir())?;

        let manifest_path = paths::profile_manifest(name);
        if !manifest_path.exists() {
            bail!(
                "Profile '{}' does not exist at {}",
                name,
                manifest_path.display()
            );
        }

        let content = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read profile '{name}'"))?;
        let manifest: ProfileManifest = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse profile '{name}'"))?;

        let profile_dir = paths::profile_dir(name);

        // Build the symlink forest and generate files
        Self::build_profile(&manifest, &profile_dir)?;

        Ok(())
    }

    /// Activate the default profile (invoked via bare `pim` with no arguments).
    #[allow(dead_code)]
    pub fn use_default() -> Result<()> {
        if let Some(name) = Self::get_default() {
            Self::set_default(&name)
        } else {
            println!("No default profile set.");
            Self::status()
        }
    }

    /// Delete a profile and everything inside it.
    pub fn delete(name: &str, force: bool) -> Result<()> {
        let profile_dir = paths::profile_dir(name);
        if !profile_dir.exists() {
            bail!("Profile '{name}' does not exist");
        }

        let is_default = Self::get_default().as_deref() == Some(name);

        if !force {
            eprintln!("Are you sure you want to delete profile '{name}'? [y/N] ");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim().to_lowercase() != "y" {
                println!("Deletion cancelled.");
                return Ok(());
            }
        }

        // Also clean up old-format standalone manifest if present
        let old_manifest = paths::profiles_root().join(format!("{name}.json"));
        if old_manifest.exists() {
            fs::remove_file(&old_manifest).ok();
        }

        // Remove the entire profile directory (manifest, configs, sessions, auth, symlinks)
        if profile_dir.exists() {
            fs::remove_dir_all(&profile_dir)
                .with_context(|| format!("Failed to remove profile '{name}'"))?;
        }

        if is_default {
            let mut config = Self::read_config();
            config.default_profile = None;
            Self::write_config(&config).ok();
        }

        println!("✅ Deleted profile '{name}'");
        Ok(())
    }

    /// Edit a profile's selections interactively (TUI).
    pub fn edit(name: &str) -> Result<()> {
        let manifest_path = paths::profile_manifest(name);
        if !manifest_path.exists() {
            bail!(
                "Profile '{}' does not exist at {}",
                name,
                manifest_path.display()
            );
        }

        let content = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read profile '{name}'"))?;
        let manifest: ProfileManifest = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse profile '{name}'"))?;

        println!("✨ Opening TUI editor for profile '{name}'...");

        // Run the TUI
        let result = crate::tui::run_editor(
            name,
            &manifest.select.extensions,
            &manifest.select.skills,
            &manifest.select.prompts,
            &manifest.select.mcp_servers,
        )?;

        // Apply selections from the TUI result
        if !result.changed {
            let same_exts =
                Self::sets_equal(&result.selected_extensions, &manifest.select.extensions);
            let same_skills = Self::sets_equal(&result.selected_skills, &manifest.select.skills);
            let same_prompts = Self::sets_equal(&result.selected_prompts, &manifest.select.prompts);
            let same_mcp =
                Self::sets_equal(&result.selected_mcp_servers, &manifest.select.mcp_servers);
            if same_exts && same_skills && same_prompts && same_mcp {
                println!("ℹ️ No changes made.");
                return Ok(());
            }
        }

        // Write manifest back
        let mut new_manifest = manifest;
        new_manifest.select.extensions = result.selected_extensions;
        new_manifest.select.skills = result.selected_skills;
        new_manifest.select.prompts = result.selected_prompts;
        new_manifest.select.mcp_servers = result.selected_mcp_servers;

        let json =
            serde_json::to_string_pretty(&new_manifest).context("Failed to serialize manifest")?;
        fs::write(&manifest_path, &json)
            .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

        println!("✅ Profile '{name}' updated successfully.");

        // Auto-rebuild if active
        let active = Self::get_active();
        if active.as_deref() == Some(name) {
            println!("⚙️  Rebuilding active view to apply changes...");
            Self::set_default(name)?;
        }

        Ok(())
    }

    /// Check if two string slices contain the same elements (order-independent).
    fn sets_equal(a: &[String], b: &[String]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut a_sorted = a.to_vec();
        let mut b_sorted = b.to_vec();
        a_sorted.sort();
        b_sorted.sort();
        a_sorted == b_sorted
    }

    // ─── Pool introspection ─────────────────────────────────────

    #[allow(dead_code)]
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

    // ─── Profile building ───────────────────────────────────────

    /// Build (or rebuild) a profile's agent directory from its manifest.
    ///
    /// * Symlinks pool items (extensions, skills, prompts) into the profile dir
    /// * Seeds config files from pool templates (`config.default.json`)
    /// * Writes manifest `configs` overrides
    /// * Generates `settings.json` and `mcp.json`
    ///
    /// Runtime state (sessions, auth, models) already live in the profile dir
    /// and are left untouched.
    #[allow(clippy::too_many_lines)]
    fn build_profile(manifest: &ProfileManifest, profile_dir: &Path) -> Result<()> {
        const RUNTIME_KEYS: &[&str] = &["lastChangelogVersion"];

        // Ensure subdirectories exist
        for sub in &["extensions", "skills", "prompts", "config", "sessions"] {
            fs::create_dir_all(profile_dir.join(sub))
                .with_context(|| format!("Failed to create {sub} directory"))?;
        }
        // Ensure pool MCP directory exists for reading server configs
        fs::create_dir_all(paths::pool_mcp_dir()).context("Failed to create pool MCP directory")?;

        let pool = paths::pool_dir();

        // --- Symlink extensions ---
        for item in &manifest.select.extensions {
            let src = pool.join("extensions").join(item);
            let link = profile_dir.join("extensions").join(item);
            if src.exists() {
                Self::symlink_item(&src, &link)?;
            } else {
                eprintln!("  ⚠ Extension '{item}' not found in pool");
            }
        }

        // --- Symlink skills ---
        for item in &manifest.select.skills {
            let src = pool.join("skills").join(item);
            let link = profile_dir.join("skills").join(item);
            if src.exists() {
                Self::symlink_item(&src, &link)?;
            } else {
                eprintln!("  ⚠ Skill '{item}' not found in pool");
            }
        }

        // --- Symlink prompts ---
        for item in &manifest.select.prompts {
            let src = pool.join("prompts").join(item);
            let link = profile_dir.join("prompts").join(item);
            if src.exists() {
                Self::symlink_item(&src, &link)?;
            } else {
                eprintln!("  ⚠ Prompt '{item}' not found in pool");
            }
        }

        // --- Seed configs from pool templates ---
        // For each selected item that is a directory with a config.default.json,
        // copy it into config/<item-name>.json (only if not already present
        // and not overridden by the manifest's configs).
        let item_types: &[(&str, &str)] = &[
            ("extensions", "extensions"),
            ("skills", "skills"),
            ("prompts", "prompts"),
        ];

        for &(_field, subdir) in item_types {
            let pool_subdir = pool.join(subdir);
            let config_dir = profile_dir.join("config");
            for item in manifest.select.for_subdir(subdir) {
                let pool_item_path = pool_subdir.join(item);
                // Only directories can carry a config template
                if !pool_item_path.is_dir() {
                    continue;
                }
                let template = pool_item_path.join(CONFIG_TEMPLATE_FILE);
                if !template.exists() {
                    continue;
                }
                let config_name = format!("{item}.json");
                let config_path = config_dir.join(&config_name);
                // Skip if already exists (user has modified it) or manifest overrides it
                if config_path.exists() || manifest.configs.contains_key(&config_name) {
                    continue;
                }
                fs::copy(&template, &config_path)
                    .with_context(|| format!("Failed to copy config template for '{item}'"))?;
            }
        }

        // --- Write manifest config overrides ---
        let config_dir = profile_dir.join("config");
        for (filename, content) in &manifest.configs {
            let content_str = serde_json::to_string_pretty(content)
                .with_context(|| format!("Failed to serialize config '{filename}'"))?;
            fs::write(config_dir.join(filename), &content_str)
                .with_context(|| format!("Failed to write config '{filename}'"))?;
        }

        // --- Generate settings.json ---
        // Merge manifest settings with existing runtime settings (e.g.
        // lastChangelogVersion written by pi). Manifest values take priority.
        let settings_path = profile_dir.join("settings.json");
        let mut merged = if let Ok(content) = fs::read_to_string(&settings_path) {
            serde_json::from_str::<serde_json::Value>(&content).unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        };

        if let Some(existing) = merged.as_object_mut() {
            if let Some(manifest_obj) = manifest.settings.as_object() {
                for (key, val) in manifest_obj {
                    if !RUNTIME_KEYS.contains(&key.as_str()) {
                        existing.insert(key.clone(), val.clone());
                    }
                }
            }
        } else {
            merged = manifest.settings.clone();
            // Strip runtime keys from fresh manifest settings
            if let Some(obj) = merged.as_object_mut() {
                for key in RUNTIME_KEYS {
                    obj.remove(*key);
                }
            }
        }

        if merged.is_null() || (merged.as_object().is_some_and(serde_json::Map::is_empty)) {
            if settings_path.exists() {
                fs::remove_file(&settings_path).ok();
            }
        } else {
            let settings_str = serde_json::to_string_pretty(&merged)?;
            fs::write(&settings_path, &settings_str).context("Failed to write settings.json")?;
        }

        // --- Generate mcp.json ---
        // Builds from pool/mcp/<name>/mcp.json for each selected server,
        // with template substitution from config/<name>.json.
        // Falls back to inline mcpServers from the manifest for backward compat.
        let mcp_path = profile_dir.join(MCP_CONFIG_FILE);
        let config_dir = profile_dir.join("config");

        let mut merged_servers = serde_json::Map::new();

        // 1. Pool-based selections
        for server_name in &manifest.select.mcp_servers {
            let pool_entry = paths::pool_mcp_dir().join(server_name).join("mcp.json");
            if let Ok(content) = fs::read_to_string(&pool_entry) {
                if let Ok(mut config_value) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(config_obj) = config_value.as_object_mut() {
                        // Strip TUI metadata — not a real MCP server config field
                        config_obj.remove("config_fields");

                        // Load per-profile config for template substitution
                        let server_config_path = config_dir.join(format!("{server_name}.json"));
                        if let Ok(config_content) = fs::read_to_string(&server_config_path) {
                            if let Ok(user_config) =
                                serde_json::from_str::<serde_json::Value>(&config_content)
                            {
                                Self::substitute_templates(&mut config_value, &user_config);
                            }
                        }

                        merged_servers.insert(server_name.clone(), config_value);
                    }
                }
            } else {
                eprintln!("  ⚠ MCP server '{server_name}' not found in pool");
            }
        }

        // 2. Backward compat: inline mcpServers (not overridden by pool selection)
        if let Some(inline_servers) = manifest.mcp_servers.as_object() {
            for (name, config) in inline_servers {
                if !merged_servers.contains_key(name) {
                    merged_servers.insert(name.clone(), config.clone());
                }
            }
        }

        // 3. Write mcp.json if we have servers or settings
        if !merged_servers.is_empty() || !manifest.mcp_settings.is_null() {
            let mut mcp = serde_json::Map::new();
            mcp.insert(
                "mcpServers".to_string(),
                serde_json::Value::Object(merged_servers),
            );
            if !manifest.mcp_settings.is_null() {
                mcp.insert("settings".to_string(), manifest.mcp_settings.clone());
            }
            let mcp_str = serde_json::to_string_pretty(&mcp)?;
            fs::write(&mcp_path, &mcp_str).context("Failed to write mcp.json")?;
        } else if mcp_path.exists() {
            fs::remove_file(&mcp_path).ok();
        }

        Ok(())
    }

    /// Recursively walk a JSON value and substitute {key} placeholders
    /// with values from the user config.
    fn substitute_templates(value: &mut serde_json::Value, config: &serde_json::Value) {
        match value {
            serde_json::Value::String(s) => {
                if let Some(config_obj) = config.as_object() {
                    for (key, val) in config_obj {
                        let placeholder = format!("{{{key}}}");
                        if s.contains(&placeholder) {
                            let replacement = match val {
                                serde_json::Value::String(v) => v.clone(),
                                other => other.to_string(),
                            };
                            *s = s.replace(&placeholder, &replacement);
                        }
                    }
                }
            }
            serde_json::Value::Object(obj) => {
                for (_key, val) in obj.iter_mut() {
                    Self::substitute_templates(val, config);
                }
            }
            serde_json::Value::Array(arr) => {
                for val in arr.iter_mut() {
                    Self::substitute_templates(val, config);
                }
            }
            _ => {}
        }
    }

    // ─── Helpers ─────────────────────────────────────────────────

    /// Copy a file or directory from src to dst.
    #[allow(dead_code)]
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
        std::os::unix::fs::symlink(target, link).with_context(|| {
            format!(
                "Failed to symlink {} → {}",
                link.display(),
                target.display()
            )
        })?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target, link).with_context(|| {
            format!(
                "Failed to symlink {} → {}",
                link.display(),
                target.display()
            )
        })?;
        Ok(())
    }

    // ─── Config ──────────────────────────────────────────────────

    fn read_config() -> PimConfig {
        let path = paths::pim_config();
        if path.exists() {
            return fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(PimConfig {
                    default_profile: None,
                });
        }

        // One-time migration from old plain-text `default` file
        let old_default = paths::pi_manager_root().join("default");
        if old_default.exists() {
            if let Ok(name) = fs::read_to_string(&old_default) {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    let config = PimConfig {
                        default_profile: Some(name),
                    };
                    if let Ok(json) = serde_json::to_string_pretty(&config) {
                        fs::write(&path, &json).ok();
                        fs::remove_file(&old_default).ok();
                    }
                    return config;
                }
            }
        }

        PimConfig {
            default_profile: None,
        }
    }

    fn write_config(config: &PimConfig) -> Result<()> {
        let path = paths::pim_config();
        fs::create_dir_all(path.parent().unwrap())?;
        let json = serde_json::to_string_pretty(config)?;
        fs::write(&path, &json).with_context(|| "Failed to write pim.json")
    }

    /// Get the name of the default profile.
    pub fn get_default() -> Option<String> {
        let config = Self::read_config();
        let name = config.default_profile.as_deref()?;
        if name.is_empty() {
            return None;
        }
        // Check either new format or old format
        if paths::profile_manifest(name).exists()
            || paths::profiles_root()
                .join(format!("{name}.json"))
                .is_file()
        {
            Some(name.to_string())
        } else {
            None
        }
    }

    /// Get the name of the currently set default profile.
    pub fn get_active() -> Option<String> {
        Self::get_default()
    }

    /// No-op: symlink healing is no longer needed.
    #[allow(dead_code)]
    pub fn auto_heal_symlink() {}

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
            "mcp_servers" => Ok(manifest.select.mcp_servers.len()),
            "configs" => Ok(manifest.configs.len()),
            _ => Ok(0),
        }
    }

    /// Launch pi with the given profile by setting `PI_CODING_AGENT_DIR`.
    ///
    /// This replaces the current process with pi (Unix exec) so signals
    /// and Ctrl+C go directly to pi. Multiple pi instances can run
    /// simultaneously from different terminals, each with their own profile.
    pub fn launch_pi(profile: &str, pi_args: &[String]) -> Result<()> {
        let profile_dir = paths::profile_dir(profile);
        // Build/rebuild symlinks and configs from manifest before launching,
        // so that edits are reflected. Does **not** change the default profile.
        if profile_dir.join("manifest.json").exists() {
            Self::build(profile)?;
        } else {
            bail!("Profile '{profile}' does not exist — create it with 'pim create {profile}'");
        }

        // Read manifest to inject per-profile env vars
        let manifest_content = fs::read_to_string(profile_dir.join("manifest.json"))
            .with_context(|| format!("Failed to read manifest for profile '{profile}'"))?;
        let manifest: ProfileManifest = serde_json::from_str(&manifest_content)
            .with_context(|| format!("Failed to parse manifest for profile '{profile}'"))?;

        #[cfg(unix)]
        {
            let mut cmd = std::process::Command::new("pi");
            cmd.args(pi_args);
            cmd.env("PI_CODING_AGENT_DIR", &profile_dir);

            // Inject per-profile env vars — override parent shell values
            for (key, val) in &manifest.env {
                cmd.env(key, val);
            }

            let err = cmd.exec();
            bail!("Failed to exec pi: {err}");
        }

        #[cfg(not(unix))]
        {
            let mut cmd = std::process::Command::new("pi");
            cmd.args(pi_args);
            cmd.env("PI_CODING_AGENT_DIR", &profile_dir);
            for (key, val) in &manifest.env {
                cmd.env(key, val);
            }
            let status = cmd.status().context("Failed to run pi")?;
            std::process::exit(status.code().unwrap_or(0));
        }
    }

    /// Return a sorted list of all profile directory names.
    pub fn list_profile_names() -> Vec<String> {
        let mut profiles = Vec::new();
        let root = paths::profiles_root();
        if !root.exists() {
            return profiles;
        }
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                // New format: directory with manifest.json inside
                if path.is_dir() && path.join("manifest.json").exists() {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        profiles.push(name.to_string());
                    }
                }
            }
        }
        profiles.sort();
        profiles
    }
}

impl Default for ProfileManifest {
    fn default() -> Self {
        ProfileManifest {
            select: Selection::default(),
            settings: serde_json::Value::Null,
            mcp_servers: serde_json::Value::Null,
            mcp_settings: serde_json::Value::Null,
            configs: HashMap::new(),
            env: HashMap::new(),
        }
    }
}

// Small helper to iterate selections by subdir name
impl Selection {
    fn for_subdir(&self, subdir: &str) -> &[String] {
        match subdir {
            "extensions" => &self.extensions,
            "skills" => &self.skills,
            "prompts" => &self.prompts,
            "mcp" => &self.mcp_servers,
            _ => &[],
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(unsafe_code)]
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn sandbox() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let home = tmp.path().to_path_buf();
        (tmp, home)
    }

    fn read_file(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn create_manifest_json(home: &Path, name: &str, json: &str) {
        let dir = home.join(".pim").join("profiles").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.json"), json).unwrap();
    }

    // ─── ProfileManifest ────────────────────────────────────────

    #[test]
    fn test_manifest_default_is_empty() {
        let m = ProfileManifest::default();
        assert!(m.select.extensions.is_empty());
        assert!(m.select.skills.is_empty());
        assert!(m.select.prompts.is_empty());
        assert!(m.settings.is_null());
        assert!(m.configs.is_empty());
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

    // ─── build_profile ──────────────────────────────────────────

    #[test]
    fn test_build_empty_manifest_creates_dirs() {
        let (_tmp, home) = sandbox();
        let manifest = ProfileManifest::default();
        let dst = home.join("profile");
        ProfileManager::build_profile(&manifest, &dst).unwrap();
        assert!(dst.join("extensions").is_dir());
        assert!(dst.join("skills").is_dir());
        assert!(dst.join("config").is_dir());
        assert!(dst.join("sessions").is_dir());
    }

    #[test]
    fn test_build_manifest_produces_settings() {
        let json = r#"{
            "select": { "extensions": [], "skills": [] },
            "settings": { "theme": "dark" }
        }"#;
        let manifest: ProfileManifest = serde_json::from_str(json).unwrap();
        let (_tmp, home) = sandbox();
        let dst = home.join("profile");
        ProfileManager::build_profile(&manifest, &dst).unwrap();
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
        let dst = home.join("profile");
        ProfileManager::build_profile(&manifest, &dst).unwrap();
        assert!(dst.join("mcp.json").exists());
    }

    // ─── Config seeding from pool templates ──────────────────────

    #[test]
    fn test_build_seeds_config_from_pool_template() {
        let (_tmp, home) = sandbox();
        with_home(&home, || {
            // Create pool extension with config template
            let pool_ext = paths::pool_extensions_dir().join("searxng");
            fs::create_dir_all(&pool_ext).unwrap();
            fs::write(
                pool_ext.join("config.default.json"),
                r#"{"baseUrl": "http://localhost:8888"}"#,
            )
            .unwrap();

            let json = r#"{"select":{"extensions":["searxng"],"skills":[]}}"#;
            let manifest: ProfileManifest = serde_json::from_str(json).unwrap();
            let dst = home.join("profile");
            ProfileManager::build_profile(&manifest, &dst).unwrap();

            let config_path = dst.join("config").join("searxng.json");
            assert!(
                config_path.exists(),
                "config should be seeded from template"
            );
            let content: serde_json::Value =
                serde_json::from_str(&read_file(&config_path)).unwrap();
            assert_eq!(content["baseUrl"], "http://localhost:8888");
        });
    }

    #[test]
    fn test_build_config_override_from_manifest() {
        let (_tmp, home) = sandbox();
        with_home(&home, || {
            // Pool has a default
            let pool_ext = paths::pool_extensions_dir().join("searxng");
            fs::create_dir_all(&pool_ext).unwrap();
            fs::write(
                pool_ext.join("config.default.json"),
                r#"{"baseUrl": "http://localhost:8888"}"#,
            )
            .unwrap();

            // But manifest overrides it
            let json = r#"{
            "select": {"extensions":["searxng"],"skills":[]},
            "configs": {"searxng.json": {"baseUrl": "https://custom.example.com"}}
        }"#;
            let manifest: ProfileManifest = serde_json::from_str(json).unwrap();
            let dst = home.join("profile");
            ProfileManager::build_profile(&manifest, &dst).unwrap();

            let config_path = dst.join("config").join("searxng.json");
            assert!(config_path.exists());
            let content: serde_json::Value =
                serde_json::from_str(&read_file(&config_path)).unwrap();
            assert_eq!(content["baseUrl"], "https://custom.example.com");
        });
    }

    // ─── Tests that modify HOME (must run sequentially) ────────

    use std::sync::Mutex;
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    fn with_home<T>(home: &Path, f: impl FnOnce() -> T) -> T {
        let _guard = HOME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let old_home = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", home) };
        let result = f();
        if let Some(h) = old_home {
            unsafe { std::env::set_var("HOME", h) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }
        result
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
            fs::create_dir_all(paths::profiles_root()).unwrap();
            let result = ProfileManager::create("empty", false, None);
            assert!(result.is_ok(), "create failed: {:?}", result.err());
        });

        let mpath = home
            .join(".pim")
            .join("profiles")
            .join("empty")
            .join("manifest.json");
        assert!(mpath.exists());
        let manifest: ProfileManifest = serde_json::from_str(&read_file(&mpath)).unwrap();
        assert!(manifest.select.extensions.is_empty());
    }

    #[test]
    fn test_auto_heal_is_now_noop() {
        let (_tmp, home) = sandbox();
        with_home(&home, || {
            ProfileManager::auto_heal_symlink();
        });
    }

    #[test]
    fn test_use_profile_creates_dir_and_sets_default() {
        let (_tmp, home) = sandbox();
        let name = "test-valid";
        with_home(&home, || {
            let profile_dir = home.join(".pim").join("profiles").join(name);
            fs::create_dir_all(&profile_dir).unwrap();
            fs::write(
                profile_dir.join("manifest.json"),
                r#"{"select":{"extensions":[],"skills":[]}}"#,
            )
            .unwrap();

            ProfileManager::set_default(name).unwrap();

            // Profile directory should exist with subdirs
            assert!(profile_dir.exists());
            assert!(profile_dir.join("extensions").is_dir());
            assert!(profile_dir.join("skills").is_dir());
            assert!(profile_dir.join("config").is_dir());
            // Should be set as default
            assert_eq!(ProfileManager::get_default().as_deref(), Some(name));
        });
    }

    #[test]
    fn test_list_profile_names() {
        let (_tmp, home) = sandbox();
        with_home(&home, || {
            fs::create_dir_all(paths::profiles_root()).unwrap();
            create_manifest_json(&home, "alpha", r#"{"select":{}}"#);
            create_manifest_json(&home, "beta", r#"{"select":{}}"#);

            let names = ProfileManager::list_profile_names();
            assert_eq!(names, vec!["alpha", "beta"]);
        });
    }
}
