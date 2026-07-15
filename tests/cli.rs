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
        fs::create_dir_all(home.join(".pi-manager").join("profiles")).unwrap();
        fs::create_dir_all(home.join(".pi-manager").join("pool").join("extensions")).unwrap();
        fs::create_dir_all(home.join(".pi-manager").join("pool").join("skills")).unwrap();
        fs::create_dir_all(home.join(".pi-manager").join("pool").join("prompts")).unwrap();
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

    /// Create a profile manifest at the given path with the given JSON content.
    fn create_profile_manifest(&self, name: &str, json: &str) {
        let path = self
            .home
            .join(".pi-manager")
            .join("profiles")
            .join(format!("{name}.json"));
        fs::write(&path, json).unwrap();
    }

    fn agent_dir(&self) -> PathBuf {
        self.home.join(".pi").join("agent")
    }

    /// Assert ~/.pi/agent is a symlink pointing to the active view of `name`.
    fn assert_active_profile(&self, name: &str) {
        let agent = self.agent_dir();
        assert!(agent.is_symlink(), "~/.pi/agent should be a symlink");
        let target = fs::read_link(&agent).unwrap();
        let expected = self.home.join(".pi-manager").join(".active").join(name);
        assert_eq!(
            target,
            expected,
            "~/.pi/agent should point to active view of '{}'\n  got:      {}\n  expected: {}",
            name,
            target.display(),
            expected.display()
        );
    }
}

// ─── create ────────────────────────────────────────────────────

#[test]
fn test_create_empty_profile() {
    let s = Sandbox::new();
    s.pim().arg("create").arg("work").assert().success();

    let manifest = s
        .home()
        .join(".pi-manager")
        .join("profiles")
        .join("work.json");
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
    s.create_profile_manifest("base", r#"{"select":{"extensions":["rtk"]}}"#);
    s.pim()
        .arg("create")
        .arg("work")
        .arg("--from")
        .arg("base")
        .assert()
        .success();

    let manifest_path = s
        .home()
        .join(".pi-manager")
        .join("profiles")
        .join("work.json");
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
fn test_use_profile_creates_symlink() {
    let s = Sandbox::new();
    s.create_profile_manifest("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.pim().arg("use").arg("work").assert().success();
    s.assert_active_profile("work");
}

#[test]
fn test_use_switches_between_profiles() {
    let s = Sandbox::new();
    s.create_profile_manifest("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.create_profile_manifest("personal", r#"{"select":{"extensions":[],"skills":[]}}"#);

    s.pim().arg("use").arg("work").assert().success();
    s.assert_active_profile("work");

    s.pim().arg("use").arg("personal").assert().success();
    s.assert_active_profile("personal");
}

#[test]
fn test_use_nonexistent_fails() {
    let s = Sandbox::new();
    s.pim().arg("use").arg("nonexistent").assert().failure();
}

#[test]
fn test_use_with_selections() {
    let s = Sandbox::new();
    // Add an extension to the pool
    fs::write(
        s.home()
            .join(".pi-manager")
            .join("pool")
            .join("extensions")
            .join("rtk.ts"),
        "// extension",
    )
    .unwrap();

    s.create_profile_manifest(
        "work",
        r#"{"select":{"extensions":["rtk.ts"],"skills":[]}}"#,
    );
    s.pim().arg("use").arg("work").assert().success();
    s.assert_active_profile("work");

    // Check the extension was symlinked into the active view
    let active_ext = s
        .home()
        .join(".pi-manager")
        .join(".active")
        .join("work")
        .join("extensions")
        .join("rtk.ts");
    assert!(active_ext.exists(), "extension should exist in active view");
    assert!(active_ext.is_symlink(), "extension should be a symlink");
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
    s.create_profile_manifest("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.pim().arg("use").arg("work").assert().success();
    s.pim().arg("status").assert().success();
}

// ─── set-default ───────────────────────────────────────────────

#[test]
fn test_set_default_creates_default_file() {
    let s = Sandbox::new();
    s.create_profile_manifest("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.pim().arg("set-default").arg("work").assert().success();

    let default_path = s.home().join(".pi-manager").join("default");
    assert!(default_path.exists());
    let content = fs::read_to_string(&default_path).unwrap();
    assert_eq!(content.trim(), "work");
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
fn test_no_args_activates_default() {
    let s = Sandbox::new();
    s.create_profile_manifest("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.pim().arg("set-default").arg("work").assert().success();
    s.pim().assert().success();
    s.assert_active_profile("work");
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

    let manifest = s
        .home()
        .join(".pi-manager")
        .join("profiles")
        .join("work.json");
    assert!(!manifest.exists(), "manifest should be deleted");
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
fn test_delete_force_active_profile_removes_symlink() {
    let s = Sandbox::new();
    s.create_profile_manifest("work", r#"{"select":{"extensions":[],"skills":[]}}"#);
    s.pim().arg("use").arg("work").assert().success();
    s.assert_active_profile("work");

    s.pim()
        .arg("delete")
        .arg("work")
        .arg("--force")
        .assert()
        .success();
    assert!(!s.agent_dir().exists(), "symlink should be removed");
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

    let default_path = s.home().join(".pi-manager").join("default");
    assert!(!default_path.exists(), "default file should be removed");
}

// ─── migration ─────────────────────────────────────────────────

#[test]
fn test_migrate_converts_old_profile() {
    let s = Sandbox::new();

    // Create an old-style profile directory
    let old_dir = s.home().join(".pi-manager").join("profiles").join("legacy");
    fs::create_dir_all(old_dir.join("extensions")).unwrap();
    fs::write(old_dir.join("extensions").join("rtk.ts"), "// ext").unwrap();
    fs::write(old_dir.join("settings.json"), r#"{"theme":"dark"}"#).unwrap();
    fs::write(old_dir.join("auth.json"), r#"{"key":"secret"}"#).unwrap();

    s.pim().arg("migrate").assert().success();

    // Check manifest was created
    let manifest = s
        .home()
        .join(".pi-manager")
        .join("profiles")
        .join("legacy.json");
    assert!(manifest.exists(), "manifest should exist after migration");

    // Check pool has the extension
    assert!(
        s.home()
            .join(".pi-manager")
            .join("pool")
            .join("extensions")
            .join("rtk.ts")
            .exists(),
        "extension should be in pool"
    );

    // Check data has auth
    assert!(
        s.home()
            .join(".pi-manager")
            .join("data")
            .join("legacy")
            .join("auth.json")
            .exists(),
        "auth should be in data dir"
    );

    // Check old directory is gone
    assert!(
        !s.home()
            .join(".pi-manager")
            .join("profiles")
            .join("legacy")
            .exists(),
        "old profile directory should be removed"
    );
}

#[test]
fn test_use_migrates_real_directory() {
    let s = Sandbox::new();

    // Create old-style profile
    let old_dir = s.home().join(".pi-manager").join("profiles").join("legacy");
    fs::create_dir_all(old_dir.join("extensions")).unwrap();
    fs::write(old_dir.join("extensions").join("rtk.ts"), "// ext").unwrap();

    // Use it — should migrate on-the-fly
    s.pim().arg("use").arg("legacy").assert().success();

    // Should now be an active view symlink
    s.assert_active_profile("legacy");

    // Old directory should be gone
    assert!(
        !s.home()
            .join(".pi-manager")
            .join("profiles")
            .join("legacy")
            .exists(),
        "old profile directory should be removed after use"
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
    s.pim().arg("use").arg("work").assert().success();
    s.assert_active_profile("work");

    // Delete it
    s.pim()
        .arg("delete")
        .arg("work")
        .arg("--force")
        .assert()
        .success();
    assert!(
        !s.home()
            .join(".pi-manager")
            .join("profiles")
            .join("work.json")
            .exists()
    );
}

#[test]
fn test_migrate_active_profile_updates_symlink() {
    let s = Sandbox::new();

    // Create an old-style profile directory
    let old_dir = s.home().join(".pi-manager").join("profiles").join("legacy");
    fs::create_dir_all(old_dir.join("extensions")).unwrap();
    fs::write(old_dir.join("extensions").join("rtk.ts"), "// ext").unwrap();

    // Point the symlink ~/.pi/agent to the old-style profile directory
    let agent = s.agent_dir();
    fs::create_dir_all(agent.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&old_dir, &agent).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&old_dir, &agent).unwrap();

    // Run migrate
    s.pim().arg("migrate").assert().success();

    // Verify it is now migrated AND the symlink is updated to the active view
    s.assert_active_profile("legacy");
}
