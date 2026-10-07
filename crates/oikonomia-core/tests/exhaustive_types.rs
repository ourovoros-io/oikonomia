//! No public type of the crate is `#[non_exhaustive]`.
//!
//! The crate documentation records that decision and why. This test is what
//! makes it hold: the attribute cannot come back in one module without
//! someone reading the decision first.

#![expect(
    clippy::expect_used,
    reason = "the helper reads the crate's own sources, and a failure to read them fails the test"
)]

use std::fs;
use std::path::{Path, PathBuf};

/// Returns every `.rs` file under `directory`.
fn rust_sources(directory: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut pending = vec![directory.to_path_buf()];

    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).expect("the source directory is readable") {
            let path = entry.expect("a directory entry is readable").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }
    sources
}

#[test]
fn no_type_of_the_crate_is_marked_non_exhaustive() {
    let sources = rust_sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    // The crate has dozens of modules; finding none means the walk is broken.
    assert_ne!(sources.len(), 0);

    let marked: Vec<String> = sources
        .iter()
        .flat_map(|path| {
            let text = fs::read_to_string(path).unwrap();
            text.lines()
                .enumerate()
                .filter(|(_, line)| line.trim_start().starts_with("#[non_exhaustive"))
                .map(|(index, _)| format!("{}:{}", path.display(), index + 1))
                .collect::<Vec<_>>()
        })
        .collect();

    assert_eq!(marked, Vec::<String>::new());
}
