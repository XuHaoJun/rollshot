use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use fs4::FileExt;

use crate::error::{Result, StoreError};

/// Write `bytes` to `path` atomically: serialize to a sibling `.tmp`, fsync,
/// then rename over the destination. A reader sees the old file or the new
/// one, never a partial write.
#[allow(dead_code)]
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| StoreError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    std::fs::create_dir_all(parent).map_err(|source| StoreError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    let tmp = path.with_extension("tmp");
    {
        let mut file = File::create(&tmp).map_err(|source| StoreError::Io {
            path: tmp.clone(),
            source,
        })?;
        file.write_all(bytes).map_err(|source| StoreError::Io {
            path: tmp.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| StoreError::Io {
            path: tmp.clone(),
            source,
        })?;
    }

    std::fs::rename(&tmp, path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    // Best-effort: fsync the directory so the rename is durable.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Read all bytes at `path`, returning `None` if the file does not exist.
#[allow(dead_code)]
pub(crate) fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(StoreError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// RAII advisory lock over a preset directory. The OS releases the flock when
/// the held file is dropped/closed.
#[allow(dead_code)]
pub(crate) struct DirLock {
    _file: File,
}

/// Acquire a blocking exclusive advisory lock on `<dir>/.lock`, creating `dir`
/// if needed. Serializes concurrent `preset.json` mutations across processes.
#[allow(dead_code)]
pub(crate) fn lock_dir(dir: &Path) -> Result<DirLock> {
    std::fs::create_dir_all(dir).map_err(|source| StoreError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let path = dir.join(".lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
    FileExt::lock(&file).map_err(|source| StoreError::Io { path, source })?;
    Ok(DirLock { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("file.json");
        write_atomic(&path, b"hello").unwrap();
        assert_eq!(read_optional_bytes(&path).unwrap(), Some(b"hello".to_vec()));
    }

    #[test]
    fn read_optional_bytes_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.json");
        assert_eq!(read_optional_bytes(&path).unwrap(), None);
    }

    #[test]
    fn write_atomic_leaves_no_tmp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.json");
        write_atomic(&path, b"data").unwrap();
        assert!(!dir.path().join("file.tmp").exists());
        assert!(path.exists());
    }

    #[test]
    fn lock_dir_serializes_two_handles() {
        use fs4::FileExt;
        let dir = tempfile::tempdir().unwrap();
        let guard = lock_dir(dir.path()).unwrap();

        // A second exclusive try-lock on the same lock file must report contention
        // while the first guard is alive (mirrors daemon InstanceGuard semantics).
        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.path().join(".lock"))
            .unwrap();
        assert!(matches!(
            FileExt::try_lock(&second),
            Err(fs4::TryLockError::WouldBlock)
        ));

        drop(guard);
    }
}
