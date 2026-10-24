//! `solidcraft-cli`: a former name of `node-cad-cli` (compatibility shim, see `alias.rs`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod alias;

fn main() -> std::process::ExitCode {
    alias::forward()
}
