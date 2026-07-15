use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Helper: create a sandbox with pim installed.
/// Sets HOME so pim resolves paths inside the temp dir.
struct Sandbox {
    _tmp: TempDir,
    home: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let tmp = TempDir::new().expect("tempdir");
        let home = tmp
            .path()
            .canonicalize()
            .unwrap_or_else(|_| tmp.path().to_path_buf());
        // Create basic structure
        fs::create_dir_all(home.join(".pim").join("profiles")).unwrap();
        fs::create_dir_all(home.join(".pim").join("pool").join("extensions")).unwrap();
        fs::create_dir_all(home.join(".pim").join("pool").join("skills")).unwrap();
        fs::create_dir_all(home.join(".pim").join("pool").join("prompts")).unwrap();
        Self { _tmp: tmp, home }
    }

    fn pim(&self) -> Command {
        let mut cmd = Command::cargo_bin("pim").unwrap();
        cmd.env("HOME", &self.home);
        cmd
    }

    fn home(&self) -> &Path {
        &self.home
    }

    /// Create a profile in the new directory-based format.
    fn create_profile(&self, name: &str, json: &str) {
        let dir = self.home.join(".pim").join("profiles").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.json"), json).unwrap();
    }

    fn agent_dir(&self) -> PathBuf {
        self.home.join(".pi").join("agent")
    }

    /// Assert the profile directory exists and is set as default.
    fn assert_active_profile(&self, name: &str) {
        // Profile directory should exist
        let profile_dir = self.home.join(".pim").join("profiles").join(name);
        assert!(
            profile_dir.exists(),
            "Profile '{}' should exist at {}",
            name,
            profile_dir.display()
        );
        assert!(
            profile_dir.join("manifest.json").exists(),
            "Profile '{name}' should have manifest.json"
        );
        // pim.json should have this profile as default
        let pim_config = self.home.join(".pim").join("pim.json");
        assert!(pim_config.exists(), "pim.json should exist");
        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&pim_config).unwrap()).unwrap();
        assert_eq!(
            content["defaultProfile"], name,
            "Default should be set to '{name}'"
        );
        // ~/.pi/agent should NOT be touched by pim
        let agent = self.agent_dir();
        if agent.is_symlink() {
            let target = fs::read_link(&agent).unwrap();
            assert_ne!(
                target, profile_dir,
                "~/.pi/agent should NOT point to the profile directory"
            );
        }
    }
}

// ─── create ────────────────────────────────────────────────────

#[test]
fn test_create_empty_profile() {
    let s = Sandbox::new();
    s.pim().arg("create").arg("work").assert().success();

    let manifest = s
        .home()
        .join(".pim")
        .join("profiles")
        .join("work")
        .join("manifest.json");
    assert!(
        manifest.exists(),
        "Profile 'work' should exist at {}",
        manifest.display()
    );
}

#[test]
fn test_create_duplicate_fails() {
    let s = Sandbox::new();
    s.pim().arg("create").arg("work").assert().success();
    s.pim().arg("create").arg("work").assert().failure();
}

#[test]
fn test_create_from_another_profile() {
    let s = Sandbox::new();
    s.create_profile("base", r#"{"select":{"extensions":["rtk"]}}"#);
    s.pim()
        .arg("create")
        .arg("work")
        .arg("--from")
        .arg("base")
        .assert()
        .success();

    let manifest_path = s
        .home()
        .join(".pim")
        .join("profiles")
        .join("work")
        .join("manifest.json");
    let content = fs::read_to_string(&manifest_path).unwrap();
    assert!(
        content.contains("rtk"),
        "manifest should contain 'rtk', got: {content}"
    );
}

#[test]
fn test_create_from_nonexistent_profile_fails() {
    let s = Sandbox::new();
    s.pim()
        .arg("create")
        .arg("work")
        .arg("--from")
        .arg("nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to read profile"));
}

// ─── list ──────────────────────────────────────────────────────

#[test]
fn test_list_empty() {
    let s = Sandbox::new();
    s.pim().arg("list").assert().success();
}

#[test]
fn test_list_with_profiles() {
    let s = Sandbox::new();
    s.pim().arg("create").arg("work").assert().success();
    s.pim().arg("create").arg("personal").assert().success();

    let output = s
        .pim()
        .arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    assert!(stdout.contains("work"));
    assert!(stdout.contains("personal"));
}

// ─── use ───────────────────────────────────────────────────────

#[test]
fn test_set_default_creates_profile_dir() {
    let s = Sandbox::new();
    s.create_profile("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.pim().arg("set-default").arg("work").assert().success();
    s.assert_active_profile("work");
}

#[test]
fn test_set_default_switches_between_profiles() {
    let s = Sandbox::new();
    s.create_profile("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.create_profile("personal", r#"{"select":{"extensions":[],"skills":[]}}"#);

    s.pim().arg("set-default").arg("work").assert().success();
    s.assert_active_profile("work");

    s.pim()
        .arg("set-default")
        .arg("personal")
        .assert()
        .success();
    s.assert_active_profile("personal");
}

#[test]
fn test_set_default_with_selections() {
    let s = Sandbox::new();
    // Add an extension to the pool
    fs::write(
        s.home()
            .join(".pim")
            .join("pool")
            .join("extensions")
            .join("rtk.ts"),
        "// extension",
    )
    .unwrap();

    s.create_profile(
        "work",
        r#"{"select":{"extensions":["rtk.ts"],"skills":[]}}"#,
    );
    s.pim().arg("set-default").arg("work").assert().success();
    s.assert_active_profile("work");

    // Check the extension was symlinked into the profile directory
    let ext = s
        .home()
        .join(".pim")
        .join("profiles")
        .join("work")
        .join("extensions")
        .join("rtk.ts");
    assert!(ext.exists(), "extension should exist in profile directory");
    assert!(ext.is_symlink(), "extension should be a symlink");
}

// ─── status ────────────────────────────────────────────────────

#[test]
fn test_status_no_profiles() {
    let s = Sandbox::new();
    s.pim().arg("status").assert().success();
}

#[test]
fn test_status_with_managed_agent() {
    let s = Sandbox::new();
    s.create_profile("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.pim().arg("set-default").arg("work").assert().success();
    s.pim().arg("status").assert().success();
}

// ─── set-default ───────────────────────────────────────────────

#[test]
fn test_set_default_creates_default_file() {
    let s = Sandbox::new();
    s.create_profile("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.pim().arg("set-default").arg("work").assert().success();

    let pim_config = s.home().join(".pim").join("pim.json");
    assert!(pim_config.exists());
    let content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&pim_config).unwrap()).unwrap();
    assert_eq!(content["defaultProfile"], "work");
}

#[test]
fn test_set_default_nonexistent_fails() {
    let s = Sandbox::new();
    s.pim()
        .arg("set-default")
        .arg("nonexistent")
        .assert()
        .failure();
}

#[test]
fn test_no_args_shows_status_when_no_default() {
    // `pim` with no args and no default → shows status
    let s = Sandbox::new();
    s.pim().assert().success();
}

// ─── delete ────────────────────────────────────────────────────

#[test]
fn test_delete_force_removes_profile() {
    let s = Sandbox::new();
    s.pim().arg("create").arg("work").assert().success();
    s.pim()
        .arg("delete")
        .arg("work")
        .arg("--force")
        .assert()
        .success();

    let profile_dir = s.home().join(".pim").join("profiles").join("work");
    assert!(!profile_dir.exists(), "profile directory should be deleted");
}

#[test]
fn test_delete_nonexistent_fails() {
    let s = Sandbox::new();
    s.pim()
        .arg("delete")
        .arg("nonexistent")
        .arg("--force")
        .assert()
        .failure();
}

#[test]
fn test_delete_force_active_profile_removes_profile_dir() {
    let s = Sandbox::new();
    s.create_profile("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.pim().arg("set-default").arg("work").assert().success();
    s.assert_active_profile("work");

    s.pim()
        .arg("delete")
        .arg("work")
        .arg("--force")
        .assert()
        .success();

    assert!(
        !s.home().join(".pim").join("profiles").join("work").exists(),
        "profile directory should be removed"
    );
}

#[test]
fn test_delete_force_default_profile_clears_default() {
    let s = Sandbox::new();
    s.pim().arg("create").arg("work").assert().success();
    s.pim().arg("set-default").arg("work").assert().success();
    s.pim()
        .arg("delete")
        .arg("work")
        .arg("--force")
        .assert()
        .success();

    let pim_config = s.home().join(".pim").join("pim.json");
    assert!(pim_config.exists(), "pim.json should still exist");
    let content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&pim_config).unwrap()).unwrap();
    assert!(
        content.get("defaultProfile").is_none()
            || content["defaultProfile"].is_null()
            || content["defaultProfile"] == "",
        "defaultProfile should be cleared, got: {:?}",
        content["defaultProfile"]
    );
}

#[test]
fn test_create_then_list_then_use_then_delete_workflow() {
    let s = Sandbox::new();

    // Create
    s.pim().arg("create").arg("work").assert().success();

    // List should show it
    let list_out = s
        .pim()
        .arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_stdout = String::from_utf8(list_out).unwrap();
    assert!(list_stdout.contains("work"));

    // Use it
    s.pim().arg("set-default").arg("work").assert().success();
    s.assert_active_profile("work");

    // Delete it
    s.pim()
        .arg("delete")
        .arg("work")
        .arg("--force")
        .assert()
        .success();
    assert!(!s.home().join(".pim").join("profiles").join("work").exists());
}
