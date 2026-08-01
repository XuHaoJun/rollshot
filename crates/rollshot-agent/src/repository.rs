//! Bounded, auditable repository reader for launch-teaser skill runs.
//!
//! A `RepositoryReadGrant` owns a per-run authorization to read specific
//! files beneath a workspace root. The companion `RepositoryReadTool`
//! exposes a registered `read_authorized_project_text` tool that enforces
//! denylist, byte/file ceilings, and records privacy-safe receipts.
//!
//! Absolute paths never enter model input, tool results, receipts,
//! Debug output, or user-visible errors.

use std::fmt;
use std::os::unix::io::OwnedFd;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use rustix::fs::{FileType, Mode, OFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::authority::RunOperation;
use crate::runtime::RunCancellation;
use crate::tools::{Tool, ToolFuture, ToolOutcome};

// ========================================================================
// Limits
// ========================================================================

/// Fixed read ceilings for a repository grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryReadLimits {
    pub max_files: usize,
    pub max_bytes_per_file: usize,
    pub max_total_bytes: usize,
    pub max_total_return_bytes: usize,
}

impl RepositoryReadLimits {
    /// The V1 fixed limits: 64 files, 64 KiB/file, 512 KiB total read,
    /// 256 KiB total returned text.
    pub fn v1() -> Self {
        Self {
            max_files: 64,
            max_bytes_per_file: 64 * 1024,
            max_total_bytes: 512 * 1024,
            max_total_return_bytes: 256 * 1024,
        }
    }
}

// ========================================================================
// Errors
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepositoryReadError {
    #[error("root path does not exist or is not a directory")]
    InvalidRoot,
    #[error("entry is not a valid relative path: {0}")]
    InvalidEntry(String),
    #[error("entry contains a denied component: {0}")]
    DeniedComponent(String),
    #[error("io error: {0}")]
    Io(String),
}

impl From<std::io::Error> for RepositoryReadError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

// ========================================================================
// Receipts
// ========================================================================

/// Grant-level receipt identifying the authorized scope without exposing
/// the absolute root path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryReadGrantReceiptV1 {
    pub schema_version: u32,
    pub root_identity_sha256: String,
    pub grant_sha256: String,
    pub entries: Vec<String>,
    pub limits: RepositoryReadLimits,
}

/// Per-file receipt recorded after a successful or truncated read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryReadReceiptV1 {
    pub relative_path: String,
    pub content_sha256: String,
    pub bytes_read: u64,
    pub bytes_returned: u64,
    pub truncated: bool,
}

// ========================================================================
// Denylist
// ========================================================================

const DENY_COMPONENTS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".env",
    ".ssh",
    "secrets",
    "credentials",
];

const DENY_SUFFIXES: &[&str] = &[".key", ".pem", ".p12", ".pfx"];

const ALLOWED_EXTENSIONS: &[&str] = &[
    "md", "txt", "rs", "toml", "json", "yaml", "yml", "js", "jsx", "ts", "tsx", "css", "html",
    "swift", "m", "mm", "c", "cc", "cpp", "h", "hpp", "go", "py", "java",
];

fn is_denied_component(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    DENY_COMPONENTS.iter().any(|d| *d == lower) || DENY_SUFFIXES.iter().any(|s| lower.ends_with(s))
}

fn is_allowed_extension(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, ext)) => ALLOWED_EXTENSIONS
            .iter()
            .any(|a| *a == ext.to_ascii_lowercase()),
        None => false,
    }
}

// ========================================================================
// Path validation
// ========================================================================

/// Validate a relative entry path. Returns normalized components or an error.
fn validate_entry(entry: &str) -> Result<Vec<String>, RepositoryReadError> {
    if entry.is_empty() {
        return Err(RepositoryReadError::InvalidEntry(entry.to_string()));
    }
    let path = Path::new(entry);
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            Component::Normal(c) => {
                let s = c
                    .to_str()
                    .ok_or_else(|| RepositoryReadError::InvalidEntry(entry.to_string()))?;
                components.push(s.to_string());
            }
            Component::CurDir => {} // skip .
            Component::RootDir | Component::Prefix(_) => {
                return Err(RepositoryReadError::InvalidEntry(entry.to_string()));
            }
            Component::ParentDir => {
                return Err(RepositoryReadError::InvalidEntry(entry.to_string()));
            }
        }
    }
    if components.is_empty() {
        return Err(RepositoryReadError::InvalidEntry(entry.to_string()));
    }
    // Check each component against the denylist.
    for c in &components {
        if is_denied_component(c) {
            return Err(RepositoryReadError::DeniedComponent(entry.to_string()));
        }
    }
    Ok(components)
}

// ========================================================================
// Hashing helpers
// ========================================================================

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash_root_identity(root: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rollshot.repository.root.v1|");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(root) {
            hasher.update(meta.dev().to_le_bytes());
            hasher.update(meta.ino().to_le_bytes());
        } else {
            hasher.update(root.to_string_lossy().as_bytes());
        }
    }
    #[cfg(not(unix))]
    {
        hasher.update(root.to_string_lossy().as_bytes());
    }
    hex_encode(&hasher.finalize())
}

fn hash_grant(entries: &[String], limits: &RepositoryReadLimits) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rollshot.repository.grant.v1|");
    for e in entries {
        hasher.update(e.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(limits.max_files.to_le_bytes());
    hasher.update(limits.max_bytes_per_file.to_le_bytes());
    hasher.update(limits.max_total_bytes.to_le_bytes());
    hasher.update(limits.max_total_return_bytes.to_le_bytes());
    hex_encode(&hasher.finalize())
}

// ========================================================================
// Grant
// ========================================================================

/// Validated, bounded grant to read specific files beneath a root directory.
///
/// The root path is private: it never appears in Debug output or receipts.
pub struct RepositoryReadGrant {
    root: PathBuf,
    root_fd: OwnedFd,
    entries: Vec<String>,
    limits: RepositoryReadLimits,
    root_identity: String,
    grant_hash: String,
}

impl fmt::Debug for RepositoryReadGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepositoryReadGrant")
            .field("root", &"<redacted>")
            .field("entries", &self.entries)
            .field("limits", &self.limits)
            .finish()
    }
}

impl RepositoryReadGrant {
    /// Open and validate a grant. The root must be an existing directory.
    /// Each entry is a relative path (file or directory) beneath the root.
    pub fn open(
        root: &Path,
        entries: Vec<String>,
        limits: RepositoryReadLimits,
    ) -> Result<Self, RepositoryReadError> {
        let meta = std::fs::metadata(root).map_err(|_| RepositoryReadError::InvalidRoot)?;
        if !meta.is_dir() {
            return Err(RepositoryReadError::InvalidRoot);
        }

        // Open the root directory once and retain the descriptor.
        let root_fd = rustix::fs::open(
            root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|e| RepositoryReadError::Io(format!("open root: {e}")))?;

        let mut validated = Vec::new();
        for entry in &entries {
            let comps = validate_entry(entry)?;
            validated.push(comps.join("/"));
        }
        validated.sort();
        validated.dedup();

        let root_identity = hash_root_identity(root);
        let grant_hash = hash_grant(&validated, &limits);

        Ok(Self {
            root: root.to_path_buf(),
            root_fd,
            entries: validated,
            limits,
            root_identity,
            grant_hash,
        })
    }

    /// Return the grant-level receipt. Does not expose the root path.
    pub fn receipt(&self) -> RepositoryReadGrantReceiptV1 {
        RepositoryReadGrantReceiptV1 {
            schema_version: 1,
            root_identity_sha256: self.root_identity.clone(),
            grant_sha256: self.grant_hash.clone(),
            entries: self.entries.clone(),
            limits: self.limits,
        }
    }

    /// Check if a relative path is within the grant scope.
    fn is_in_scope(&self, relative: &str) -> bool {
        for entry in &self.entries {
            if relative == *entry || relative.starts_with(&format!("{entry}/")) {
                return true;
            }
        }
        false
    }
}

// ========================================================================
// Tool handle
// ========================================================================

/// Handle holding the tool and its collected receipts.
pub struct RepositoryReadToolHandle {
    tool: Arc<dyn Tool>,
    receipts: Arc<Mutex<Vec<RepositoryReadReceiptV1>>>,
}

impl RepositoryReadToolHandle {
    pub fn tool(&self) -> Arc<dyn Tool> {
        self.tool.clone()
    }

    pub fn receipts(&self) -> Vec<RepositoryReadReceiptV1> {
        self.receipts.lock().unwrap().clone()
    }
}

// ========================================================================
// Tool implementation
// ========================================================================

struct RepositoryReadToolInner {
    grant: RepositoryReadGrant,
    cancellation: RunCancellation,
    files_read: Mutex<usize>,
    total_bytes_read: Mutex<u64>,
    total_bytes_returned: Mutex<u64>,
    receipts: Arc<Mutex<Vec<RepositoryReadReceiptV1>>>,
}

struct RepositoryReadTool {
    inner: Arc<RepositoryReadToolInner>,
}

impl RepositoryReadTool {
    #[allow(clippy::new_ret_no_self)]
    fn new(grant: RepositoryReadGrant, cancellation: RunCancellation) -> RepositoryReadToolHandle {
        let inner = Arc::new(RepositoryReadToolInner {
            grant,
            cancellation,
            files_read: Mutex::new(0),
            total_bytes_read: Mutex::new(0),
            total_bytes_returned: Mutex::new(0),
            receipts: Arc::new(Mutex::new(Vec::new())),
        });
        let tool: Arc<dyn Tool> = Arc::new(Self {
            inner: inner.clone(),
        });
        RepositoryReadToolHandle {
            tool,
            receipts: inner.receipts.clone(),
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadAuthorizedProjectTextArgs {
    path: String,
}

impl Tool for RepositoryReadTool {
    fn name(&self) -> &str {
        "read_authorized_project_text"
    }

    fn json_schema(&self) -> serde_json::Value {
        crate::tools::tool_schema::<ReadAuthorizedProjectTextArgs>()
    }

    fn required_operations(&self) -> &'static [RunOperation] {
        &[RunOperation::ReadAuthorizedWorkspaceFile]
    }

    fn call<'a>(&'a self, arguments: &'a serde_json::Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: ReadAuthorizedProjectTextArgs =
                match serde_json::from_value(arguments.clone()) {
                    Ok(a) => a,
                    Err(e) => {
                        return Ok(ToolOutcome::Recoverable {
                            error: format!("invalid arguments: {e}"),
                        });
                    }
                };

            if self.inner.cancellation.is_cancelled() {
                return Ok(ToolOutcome::Recoverable {
                    error: "run cancelled".to_string(),
                });
            }

            let components = match validate_entry(&args.path) {
                Ok(c) => c,
                Err(e) => {
                    return Ok(ToolOutcome::Recoverable {
                        error: format!("invalid path: {e}"),
                    });
                }
            };
            let relative = components.join("/");

            if !self.inner.grant.is_in_scope(&relative) {
                return Ok(ToolOutcome::Recoverable {
                    error: "path not in grant scope".to_string(),
                });
            }

            // Check aggregate limits before opening.
            {
                let files = self.inner.files_read.lock().unwrap();
                if *files >= self.inner.grant.limits.max_files {
                    return Ok(ToolOutcome::Recoverable {
                        error: "file limit reached".to_string(),
                    });
                }
            }
            {
                let total = self.inner.total_bytes_read.lock().unwrap();
                if *total >= self.inner.grant.limits.max_total_bytes as u64 {
                    return Ok(ToolOutcome::Recoverable {
                        error: "total read limit reached".to_string(),
                    });
                }
            }
            {
                let total_ret = self.inner.total_bytes_returned.lock().unwrap();
                if *total_ret >= self.inner.grant.limits.max_total_return_bytes as u64 {
                    return Ok(ToolOutcome::Recoverable {
                        error: "total return limit reached".to_string(),
                    });
                }
            }

            let result =
                read_via_descriptor(&self.inner.grant.root_fd, &components, &self.inner.grant);

            match result {
                Ok(ReadResult {
                    content,
                    bytes_read,
                    truncated,
                }) => {
                    if self.inner.cancellation.is_cancelled() {
                        return Ok(ToolOutcome::Recoverable {
                            error: "run cancelled".to_string(),
                        });
                    }

                    let content_sha256 = hex_encode(&Sha256::digest(content.as_bytes()));
                    let bytes_returned = content.len() as u64;

                    {
                        let mut files = self.inner.files_read.lock().unwrap();
                        *files += 1;
                        let mut total = self.inner.total_bytes_read.lock().unwrap();
                        *total += bytes_read;
                        let mut total_ret = self.inner.total_bytes_returned.lock().unwrap();
                        *total_ret += bytes_returned;
                    }

                    self.inner
                        .receipts
                        .lock()
                        .unwrap()
                        .push(RepositoryReadReceiptV1 {
                            relative_path: relative.clone(),
                            content_sha256: content_sha256.clone(),
                            bytes_read,
                            bytes_returned,
                            truncated,
                        });

                    Ok(ToolOutcome::Success {
                        result_json: serde_json::json!({
                            "path": relative,
                            "content": content,
                            "content_sha256": content_sha256,
                            "bytes_read": bytes_read,
                            "bytes_returned": bytes_returned,
                            "truncated": truncated,
                        }),
                    })
                }
                Err(e) => Ok(ToolOutcome::Recoverable {
                    error: e.to_string(),
                }),
            }
        })
    }
}

// ========================================================================
// Descriptor-relative file reading
// ========================================================================

struct ReadResult {
    content: String,
    bytes_read: u64,
    truncated: bool,
}

/// Read a file using descriptor-relative traversal (no-follow at every step).
fn read_via_descriptor(
    root_fd: &OwnedFd,
    components: &[String],
    grant: &RepositoryReadGrant,
) -> Result<ReadResult, RepositoryReadError> {
    use rustix::fs::{fstat, openat};

    // Clone the root fd so we own the traversal chain.
    let mut current_fd = root_fd
        .try_clone()
        .map_err(|e| RepositoryReadError::Io(format!("clone root fd: {e}")))?;
    for (i, comp) in components.iter().enumerate() {
        let is_last = i == components.len() - 1;

        // Check denylist for every component.
        if is_denied_component(comp) {
            return Err(RepositoryReadError::DeniedComponent(comp.clone()));
        }

        if is_last {
            // Last component: try to open as a regular file.
            let flags = OFlags::RDONLY | OFlags::NOFOLLOW;
            let file_fd = match openat(&current_fd, comp.as_str(), flags, Mode::empty()) {
                Ok(fd) => fd,
                Err(e) => {
                    return Err(RepositoryReadError::Io(format!("openat({comp}): {e}")));
                }
            };

            let stat =
                fstat(&file_fd).map_err(|e| RepositoryReadError::Io(format!("fstat: {e}")))?;
            let ft = FileType::from_raw_mode(stat.st_mode);
            if ft != FileType::RegularFile {
                return Err(RepositoryReadError::Io(
                    "target is not a regular file".to_string(),
                ));
            }

            if !is_allowed_extension(comp) {
                return Err(RepositoryReadError::Io(format!(
                    "unsupported file extension: {comp}"
                )));
            }

            return read_file_content(&file_fd, &grant.limits);
        }

        // Intermediate component: must be a directory.
        let dir_fd = openat(
            &current_fd,
            comp.as_str(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|e| RepositoryReadError::Io(format!("openat({comp}): {e}")))?;

        let stat = fstat(&dir_fd).map_err(|e| RepositoryReadError::Io(format!("fstat: {e}")))?;
        let ft = FileType::from_raw_mode(stat.st_mode);
        if ft != FileType::Directory {
            return Err(RepositoryReadError::Io(
                "intermediate path is not a directory".to_string(),
            ));
        }

        current_fd = dir_fd;
    }

    // All components consumed — path points to a directory.
    Err(RepositoryReadError::Io(
        "path is a directory, not a file".to_string(),
    ))
}

fn read_file_content(
    fd: &impl rustix::fd::AsFd,
    grant: &RepositoryReadLimits,
) -> Result<ReadResult, RepositoryReadError> {
    use rustix::io::read;

    let max_read = grant.max_bytes_per_file;
    let max_return = grant.max_total_return_bytes;

    let mut buf = Vec::with_capacity(max_read.min(8192));
    let mut total_read: usize = 0;
    let mut truncated = false;

    let mut chunk = [0u8; 8192];
    loop {
        if total_read >= max_read {
            truncated = true;
            break;
        }
        let to_read = (max_read - total_read).min(chunk.len());
        match read(fd, &mut chunk[..to_read]) {
            Ok(0) => break,
            Ok(n) => {
                if chunk[..n].contains(&0) {
                    return Err(RepositoryReadError::Io(
                        "binary file content detected".to_string(),
                    ));
                }
                total_read += n;
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(RepositoryReadError::Io(e.to_string())),
        }
    }

    let content = String::from_utf8_lossy(&buf).to_string();

    let (final_content, was_truncated) = if content.len() > max_return {
        let mut end = max_return;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        (content[..end].to_string(), true)
    } else {
        (content, truncated)
    };

    Ok(ReadResult {
        content: final_content,
        bytes_read: total_read as u64,
        truncated: was_truncated,
    })
}

// ========================================================================
// Public constructor
// ========================================================================

/// Create a new repository read tool from a grant and cancellation token.
pub fn repository_read_tool(
    grant: RepositoryReadGrant,
    cancellation: RunCancellation,
) -> RepositoryReadToolHandle {
    RepositoryReadTool::new(grant, cancellation)
}

// ========================================================================
// Tests
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct Fixture {
        root: TempDir,
        outside: TempDir,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        std::fs::create_dir_all(root.path().join("docs")).unwrap();
        std::fs::write(root.path().join("README.md"), b"# Hello\n").unwrap();
        std::fs::write(root.path().join("docs/guide.md"), b"# Guide\n").unwrap();
        std::fs::write(root.path().join("docs/code.rs"), b"fn main() {}\n").unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"secret\n").unwrap();

        Fixture { root, outside }
    }

    fn args(path: &str) -> serde_json::Value {
        serde_json::json!({"path": path})
    }

    #[tokio::test]
    async fn exact_file_read_succeeds() {
        let f = fixture();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["README.md".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let result = handle.tool().call(&args("README.md")).await.unwrap();
        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["path"], "README.md");
                assert_eq!(result_json["content"], "# Hello\n");
                assert_eq!(result_json["truncated"], false);
                assert!(result_json["content_sha256"].as_str().unwrap().len() == 64);
            }
            other => panic!("expected Success, got {other:?}"),
        }
        assert_eq!(handle.receipts().len(), 1);
        assert_eq!(handle.receipts()[0].relative_path, "README.md");
    }

    #[tokio::test]
    async fn directory_grant_allows_descendants() {
        let f = fixture();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["docs".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let result = handle.tool().call(&args("docs/guide.md")).await.unwrap();
        assert!(matches!(result, ToolOutcome::Success { .. }));
    }

    #[tokio::test]
    async fn path_outside_grant_scope_is_rejected() {
        let f = fixture();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["docs".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let result = handle.tool().call(&args("README.md")).await.unwrap();
        assert!(matches!(result, ToolOutcome::Recoverable { .. }));
        assert!(handle.receipts().is_empty());
    }

    #[tokio::test]
    async fn dotdot_traversal_is_rejected() {
        let f = fixture();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["docs".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let result = handle.tool().call(&args("../README.md")).await.unwrap();
        assert!(matches!(result, ToolOutcome::Recoverable { .. }));
    }

    #[tokio::test]
    async fn absolute_entry_is_rejected() {
        let f = fixture();
        let result = RepositoryReadGrant::open(
            f.root.path(),
            vec!["/etc/passwd".into()],
            RepositoryReadLimits::v1(),
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn symlink_inside_grant_cannot_escape_root() {
        let f = fixture();
        std::os::unix::fs::symlink(
            f.outside.path().join("secret.txt"),
            f.root.path().join("docs/link.txt"),
        )
        .unwrap();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["docs".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let result = handle.tool().call(&args("docs/link.txt")).await.unwrap();
        assert!(matches!(result, ToolOutcome::Recoverable { .. }));
        assert!(handle.receipts().is_empty());
    }

    #[tokio::test]
    async fn symlink_directory_cannot_escape_root() {
        let f = fixture();
        std::os::unix::fs::symlink(f.outside.path(), f.root.path().join("escape")).unwrap();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["escape".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let result = handle
            .tool()
            .call(&args("escape/secret.txt"))
            .await
            .unwrap();
        assert!(matches!(result, ToolOutcome::Recoverable { .. }));
    }

    #[tokio::test]
    async fn denied_component_dot_git_is_rejected() {
        let f = fixture();
        std::fs::create_dir_all(f.root.path().join(".git/objects")).unwrap();
        std::fs::write(f.root.path().join(".git/config"), b"x").unwrap();
        let result = RepositoryReadGrant::open(
            f.root.path(),
            vec![".git/config".into()],
            RepositoryReadLimits::v1(),
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn denied_suffix_pem_is_rejected() {
        let f = fixture();
        std::fs::write(f.root.path().join("cert.pem"), b"x").unwrap();
        let result = RepositoryReadGrant::open(
            f.root.path(),
            vec!["cert.pem".into()],
            RepositoryReadLimits::v1(),
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn binary_nul_content_is_rejected() {
        let f = fixture();
        std::fs::write(f.root.path().join("bin.txt"), b"hello\0world").unwrap();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["bin.txt".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let result = handle.tool().call(&args("bin.txt")).await.unwrap();
        assert!(matches!(result, ToolOutcome::Recoverable { .. }));
    }

    #[tokio::test]
    async fn unsupported_extension_is_rejected() {
        let f = fixture();
        std::fs::write(f.root.path().join("image.png"), b"png").unwrap();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["image.png".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let result = handle.tool().call(&args("image.png")).await.unwrap();
        assert!(matches!(result, ToolOutcome::Recoverable { .. }));
    }

    #[tokio::test]
    async fn per_file_limit_truncates() {
        let f = fixture();
        let big = "x".repeat(100_000);
        std::fs::write(f.root.path().join("big.md"), big.as_bytes()).unwrap();
        let limits = RepositoryReadLimits {
            max_bytes_per_file: 100,
            ..RepositoryReadLimits::v1()
        };
        let grant =
            RepositoryReadGrant::open(f.root.path(), vec!["big.md".into()], limits).unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let result = handle.tool().call(&args("big.md")).await.unwrap();
        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["truncated"], true);
                assert_eq!(result_json["bytes_read"], 100);
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancellation_prevents_read() {
        let f = fixture();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["README.md".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let cancel = RunCancellation::new();
        cancel.cancel();
        let handle = repository_read_tool(grant, cancel);
        let result = handle.tool().call(&args("README.md")).await.unwrap();
        assert!(matches!(result, ToolOutcome::Recoverable { .. }));
    }

    #[tokio::test]
    async fn grant_receipt_omits_root_path() {
        let f = fixture();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["README.md".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let receipt = grant.receipt();
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(!json.contains(f.root.path().to_str().unwrap()));
        assert_eq!(receipt.schema_version, 1);
        assert!(!receipt.root_identity_sha256.is_empty());
        assert!(!receipt.grant_sha256.is_empty());
    }

    #[tokio::test]
    async fn debug_output_redacts_root() {
        let f = fixture();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["README.md".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let debug = format!("{grant:?}");
        assert!(!debug.contains(f.root.path().to_str().unwrap()));
        assert!(debug.contains("<redacted>"));
    }

    #[tokio::test]
    async fn multiple_reads_collect_receipts() {
        let f = fixture();
        let grant = RepositoryReadGrant::open(
            f.root.path(),
            vec!["docs".into()],
            RepositoryReadLimits::v1(),
        )
        .unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let _ = handle.tool().call(&args("docs/guide.md")).await;
        let _ = handle.tool().call(&args("docs/code.rs")).await;
        assert_eq!(handle.receipts().len(), 2);
    }

    #[tokio::test]
    async fn file_limit_prevents_extra_reads() {
        let f = fixture();
        let limits = RepositoryReadLimits {
            max_files: 1,
            ..RepositoryReadLimits::v1()
        };
        let grant = RepositoryReadGrant::open(f.root.path(), vec!["docs".into()], limits).unwrap();
        let handle = repository_read_tool(grant, RunCancellation::new());
        let _ = handle.tool().call(&args("docs/guide.md")).await;
        let result = handle.tool().call(&args("docs/code.rs")).await.unwrap();
        assert!(matches!(result, ToolOutcome::Recoverable { .. }));
    }
}
