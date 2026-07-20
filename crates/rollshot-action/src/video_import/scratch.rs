use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rustix::fs::{flock, FlockOperation};

use super::VideoImportError;

struct ImportedScratchInner {
    root: PathBuf,
    bytes_used: u64,
    lock_file: std::fs::File,
}

pub struct ImportedScratch(ImportedScratchInner);

impl std::fmt::Debug for ImportedScratch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportedScratch")
            .field("bytes_used", &self.0.bytes_used)
            .finish()
    }
}

impl ImportedScratch {
    pub fn create(parent: &Path) -> Result<Self, VideoImportError> {
        fs::create_dir_all(parent).map_err(|_| VideoImportError::ScratchIo)?;

        let canonical_parent = fs::canonicalize(parent).map_err(|_| VideoImportError::ScratchIo)?;

        let dir_name = format!("import-{}-{}", std::process::id(), generate_nonce());
        let root = canonical_parent.join(&dir_name);
        fs::create_dir(&root).map_err(|_| VideoImportError::ScratchIo)?;

        let lock_path = root.join(".lock");
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| {
                let _ = fs::remove_dir_all(&root);
                VideoImportError::ScratchIo
            })?;

        flock(&lock_file, FlockOperation::LockExclusive).map_err(|_| {
            let _ = fs::remove_dir_all(&root);
            VideoImportError::ScratchIo
        })?;

        Ok(Self(ImportedScratchInner {
            root,
            bytes_used: 0,
            lock_file,
        }))
    }

    pub fn try_lock_existing(dir: &Path) -> Option<Self> {
        let lock_path = dir.join(".lock");
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .ok()?;

        flock(&lock_file, FlockOperation::NonBlockingLockExclusive).ok()?;

        let bytes_used = dir_size(dir).unwrap_or(0);

        Some(Self(ImportedScratchInner {
            root: dir.to_path_buf(),
            bytes_used,
            lock_file,
        }))
    }

    pub fn root(&self) -> &Path {
        &self.0.root
    }

    pub fn add_bytes(&mut self, n: u64) {
        self.0.bytes_used = self.0.bytes_used.saturating_add(n);
    }

    pub fn bytes_used(&self) -> u64 {
        self.0.bytes_used
    }

    fn release_lock(&mut self) {
        let _ = flock(&self.0.lock_file, FlockOperation::Unlock);
    }
}

impl Drop for ImportedScratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0.root);
        self.release_lock();
    }
}

fn generate_nonce() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut hasher = s.build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );
    hasher.write_u64(std::process::id() as u64);
    hasher.finish()
}

fn dir_size(dir: &Path) -> io::Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
    }
    Ok(total)
}

pub fn cleanup_stale_import_scratch(parent: &Path) {
    let canonical = match fs::canonicalize(parent) {
        Ok(p) => p,
        Err(_) => return,
    };

    let entries = match fs::read_dir(&canonical) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("import-") {
            continue;
        }

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_dir() {
            continue;
        }

        let dir_path = entry.path();

        if let Some(_locked) = ImportedScratch::try_lock_existing(&dir_path) {
            let _ = fs::remove_dir_all(&dir_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_makes_directory_and_acquires_lock() {
        let parent = tempfile::tempdir().unwrap();
        let scratch = ImportedScratch::create(parent.path()).unwrap();
        assert!(scratch.root().exists());
        assert!(scratch.root().starts_with(parent.path()));
        assert!(scratch.root().to_string_lossy().contains("import-"));
    }

    #[test]
    fn drop_removes_directory_and_releases_lock() {
        let parent = tempfile::tempdir().unwrap();
        let root;
        {
            let scratch = ImportedScratch::create(parent.path()).unwrap();
            root = scratch.root().to_path_buf();
            assert!(root.exists());
        }
        assert!(!root.exists());
    }

    #[test]
    fn two_scratches_do_not_collide() {
        let parent = tempfile::tempdir().unwrap();
        let a = ImportedScratch::create(parent.path()).unwrap();
        let b = ImportedScratch::create(parent.path()).unwrap();
        assert_ne!(a.root(), b.root());
    }

    #[test]
    fn try_lock_existing_locks_stale_directory() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("import-stale-123");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join(".lock"), b"").unwrap();

        let locked = ImportedScratch::try_lock_existing(&dir).unwrap();
        assert_eq!(locked.root(), dir.as_path());
        assert!(locked.root().exists());
        // Don't let drop clean it — just test the lock was acquired.
        let _ = locked;
    }

    #[test]
    fn try_lock_existing_fails_when_held() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("import-locked-456");
        fs::create_dir(&dir).unwrap();

        // Hold the lock in a separate file handle
        let lock_path = dir.join(".lock");
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        flock(&lock_file, FlockOperation::LockExclusive).unwrap();

        assert!(ImportedScratch::try_lock_existing(&dir).is_none());
    }

    #[test]
    fn cleanup_stale_removes_lockable_directories() {
        let parent = tempfile::tempdir().unwrap();

        let stale = parent.path().join("import-stale-aaa");
        fs::create_dir(&stale).unwrap();
        fs::write(stale.join(".lock"), b"").unwrap();

        let non_import = parent.path().join("other-dir");
        fs::create_dir(&non_import).unwrap();

        let import_file = parent.path().join("import-not-a-dir");
        fs::write(&import_file, b"").unwrap();

        cleanup_stale_import_scratch(parent.path());

        assert!(!stale.exists());
        assert!(non_import.exists());
    }

    #[test]
    fn cleanup_skips_held_locks() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("import-active-bbb");
        fs::create_dir(&dir).unwrap();

        let lock_path = dir.join(".lock");
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        flock(&lock_file, FlockOperation::LockExclusive).unwrap();

        cleanup_stale_import_scratch(parent.path());
        assert!(dir.exists());
    }

    #[test]
    fn add_bytes_tracks_usage() {
        let parent = tempfile::tempdir().unwrap();
        let mut scratch = ImportedScratch::create(parent.path()).unwrap();
        assert_eq!(scratch.bytes_used(), 0);
        scratch.add_bytes(1024);
        assert_eq!(scratch.bytes_used(), 1024);
        scratch.add_bytes(u64::MAX - 100);
        assert_eq!(scratch.bytes_used(), u64::MAX);
    }
}
