//! Workspace-aware path helpers.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static WORKSPACE_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn workspace_root() -> &'static Path {
    WORKSPACE_ROOT.get_or_init(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
    })
}

pub fn resolve_workspace_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root().join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_root_contains_root_manifest() {
        assert!(workspace_root().join("Cargo.toml").exists());
    }

    #[test]
    fn test_resolve_workspace_path_joins_relative_paths() {
        assert_eq!(
            resolve_workspace_path("config/default.yaml"),
            workspace_root().join("config/default.yaml")
        );
    }
}
