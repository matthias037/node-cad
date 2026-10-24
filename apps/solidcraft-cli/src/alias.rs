//! The CLI's former names (`solidcraft-cli`, `solvecraft-cli`), kept so existing scripts and MCP
//! client setups (`claude mcp add solidcraft -- solidcraft-cli mcp`) keep working. Compatibility
//! shim (docs/compatibility.md): each runs `node-cad-cli` (next to this executable, else on PATH)
//! with the same arguments and standard streams, after a one-line deprecation note on stderr
//! (stdout stays clean for MCP's JSON-RPC), and exits with its status.

use std::process::{Command, ExitCode};

const TARGET: &str = "node-cad-cli";

/// Run `node-cad-cli` in place of this alias (named by its binary target).
pub fn forward() -> ExitCode {
    let name = env!("CARGO_BIN_NAME");
    eprintln!("{name} is deprecated and will be removed in a later release: use {TARGET}");
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(format!("{TARGET}{}", std::env::consts::EXE_SUFFIX))))
        .filter(|p| p.is_file());
    let program = sibling.map(|p| p.into_os_string()).unwrap_or_else(|| TARGET.into());
    match Command::new(&program).args(std::env::args_os().skip(1)).status() {
        Ok(status) => match status.code() {
            Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
            None => ExitCode::FAILURE,
        },
        Err(e) => {
            eprintln!("{name}: could not run {}: {e}", program.to_string_lossy());
            ExitCode::FAILURE
        }
    }
}
