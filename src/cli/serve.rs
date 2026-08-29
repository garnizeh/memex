use crate::errors::MemexError;

/// Executes the `serve` command in MCP stdio mode.
pub async fn run_serve(mcp: bool) -> Result<(), MemexError> {
    if mcp {
        eprintln!("Starting Memex MCP server on stdio...");
        // TODO: implement in Phase 7
    }
    Ok(())
}
