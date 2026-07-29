//! Conservative filesystem cleanup for orchestrator-owned terminal workspaces.

use std::{
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CleanupError {
    #[error("cleanup root is unavailable")]
    RootUnavailable,
    #[error("cleanup target escapes its configured root")]
    OutOfRoot,
    #[error("cleanup target contains a symlink")]
    SymlinkDetected,
    #[error("workspace ownership marker is missing or mismatched")]
    OwnershipMismatch,
    #[error("workspace is unavailable")]
    Missing,
    #[error("filesystem cleanup failed")]
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupReceipt {
    pub path: PathBuf,
    pub reclaimed_bytes: u64,
}

/// Removes one workspace only after proving that it is an in-root, non-symlink
/// tree carrying the exact marker written by `WorkspaceAllocator`. Database
/// terminal-state and lease checks happen before this adapter is called.
pub fn cleanup_owned_workspace(
    root: impl AsRef<Path>,
    target: impl AsRef<Path>,
    expected_work_item_id: &str,
    expected_run_id: &str,
) -> Result<CleanupReceipt, CleanupError> {
    let requested_root = root.as_ref();
    let root = requested_root
        .canonicalize()
        .map_err(|_| CleanupError::RootUnavailable)?;
    let relative = target
        .as_ref()
        .strip_prefix(requested_root)
        .map_err(|_| CleanupError::OutOfRoot)?;
    if relative.as_os_str().is_empty() {
        return Err(CleanupError::OutOfRoot);
    }
    let target = root.join(relative);
    let mut current = root.clone();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(CleanupError::OutOfRoot);
        }
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                CleanupError::Missing
            } else {
                CleanupError::Io
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(CleanupError::SymlinkDetected);
        }
    }
    let marker = fs::read_to_string(target.join(".spire-owner"))
        .map_err(|_| CleanupError::OwnershipMismatch)?;
    let expected = format!("work_item_id={expected_work_item_id}\nrun_id={expected_run_id}\n");
    if marker != expected {
        return Err(CleanupError::OwnershipMismatch);
    }
    let reclaimed_bytes = tree_size(&target)?;
    fs::remove_dir_all(&target).map_err(|_| CleanupError::Io)?;
    Ok(CleanupReceipt {
        path: target,
        reclaimed_bytes,
    })
}

fn tree_size(path: &Path) -> Result<u64, CleanupError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CleanupError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(CleanupError::SymlinkDetected);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    fs::read_dir(path)
        .map_err(|_| CleanupError::Io)?
        .try_fold(0_u64, |total, entry| {
            let entry = entry.map_err(|_| CleanupError::Io)?;
            tree_size(&entry.path()).map(|size| total.saturating_add(size))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "spire-cleanup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn cleanup_refuses_unowned_and_out_of_root_paths() {
        let root = root();
        let target = root.join("work").join("run");
        fs::create_dir_all(&target).unwrap();
        assert_eq!(
            cleanup_owned_workspace(&root, &target, "work", "run"),
            Err(CleanupError::OwnershipMismatch)
        );
        assert_eq!(
            cleanup_owned_workspace(&root, std::env::temp_dir(), "work", "run"),
            Err(CleanupError::OutOfRoot)
        );
    }

    #[test]
    fn cleanup_removes_only_the_marked_workspace_and_counts_bytes() {
        let root = root();
        let target = root.join("work").join("run");
        fs::create_dir_all(&target).unwrap();
        fs::write(
            target.join(".spire-owner"),
            "work_item_id=work\nrun_id=run\n",
        )
        .unwrap();
        fs::write(target.join("evidence"), "abcd").unwrap();
        let receipt = cleanup_owned_workspace(&root, &target, "work", "run").unwrap();
        assert!(receipt.reclaimed_bytes >= 4);
        assert!(!target.exists());
        assert!(root.exists());
    }
}
