use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace identifier is unsafe")]
    UnsafeIdentifier,
    #[error("workspace root is unavailable")]
    RootUnavailable,
    #[error("workspace path escapes its configured root")]
    PathEscape,
    #[error("workspace already exists")]
    AlreadyAllocated,
    #[error("filesystem operation failed")]
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocatedWorkspace {
    pub path: PathBuf,
    pub branch: String,
    pub ownership_marker: PathBuf,
}

pub struct WorkspaceAllocator {
    root: PathBuf,
}
impl WorkspaceAllocator {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, WorkspaceError> {
        let root = root.into();
        if !root.is_absolute() || !root.is_dir() {
            return Err(WorkspaceError::RootUnavailable);
        }
        Ok(Self {
            root: root
                .canonicalize()
                .map_err(|_| WorkspaceError::RootUnavailable)?,
        })
    }
    pub fn allocate(
        &self,
        work_item_id: &str,
        run_id: &str,
    ) -> Result<AllocatedWorkspace, WorkspaceError> {
        if !safe_component(work_item_id) || !safe_component(run_id) {
            return Err(WorkspaceError::UnsafeIdentifier);
        }
        let branch = format!("agent/{work_item_id}-{run_id}");
        let path = self.root.join(work_item_id).join(run_id);
        let parent = path.parent().ok_or(WorkspaceError::PathEscape)?;
        std::fs::create_dir_all(parent).map_err(|_| WorkspaceError::Io)?;
        if parent
            .canonicalize()
            .map_err(|_| WorkspaceError::Io)?
            .strip_prefix(&self.root)
            .is_err()
        {
            return Err(WorkspaceError::PathEscape);
        }
        std::fs::create_dir(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                WorkspaceError::AlreadyAllocated
            } else {
                WorkspaceError::Io
            }
        })?;
        let ownership_marker = path.join(".spire-owner");
        std::fs::write(
            &ownership_marker,
            format!("work_item_id={work_item_id}\nrun_id={run_id}\n"),
        )
        .map_err(|_| WorkspaceError::Io)?;
        Ok(AllocatedWorkspace {
            path,
            branch,
            ownership_marker,
        })
    }
}
fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    #[test]
    fn rejects_traversal_and_duplicate_paths() {
        let root = std::env::temp_dir().join(format!(
            "spire-workspace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let allocator = WorkspaceAllocator::new(&root).unwrap();
        assert!(matches!(
            allocator.allocate("../escape", "run"),
            Err(WorkspaceError::UnsafeIdentifier)
        ));
        allocator.allocate("work", "run").unwrap();
        assert!(matches!(
            allocator.allocate("work", "run"),
            Err(WorkspaceError::AlreadyAllocated)
        ));
    }
}
