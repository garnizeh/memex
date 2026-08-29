use crate::errors::MemexError;
use std::path::Path;

/// Executes the `index` command.
pub fn run_index(path: &Path, quiet: bool, verbose: bool) -> Result<(), MemexError> {
    if !quiet {
        eprintln!("Running index at {:?} (verbose: {})", path, verbose);
    }
    // TODO: implement in Phase 6
    Ok(())
}
