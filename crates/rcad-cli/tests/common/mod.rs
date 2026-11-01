//! Helpers shared by the CLI integration tests.
use std::path::PathBuf;

/// The `rcad-cli` binary under test. Tests run from a nextest archive see
/// the binary at its extracted path through `NEXTEST_BIN_EXE_*`; the
/// compile-time `CARGO_BIN_EXE_*` path only exists on the machine that
/// built it.
pub fn rcad_cli() -> PathBuf {
    ["NEXTEST_BIN_EXE_rcad-cli", "NEXTEST_BIN_EXE_rcad_cli"]
        .iter()
        .find_map(std::env::var_os)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_rcad-cli")))
}
