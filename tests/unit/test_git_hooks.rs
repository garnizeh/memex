use std::fs;
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

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::metadata(&hook_file).unwrap().permissions();
            assert_ne!(
                perms.mode() & 0o111,
                0,
                "Hook {} must be executable",
                hook_name
            );
        }
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
fn test_install_git_hooks_appends_to_existing_hook() {
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

    let existing_hook = hooks_dir.join("post-commit");
    fs::write(&existing_hook, "#!/bin/sh\necho 'existing hook'\n").unwrap();

    let script_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("install-git-hooks.sh");

    let status = Command::new("bash")
        .arg(&script_path)
        .current_dir(repo_dir)
        .status()
        .unwrap();
    assert!(status.success());

    let content = fs::read_to_string(&existing_hook).unwrap();
    assert!(content.contains("echo 'existing hook'"));
    assert!(content.contains("memex index --quiet"));
}
