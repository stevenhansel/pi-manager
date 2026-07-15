//! Extension manifest schema (`extension.json`).
//!
//! Each item in the pool directory (`pool/extensions/<name>/`) can carry an
//! `extension.json` that declares its config fields, dependencies, and
//! metadata. The TUI uses this to display status and prompt for inputs.

use serde::{Deserialize, Serialize};

/// The manifest inside a pool extension directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifest {
    /// Display name (defaults to directory name).
    #[serde(default)]
    pub name: Option<String>,

    /// One-line description of what this extension does.
    #[serde(default)]
    pub description: Option<String>,

    /// Config fields the user should fill in.
    #[serde(default)]
    pub config_fields: Vec<ConfigField>,

    /// Checks to run (bin in PATH, URL reachability, etc.).
    #[serde(default)]
    pub checks: Vec<ExtensionCheck>,

    /// Tags for categorization.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// One configurable field — pim will prompt for this via the TUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigField {
    /// The JSON key written to `<ext>.json` (e.g. `"searxngBaseUrl"`).
    pub key: String,

    /// Human-readable label shown in prompts.
    #[serde(default)]
    pub label: Option<String>,

    /// Field type for input rendering.
    #[serde(default)]
    pub r#type: FieldType,

    /// Default value if the user leaves it blank.
    #[serde(default)]
    pub default: Option<String>,

    /// Whether this field is required for the extension to work.
    #[serde(default)]
    pub required: bool,

    /// Help text shown in the detail panel.
    #[serde(default)]
    pub help: Option<String>,

    /// If set, pim shows the corresponding env var name.
    #[serde(default)]
    pub env_var: Option<String>,

    /// How this field interacts with its env var.
    /// "fallback" — config file wins, env var is fallback (default).
    /// "prefer"  — env var wins over config file.
    /// "override" — env var always overrides at runtime (for secrets).
    #[serde(default)]
    pub env_priority: EnvPriority,
}

/// Input widget type for a config field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum FieldType {
    /// Plain text (default).
    #[default]
    #[serde(rename = "string")]
    String,
    /// Masked input (e.g. API keys).
    #[serde(rename = "password")]
    Password,
    /// Numeric input.
    #[serde(rename = "number")]
    Number,
    /// Boolean toggle.
    #[serde(rename = "boolean")]
    Boolean,
    /// URL (validated before save).
    #[serde(rename = "url")]
    Url,
}

/// How the config field value relates to its corresponding env var.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum EnvPriority {
    /// Config file wins, env var is fallback.
    #[default]
    #[serde(rename = "fallback")]
    Fallback,
    /// Env var wins over config file.
    #[serde(rename = "prefer")]
    Prefer,
    /// Env var always overrides at runtime.
    #[serde(rename = "override")]
    Override,
}

/// A pre-flight check the extension needs to pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExtensionCheck {
    /// A binary must be in PATH (optionally with version constraint).
    #[serde(rename = "bin")]
    Bin {
        /// Command name (e.g. `"rtk"`).
        value: String,
        /// Semver constraint (e.g. `">= 0.23.0"`).
        #[serde(default)]
        version: Option<String>,
    },
    /// A URL must be reachable.
    #[serde(rename = "url")]
    Url {
        /// URL to check (supports `{fieldKey}` substitution).
        value: String,
        /// Human-readable description.
        #[serde(default)]
        description: Option<String>,
    },
}

// ─── Loading ────────────────────────────────────────────────────────────

impl ExtensionManifest {
    /// Load `extension.json` from a pool item directory.
    pub fn load(dir: &std::path::Path) -> Option<Self> {
        let path = dir.join("extension.json");
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Get the display name for an item.
    #[allow(dead_code)]
    pub fn display_name<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.name.as_deref().unwrap_or(fallback)
    }

    /// Count required fields that have no default.
    pub fn required_field_count(&self) -> usize {
        self.config_fields
            .iter()
            .filter(|f| f.required && f.default.is_none())
            .count()
    }
}

// ─── Field Resolution ───────────────────────────────────────────────────

/// The resolved value for a config field after merging defaults, config file,
/// and env var according to the field's env priority.
#[allow(dead_code)]
#[allow(clippy::match_same_arms)]
pub fn resolve_field(
    field: &ConfigField,
    config_value: Option<&str>,
    env_value: Option<&str>,
) -> Option<String> {
    match (field.env_priority, config_value, env_value) {
        // Override or Prefer: env wins over config
        (EnvPriority::Override | EnvPriority::Prefer, _, Some(v)) => Some(v.to_string()),
        (EnvPriority::Override | EnvPriority::Prefer, v, None) => v.map(String::from),

        // Fallback: config wins over env, but env wins over default
        (EnvPriority::Fallback, Some(v), _) => Some(v.to_string()),
        (EnvPriority::Fallback, None, Some(v)) => Some(v.to_string()),
        (EnvPriority::Fallback, None, None) => field.default.clone(),
    }
}

// ─── Status ─────────────────────────────────────────────────────────────

/// The configuration state of a selected extension.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigStatus {
    /// Ready: all required fields have values.
    Ready,
    /// Some optional fields are missing, but the extension should work.
    Partial,
    /// Required fields are missing — the extension may not work.
    Missing,
}

/// Evaluate the status of an extension based on its manifest and current config.
pub fn evaluate_status(
    manifest: &ExtensionManifest,
    config: Option<&serde_json::Value>,
) -> ConfigStatus {
    let config = match config {
        Some(c) if c.is_object() => c.as_object().unwrap(),
        _ => {
            if manifest.required_field_count() > 0 {
                return ConfigStatus::Missing;
            }
            return ConfigStatus::Ready;
        }
    };

    let mut missing_required = 0;
    let mut missing_optional = 0;

    for field in &manifest.config_fields {
        let has_value = config
            .get(&field.key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());

        if !has_value && field.default.is_none() {
            if field.required {
                missing_required += 1;
            } else {
                missing_optional += 1;
            }
        }
    }

    if missing_required > 0 {
        ConfigStatus::Missing
    } else if missing_optional > 0 {
        ConfigStatus::Partial
    } else {
        ConfigStatus::Ready
    }
}

/// A display-friendly label for the status.
pub fn status_label(status: &ConfigStatus) -> &'static str {
    match status {
        ConfigStatus::Ready => "✓ Ready",
        ConfigStatus::Partial => "△ Partial",
        ConfigStatus::Missing => "✗ Missing",
    }
}

/// A status icon (single character).
#[allow(dead_code)]
pub fn status_icon(status: &ConfigStatus) -> &'static str {
    match status {
        ConfigStatus::Ready => "✓",
        ConfigStatus::Partial => "△",
        ConfigStatus::Missing => "✗",
    }
}
