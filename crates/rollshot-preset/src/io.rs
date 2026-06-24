use std::fs::File;
use std::io::Write;
use std::path::Path;

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
}
