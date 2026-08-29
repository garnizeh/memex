pub mod gitignore;
pub mod walker;

pub use gitignore::PathFilter;
pub use walker::{is_markdown_file, FileDiscovery};

use std::path::Path;

/// Validates whether a directory is safe to use as a project root for indexing.
/// Refuses home directory or filesystem roots unless forced.
pub fn unsafe_index_root_reason(path: &Path) -> Option<String> {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    // Check filesystem root
    if resolved.parent().is_none() {
        return Some("the filesystem root".to_string());
    }

    // Check user home directory
    if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) {
        let canon_home = home.canonicalize().unwrap_or(home);
        if resolved == canon_home {
            return Some("your home directory".to_string());
        }
        if canon_home.starts_with(&resolved) && canon_home != resolved {
            return Some("a parent of your home directory".to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_filesystem_root_rejected() {
        #[cfg(unix)]
        {
            let root = Path::new("/");
            let reason = unsafe_index_root_reason(root);
            assert!(
                reason == Some("the filesystem root".to_string())
                    || reason == Some("a parent of your home directory".to_string())
            );
        }
        #[cfg(windows)]
        {
            let root = Path::new(r"C:\");
            let reason = unsafe_index_root_reason(root);
            assert!(
                reason == Some("the filesystem root".to_string())
                    || reason == Some("a parent of your home directory".to_string())
            );
        }
    }

    #[test]
    fn test_home_dir_rejected() {
        if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) {
            let reason = unsafe_index_root_reason(&home);
            assert_eq!(reason, Some("your home directory".to_string()));
        }
    }

    #[test]
    fn test_parent_of_home_dir_rejected() {
        if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) {
            if let Some(parent) = home.parent() {
                let reason = unsafe_index_root_reason(parent);
                assert!(reason.is_some());
                let r = reason.unwrap();
                assert!(
                    r == "the filesystem root" || r == "a parent of your home directory",
                    "Unexpected reason: {}",
                    r
                );
            }
        }
    }

    #[test]
    fn test_safe_subdirectory_accepted() {
        let temp_dir = TempDir::new().unwrap();
        let sub_dir = temp_dir.path().join("safe_project");
        fs::create_dir(&sub_dir).unwrap();

        let reason = unsafe_index_root_reason(&sub_dir);
        assert_eq!(reason, None);
    }

    #[test]
    #[cfg(unix)]
    fn test_symlink_to_home_rejected() {
        if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) {
            let temp_dir = TempDir::new().unwrap();
            let link = temp_dir.path().join("home_link");
            std::os::unix::fs::symlink(&home, &link).unwrap();

            let reason = unsafe_index_root_reason(&link);
            assert_eq!(reason, Some("your home directory".to_string()));
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_symlink_to_safe_dir_accepted() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("target_dir");
        fs::create_dir(&target).unwrap();

        let link = temp_dir.path().join("safe_link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let reason = unsafe_index_root_reason(&link);
        assert_eq!(reason, None);
    }
}
