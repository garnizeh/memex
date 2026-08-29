use crate::errors::MemexError;

/// Executes the `install` command.
pub fn run_install(target: Option<&str>, yes: bool) -> Result<(), MemexError> {
    eprintln!(
        "Running install (target: {:?}, non-interactive: {})",
        target, yes
    );
    // TODO: implement in Phase 8
    Ok(())
}
