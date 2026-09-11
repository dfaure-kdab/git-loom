//! Gives cargo a reason to build the binary that `cargo test` needs.
//!
//! The unit tests drive real rebases, and hand `target/<profile>/git-loom` to
//! git as the sequence editor. Cargo builds a package's binaries for
//! `cargo test` only when there is an integration test to build them for, so
//! this file existing is what puts the binary there — and what rebuilds it
//! when a source file changes.

use std::path::Path;

#[test]
fn the_binary_the_unit_tests_drive_is_built() {
    let exe = env!("CARGO_BIN_EXE_git-loom");
    assert!(Path::new(exe).exists(), "'{exe}' was not built");
}
