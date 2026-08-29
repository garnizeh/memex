pub mod gitignore;
pub mod walker;

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
        if let Ok(canon_home) = home.canonicalize() {
            if resolved == canon_home {
                return Some("your home directory".to_string());
            }
            if canon_home.starts_with(&resolved) && canon_home != resolved {
                return Some("a parent of your home directory".to_string());
            }
        }
    }

    None
}
