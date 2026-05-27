//! Helpers shared across harness impls.

use std::path::Path;

pub(super) fn binary_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

pub(super) fn file_mentions_sidekick_hook(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|c| c.contains("sidekick hook"))
        .unwrap_or(false)
}
