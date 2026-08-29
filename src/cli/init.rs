use crate::errors::MemexError;
use std::path::Path;

/// Executes the `init` command.
pub fn run_init(path: &Path, force: bool, verbose: bool) -> Result<(), MemexError> {
    eprintln!(
        "Running init at {:?} (force: {}, verbose: {})",
        path, force, verbose
    );
    // TODO: implement in Phase 6
    Ok(())
}
