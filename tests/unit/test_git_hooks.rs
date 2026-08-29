#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn test_install_git_hooks_in_new_git_repo() {
    let tmp = TempDir::new().unwrap();
    let repo_dir = tmp.path();

    // Initialize a git repo
    let init_status = Command::new("git")
        .args(["init"])
        .current_dir(repo_dir)
        .status()
        .expect("git init should succeed");
    assert!(init_status.success());

    // Path to install-git-hooks.sh
    let script_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("install-git-hooks.sh");
    assert!(script_path.exists(), "install script must exist");

    // Run the install script inside the repo
    let install_status = Command::new("bash")
        .arg(&script_path)
        .current_dir(repo_dir)
        .status()
        .expect("script execution should succeed");
    assert!(install_status.success());

    // Verify hooks exist
    let hooks_dir = repo_dir.join(".git").join("hooks");
    for hook_name in ["post-commit", "post-merge", "post-checkout"] {
        let hook_file = hooks_dir.join(hook_name);
        assert!(hook_file.exists(), "Hook {} must exist", hook_name);

        let content = fs::read_to_string(&hook_file).unwrap();
        assert!(
            content.contains("memex index --quiet"),
            "Hook {} must contain memex index invocation",
            hook_name
        );

        let perms = fs::metadata(&hook_file).unwrap().permissions();
        assert_ne!(
            perms.mode() & 0o111,
            0,
            "Hook {} must be executable",
            hook_name
        );
    }

    // Run again to verify idempotency
    let second_run = Command::new("bash")
        .arg(&script_path)
        .current_dir(repo_dir)
        .output()
        .expect("second execution should succeed");
    assert!(second_run.status.success());
    let stdout = String::from_utf8_lossy(&second_run.stdout);
    assert!(stdout.contains("Memex hook already present"));
}

#[test]
fn test_install_git_hooks_wraps_non_shell_and_exiting_hooks() {
    let tmp = TempDir::new().unwrap();
    let repo_dir = tmp.path();

    let init_status = Command::new("git")
        .args(["init"])
        .current_dir(repo_dir)
        .status()
        .unwrap();
    assert!(init_status.success());

    let hooks_dir = repo_dir.join(".git").join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();

    // Create a mock Python/custom hook that exits with 0
    let existing_hook = hooks_dir.join("post-commit");
    fs::write(
        &existing_hook,
        "#!/usr/bin/env python3\nimport sys\nprint('python hook executed')\nsys.exit(0)\n",
    )
    .unwrap();

    let mut perms = fs::metadata(&existing_hook).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&existing_hook, perms).unwrap();

    let script_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("install-git-hooks.sh");

    let status = Command::new("bash")
        .arg(&script_path)
        .current_dir(repo_dir)
        .status()
        .unwrap();
    assert!(status.success());

    let legacy_backup = hooks_dir.join("post-commit.pre-memex");
    assert!(legacy_backup.exists(), "Legacy hook backup must exist");

    let wrapped_content = fs::read_to_string(&existing_hook).unwrap();
    assert!(wrapped_content.contains("post-commit.pre-memex"));
    assert!(wrapped_content.contains("memex index --quiet"));
}

#[test]
fn test_install_git_hooks_respects_custom_core_hooks_path() {
    let tmp = TempDir::new().unwrap();
    let repo_dir = tmp.path();

    Command::new("git")
        .args(["init"])
        .current_dir(repo_dir)
        .status()
        .unwrap();

    let custom_hooks = repo_dir.join(".custom_hooks");
    fs::create_dir_all(&custom_hooks).unwrap();

    Command::new("git")
        .args(["config", "core.hooksPath", ".custom_hooks"])
        .current_dir(repo_dir)
        .status()
        .unwrap();

    let script_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("install-git-hooks.sh");

    let status = Command::new("bash")
        .arg(&script_path)
        .current_dir(repo_dir)
        .status()
        .unwrap();
    assert!(status.success());

    assert!(custom_hooks.join("post-commit").exists());
    assert!(custom_hooks.join("post-merge").exists());
    assert!(custom_hooks.join("post-checkout").exists());
}
