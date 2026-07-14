use assert_cmd::Command;
use predicates::str;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Create a temporary directory and set HOME to it so pi-manager operates in
/// an isolated sandbox. Returns the TempDir (keeps it alive) and its path.
fn sandbox() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let home = tmp.path().canonicalize().unwrap_or_else(|_| tmp.path().to_path_buf());
    (tmp, home)
}

/// Build a pi-manager command with HOME set to the given directory.
fn pi_man(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("pim").expect("pi-manager binary not found");
    cmd.env("HOME", home);
    cmd
}

/// Create a minimal pi agent directory at ~/.pi/agent/ in the sandbox.
fn create_stock_agent(home: &Path) {
    let agent = home.join(".pi").join("agent");
    fs::create_dir_all(&agent).unwrap();
    // Put a settings file and some extensions to make it realistic
    fs::write(agent.join("settings.json"), r#"{"theme": "dark"}"#).unwrap();
    fs::create_dir_all(agent.join("extensions")).unwrap();
    fs::create_dir_all(agent.join("skills")).unwrap();
    fs::create_dir_all(agent.join("prompts")).unwrap();
}

/// Assert that a profile directory exists at ~/.pi-manager/profiles/<name>/.
fn assert_profile_exists(home: &Path, name: &str) {
    let profile = home.join(".pi-manager").join("profiles").join(name);
    assert!(profile.exists(), "Profile '{}' should exist at {}", name, profile.display());
    assert!(profile.join("extensions").exists(), "Profile '{}' missing extensions/", name);
    assert!(profile.join("skills").exists(), "Profile '{}' missing skills/", name);
    assert!(profile.join("prompts").exists(), "Profile '{}' missing prompts/", name);
}

/// Assert that a profile directory does NOT exist.
fn assert_profile_missing(home: &Path, name: &str) {
    let profile = home.join(".pi-manager").join("profiles").join(name);
    assert!(!profile.exists(), "Profile '{}' should not exist", name);
}

/// Assert that ~/.pi/agent is a symlink pointing to the given profile directory.
fn assert_symlink_points_to(home: &Path, profile_name: &str) {
    let agent = home.join(".pi").join("agent");
    assert!(agent.is_symlink(), "~/.pi/agent should be a symlink");
    let target = fs::read_link(&agent).expect("failed to read symlink");
    let expected = home.join(".pi-manager").join("profiles").join(profile_name);
    // Canonicalize both for comparison (symlink target may be absolute)
    let target_canon = target.canonicalize().unwrap_or(target);
    let expected_canon = expected.canonicalize().unwrap_or(expected);
    assert_eq!(
        target_canon, expected_canon,
        "~/.pi/agent should point to profile '{}'",
        profile_name
    );
}

/// Assert that ~/.pi/agent is NOT a symlink pointing to any pi-manager profile.
fn assert_not_managed(home: &Path) {
    let agent = home.join(".pi").join("agent");
    if agent.is_symlink() {
        // If it IS a symlink, ensure it does NOT point into our profiles dir
        let target = fs::read_link(&agent).unwrap();
        let profiles_root = home.join(".pi-manager").join("profiles");
        assert!(
            !target.starts_with(&profiles_root),
            "~/.pi/agent symlink should not point into pi-manager profiles"
        );
    }
    // Otherwise it's a dir or doesn't exist — both are "not managed"
}

/// Assert the default file content.
fn assert_default_is(home: &Path, name: &str) {
    let def = home.join(".pi-manager").join("default");
    assert!(def.exists(), "Default file should exist");
    let content = fs::read_to_string(&def).unwrap();
    assert_eq!(content.trim(), name);
}

/// Assert the default file does NOT exist.
fn assert_no_default(home: &Path) {
    let def = home.join(".pi-manager").join("default");
    assert!(!def.exists(), "Default file should not exist");
}

// ─── Tests ──────────────────────────────────────────────────────────────────

// ── create ──────────────────────────────────────────────────────────────────

#[test]
fn create_empty_profile() {
    let (_tmp, home) = sandbox();

    pi_man(&home)
        .arg("create")
        .arg("work")
        .assert()
        .success()
        .stdout(str::contains("Created profile 'work'"));

    assert_profile_exists(&home, "work");
}

#[test]
fn create_from_base_copies_content() {
    let (_tmp, home) = sandbox();
    create_stock_agent(&home);

    pi_man(&home)
        .args(["create", "work", "--from-base"])
        .assert()
        .success()
        .stdout(str::contains("Created profile 'work'"));

    assert_profile_exists(&home, "work");

    // The settings.json from the stock agent should have been copied
    let profile = home.join(".pi-manager").join("profiles").join("work");
    let settings = profile.join("settings.json");
    assert!(settings.exists(), "settings.json should be copied from base");
    let content = fs::read_to_string(&settings).unwrap();
    assert_eq!(content.trim(), r#"{"theme": "dark"}"#);
}

#[test]
fn create_from_another_profile() {
    let (_tmp, home) = sandbox();

    // Create source profile
    pi_man(&home).args(["create", "source"]).assert().success();
    // Add a marker file to source
    let src = home.join(".pi-manager").join("profiles").join("source");
    fs::write(src.join("marker.txt"), "hello").unwrap();

    // Create from source
    pi_man(&home)
        .args(["create", "target", "--from", "source"])
        .assert()
        .success()
        .stdout(str::contains("Created profile 'target'"));

    // Target should have the marker
    let tgt = home.join(".pi-manager").join("profiles").join("target");
    assert!(tgt.join("marker.txt").exists(), "marker.txt should be copied from source");
    assert_eq!(fs::read_to_string(tgt.join("marker.txt")).unwrap(), "hello");
}

#[test]
fn create_duplicate_fails() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "work"]).assert().success();

    pi_man(&home)
        .args(["create", "work"])
        .assert()
        .failure()
        .stderr(str::contains("already exists"));
}

#[test]
fn create_from_base_without_agent_fails() {
    let (_tmp, home) = sandbox();

    pi_man(&home)
        .args(["create", "work", "--from-base"])
        .assert()
        .failure()
        .stderr(str::contains("No pi config found"));
}

#[test]
fn create_from_nonexistent_profile_fails() {
    let (_tmp, home) = sandbox();

    pi_man(&home)
        .args(["create", "work", "--from", "nonexistent"])
        .assert()
        .failure()
        .stderr(str::contains("does not exist"));
}

// ── list ────────────────────────────────────────────────────────────────────

#[test]
fn list_empty() {
    let (_tmp, home) = sandbox();

    pi_man(&home)
        .arg("list")
        .assert()
        .success()
        .stdout(str::contains("No profiles found"));
}

#[test]
fn list_with_profiles() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "alpha"]).assert().success();
    pi_man(&home).args(["create", "beta"]).assert().success();

    pi_man(&home)
        .arg("list")
        .assert()
        .success()
        .stdout(str::contains("alpha"))
        .stdout(str::contains("beta"));
}

#[test]
fn list_shows_active_and_default_markers() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "main"]).assert().success();
    pi_man(&home).args(["create", "backup"]).assert().success();

    // Set default
    pi_man(&home).args(["set-default", "main"]).assert().success();
    // Activate
    pi_man(&home).args(["use", "main"]).assert().success();

    pi_man(&home)
        .arg("list")
        .assert()
        .success()
        .stdout(str::contains("main"))
        .stdout(str::contains("backup"))
        .stdout(str::contains("active"))
        .stdout(str::contains("default"));
}

// ── status ─────────────────────────────────────────────────────────────────

#[test]
fn status_no_profiles() {
    let (_tmp, home) = sandbox();

    pi_man(&home)
        .arg("status")
        .assert()
        .success()
        .stdout(str::contains("does not exist"))
        .stdout(str::contains("No default profile set"));
}

#[test]
fn status_with_managed_agent() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "work"]).assert().success();
    pi_man(&home).args(["use", "work"]).assert().success();

    pi_man(&home)
        .arg("status")
        .assert()
        .success()
        .stdout(str::contains("Active profile: work"))
        .stdout(str::contains("No default profile set"));
}

#[test]
fn status_with_unmanaged_directory() {
    let (_tmp, home) = sandbox();
    create_stock_agent(&home);

    pi_man(&home)
        .arg("status")
        .assert()
        .success()
        .stdout(str::contains("regular directory"))
        .stdout(str::contains("No default profile set"));
}

// ── set-default ────────────────────────────────────────────────────────────

#[test]
fn set_default_creates_default_file() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "work"]).assert().success();
    pi_man(&home).args(["set-default", "work"]).assert().success();

    assert_default_is(&home, "work");
}

#[test]
fn set_default_nonexistent_fails() {
    let (_tmp, home) = sandbox();

    pi_man(&home)
        .args(["set-default", "ghost"])
        .assert()
        .failure()
        .stderr(str::contains("does not exist"));
}

// ── use ────────────────────────────────────────────────────────────────────

#[test]
fn use_profile_creates_symlink() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "work"]).assert().success();
    pi_man(&home)
        .args(["use", "work"])
        .assert()
        .success()
        .stdout(str::contains("Activated profile 'work'"));

    assert_symlink_points_to(&home, "work");
}

#[test]
fn use_switches_between_profiles() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "work"]).assert().success();
    pi_man(&home).args(["create", "personal"]).assert().success();

    // Activate work
    pi_man(&home).args(["use", "work"]).assert().success();
    assert_symlink_points_to(&home, "work");

    // Switch to personal
    pi_man(&home).args(["use", "personal"]).assert().success();
    assert_symlink_points_to(&home, "personal");
}

#[test]
fn use_nonexistent_fails() {
    let (_tmp, home) = sandbox();

    pi_man(&home)
        .args(["use", "ghost"])
        .assert()
        .failure()
        .stderr(str::contains("does not exist"));
}

#[test]
fn use_migrates_real_directory() {
    let (_tmp, home) = sandbox();

    // Create a real ~/.pi/agent directory (as if the user ran pi normally)
    create_stock_agent(&home);

    // Create a profile
    pi_man(&home).args(["create", "work"]).assert().success();

    // Activate it — should migrate the real directory first
    pi_man(&home)
        .args(["use", "work"])
        .assert()
        .success()
        .stdout(str::contains("Activated profile 'work'"));

    // ~/.pi/agent should now be a symlink
    assert_symlink_points_to(&home, "work");

    // The original content should be backed up as a profile named "default"
    let backup = home.join(".pi-manager").join("profiles").join("default");
    assert!(backup.exists(), "Original config should be backed up as 'default' profile");
    assert!(backup.join("settings.json").exists(), "settings.json should be in backup");
}

// ── default activation (no subcommand) ─────────────────────────────────────

#[test]
fn no_args_activates_default() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "work"]).assert().success();
    pi_man(&home).args(["set-default", "work"]).assert().success();

    // No subcommand → activate default
    pi_man(&home)
        .assert()
        .success()
        .stdout(str::contains("Activated profile 'work'"));

    assert_symlink_points_to(&home, "work");
}

#[test]
fn no_args_without_default_shows_status() {
    let (_tmp, home) = sandbox();

    pi_man(&home)
        .assert()
        .success()
        .stdout(str::contains("No default profile set"))
        .stdout(str::contains("does not exist"));
}

// ── delete ──────────────────────────────────────────────────────────────────

#[test]
fn delete_force_removes_profile() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "work"]).assert().success();
    pi_man(&home)
        .args(["delete", "work", "--force"])
        .assert()
        .success()
        .stdout(str::contains("Deleted profile 'work'"));

    assert_profile_missing(&home, "work");
}

#[test]
fn delete_force_active_profile_removes_symlink() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "work"]).assert().success();
    pi_man(&home).args(["use", "work"]).assert().success();

    // Delete while active
    pi_man(&home)
        .args(["delete", "work", "--force"])
        .assert()
        .success()
        .stdout(str::contains("Removed active symlink"));

    assert_profile_missing(&home, "work");

    // ~/.pi/agent should no longer exist (symlink was removed)
    let agent = home.join(".pi").join("agent");
    assert!(!agent.exists(), "Symlink should be removed");
}

#[test]
fn delete_force_default_profile_clears_default() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "work"]).assert().success();
    pi_man(&home).args(["set-default", "work"]).assert().success();

    pi_man(&home)
        .args(["delete", "work", "--force"])
        .assert()
        .success()
        .stdout(str::contains("Cleared default"));

    assert_no_default(&home);
}

#[test]
fn delete_nonexistent_fails() {
    let (_tmp, home) = sandbox();

    pi_man(&home)
        .args(["delete", "ghost", "--force"])
        .assert()
        .failure()
        .stderr(str::contains("does not exist"));
}

// ── edge cases ──────────────────────────────────────────────────────────────

#[test]
fn create_then_list_then_use_then_delete_workflow() {
    let (_tmp, home) = sandbox();

    // 1. Create two profiles
    pi_man(&home).args(["create", "work"]).assert().success();
    pi_man(&home).args(["create", "personal"]).assert().success();

    // 2. List should show both
    pi_man(&home)
        .arg("list")
        .assert()
        .success()
        .stdout(str::contains("work"))
        .stdout(str::contains("personal"));

    // 3. Set default and activate
    pi_man(&home).args(["set-default", "work"]).assert().success();
    pi_man(&home).args(["use", "work"]).assert().success();
    assert_symlink_points_to(&home, "work");
    assert_default_is(&home, "work");

    // 4. Switch to personal
    pi_man(&home).args(["use", "personal"]).assert().success();
    assert_symlink_points_to(&home, "personal");

    // 5. Status should reflect the switch
    pi_man(&home)
        .arg("status")
        .assert()
        .success()
        .stdout(str::contains("Active profile: personal"))
        .stdout(str::contains("Default profile: work"));

    // 6. Delete active profile
    pi_man(&home)
        .args(["delete", "personal", "--force"])
        .assert()
        .success()
        .stdout(str::contains("Removed active symlink"));

    // 7. Delete default profile
    pi_man(&home)
        .args(["delete", "work", "--force"])
        .assert()
        .success()
        .stdout(str::contains("Cleared default"));

    // 8. List should show none
    pi_man(&home)
        .arg("list")
        .assert()
        .success()
        .stdout(str::contains("No profiles found"));
}

// ── inheritance / merge ────────────────────────────────────────────────────

/// Helper: read a file from the sandboxed home dir.
fn read_sandbox_file(home: &Path, rel: &str) -> String {
    fs::read_to_string(home.join(rel)).unwrap()
}

#[test]
fn create_with_inheritance_and_use_activates_merged_symlink() {
    let (_tmp, home) = sandbox();

    // Create a base profile with an mcp.json
    pi_man(&home).args(["create", "base"]).assert().success();
    fs::write(
        home.join(".pi-manager").join("profiles").join("base").join("mcp.json"),
        r#"{"mcpServers": {"playwright": {"command": "pw"}}, "settings": {"timeout": 30}}"#,
    )
    .unwrap();

    // Create a child profile inheriting from base
    pi_man(&home)
        .args(["create", "child", "--inherits", "base"])
        .assert()
        .success();
    fs::write(
        home.join(".pi-manager").join("profiles").join("child").join("mcp.json"),
        r#"{"mcpServers": {"homeassistant": {"command": "ha"}}, "settings": {"toolPrefix": "mcp"}}"#,
    )
    .unwrap();

    // Activate the child profile — triggers merge
    pi_man(&home)
        .args(["use", "child"])
        .assert()
        .success()
        .stdout(str::contains("Activated profile 'child'"))
        .stdout(str::contains("Merged inheritance chain"))
        .stdout(str::contains("child ← base"));

    // The symlink should point to the merged view
    let agent = home.join(".pi").join("agent");
    assert!(agent.is_symlink());
    let target = fs::read_link(&agent).unwrap();
    assert!(
        target.starts_with(&home.join(".pi-manager").join(".merged")),
        "symlink should point to merged view, got: {}",
        target.display()
    );

    // Verify the merged mcp.json has both servers
    let merged_mcp = read_sandbox_file(
        &home,
        ".pi-manager/.merged/child/mcp.json",
    );
    let val: serde_json::Value = serde_json::from_str(&merged_mcp).unwrap();
    assert!(
        val["mcpServers"].get("playwright").is_some(),
        "merged mcp.json should have playwright from parent"
    );
    assert!(
        val["mcpServers"].get("homeassistant").is_some(),
        "merged mcp.json should have homeassistant from child"
    );
    // Settings merged: timeout from parent, toolPrefix from child
    assert_eq!(val["settings"]["timeout"], 30);
    assert_eq!(val["settings"]["toolPrefix"], "mcp");
}

#[test]
fn use_profile_without_inheritance_does_not_create_merged_view() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "simple"]).assert().success();

    pi_man(&home)
        .args(["use", "simple"])
        .assert()
        .success()
        .stdout(str::contains("Activated profile 'simple'"));

    // Symlink should point directly to profile dir, NOT merged view
    let agent = home.join(".pi").join("agent");
    assert!(agent.is_symlink());
    let target = fs::read_link(&agent).unwrap();
    let expected = home.join(".pi-manager").join("profiles").join("simple");
    assert_eq!(target, expected, "should point directly to profile");

    // No merged view should exist
    let merged = home.join(".pi-manager").join(".merged").join("simple");
    assert!(!merged.exists(), "no merged view for non-inheriting profile");
}

#[test]
fn delete_inheriting_profile_cleans_merged_view() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "base"]).assert().success();
    pi_man(&home)
        .args(["create", "child", "--inherits", "base"])
        .assert()
        .success();

    // Activate to trigger merge build
    pi_man(&home).args(["use", "child"]).assert().success();

    let merged = home.join(".pi-manager").join(".merged").join("child");
    assert!(merged.exists(), "merged view should exist");

    // Delete (force) — should clean up merged view
    pi_man(&home)
        .args(["delete", "child", "--force"])
        .assert()
        .success();

    assert!(!merged.exists(), "merged view should be cleaned up on delete");
}

#[test]
fn status_shows_merged_profiles_as_active() {
    let (_tmp, home) = sandbox();

    pi_man(&home).args(["create", "base"]).assert().success();
    pi_man(&home)
        .args(["create", "child", "--inherits", "base"])
        .assert()
        .success();

    pi_man(&home).args(["use", "child"]).assert().success();

    pi_man(&home)
        .arg("status")
        .assert()
        .success()
        .stdout(str::contains("Active profile: child"));
}
