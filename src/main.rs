//! Desktop entry point. `rust-terminal mcp` starts the MCP server for coding agents instead.

fn main() -> eframe::Result<()> {
    if std::env::args().nth(1).is_some_and(|a| a == "check-feeds") {
        rust_terminal::check_feeds();
        return Ok(());
    }
    if std::env::args()
        .nth(1)
        .is_some_and(|a| a == "mcp" || a == "--mcp")
    {
        rust_terminal::run_mcp();
        return Ok(());
    }
    rust_terminal::run_desktop()
}
