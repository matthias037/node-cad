//! The `solidcraft-cli` and `solvecraft-cli` compatibility aliases forward to `node-cad-cli`.
use std::process::Command;

const ALIASES: [(&str, &str); 2] = [("solidcraft-cli", env!("CARGO_BIN_EXE_solidcraft-cli")), ("solvecraft-cli", env!("CARGO_BIN_EXE_solvecraft-cli"))];

#[test]
fn legacy_aliases_forward_with_a_deprecation_note() {
    for (name, alias) in ALIASES {
        let out = Command::new(alias).arg("--version").output().unwrap();
        assert!(out.status.success(), "{name}: {out:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(stdout.trim(), format!("node-cad-cli {}", env!("CARGO_PKG_VERSION")), "{name}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("deprecated") && stderr.contains(name) && stderr.contains("node-cad-cli"), "{stderr}");
    }
}

#[test]
fn legacy_aliases_pass_the_exit_status_through() {
    let real = Command::new(env!("CARGO_BIN_EXE_node-cad-cli")).arg("no-such-command").output().unwrap();
    for (name, alias) in ALIASES {
        let out = Command::new(alias).arg("no-such-command").output().unwrap();
        assert_eq!(out.status.code(), real.status.code(), "{name}");
        assert_eq!(out.stdout, real.stdout, "{name}");
    }
}
