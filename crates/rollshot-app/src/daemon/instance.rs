use fs4::{FileExt, TryLockError};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

pub struct InstanceGuard {
    _file: File,
}

pub enum AcquireResult {
    Acquired(InstanceGuard),
    AlreadyRunning,
}

pub fn lock_path() -> Result<PathBuf, String> {
    crate::daemon::config::rollshot_config_dir().map(|dir| dir.join("daemon.lock"))
}

pub fn acquire_at(path: &Path) -> Result<AcquireResult, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create daemon state directory: {error}"))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| format!("failed to open daemon lock: {error}"))?;

    match FileExt::try_lock(&file) {
        Ok(()) => Ok(AcquireResult::Acquired(InstanceGuard { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(AcquireResult::AlreadyRunning),
        Err(TryLockError::Error(error)) => Err(format!("failed to acquire daemon lock: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_guard_reports_already_running() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let first = acquire_at(&path).unwrap();
        assert!(matches!(first, AcquireResult::Acquired(_)));
        let second = acquire_at(&path).unwrap();
        assert!(matches!(second, AcquireResult::AlreadyRunning));
    }

    #[test]
    fn dropping_guard_allows_reacquisition() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let guard = match acquire_at(&path).unwrap() {
            AcquireResult::Acquired(guard) => guard,
            AcquireResult::AlreadyRunning => panic!("first lock must succeed"),
        };
        drop(guard);

        assert!(matches!(
            acquire_at(&path).unwrap(),
            AcquireResult::Acquired(_)
        ));
    }
}
