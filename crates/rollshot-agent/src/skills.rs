//! Bounded static host skill catalog.
//!
//! A `StaticSkillCatalog` loads skill packages from bundled in-memory sources
//! and optionally from host-owned directory roots. Loading is descriptor-
//! relative and no-follow on Unix. The catalog is immutable after construction;
//! invocation produces an immutable `SkillUse` with pinned body bytes.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ========================================================================
// Validated identity newtypes
// ========================================================================

/// Validated source authority identifier.
///
/// Rules: non-empty, ≤ 64 bytes, lowercase ASCII alphanumeric / `-` / `_`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SkillAuthorityId(String);

impl SkillAuthorityId {
    /// Parse and validate a source authority ID.
    ///
    /// Rules: non-empty, ≤ 64 bytes, lowercase ASCII alphanumeric / `-` / `_` / `.`.
    pub fn parse(s: &str) -> Result<Self, SkillError> {
        if s.is_empty() || s.len() > 64 {
            return Err(SkillError::InvalidAuthorityId(s.to_string()));
        }
        if !s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_' || b == b'.'
        }) {
            return Err(SkillError::InvalidAuthorityId(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }

    /// Returns the authority ID string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated package identifier.
///
/// Rules: non-empty, ≤ 64 bytes, lowercase ASCII alphanumeric / `-` / `_`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SkillPackageId(String);

impl SkillPackageId {
    /// Parse and validate a package ID.
    pub fn parse(s: &str) -> Result<Self, SkillError> {
        if s.is_empty() || s.len() > 64 {
            return Err(SkillError::InvalidPackageId(s.to_string()));
        }
        if !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(SkillError::InvalidPackageId(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }

    /// Returns the package ID string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated resource identifier.
///
/// Rules: non-empty, ≤ 64 bytes, lowercase ASCII alphanumeric / `-` / `_` / `.`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SkillResourceId(String);

impl SkillResourceId {
    /// Parse and validate a resource ID.
    pub fn parse(s: &str) -> Result<Self, SkillError> {
        if s.is_empty() || s.len() > 64 {
            return Err(SkillError::InvalidResourceId(s.to_string()));
        }
        if !s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_' || b == b'.'
        }) {
            return Err(SkillError::InvalidResourceId(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }

    /// Returns the resource ID string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ========================================================================
// Manifest V1 DTO (private, strict)
// ========================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillManifestV1 {
    schema_version: u32,
    package_id: String,
    name: String,
    description: String,
    #[serde(default)]
    declared_version: Option<String>,
    main: String,
}

impl SkillManifestV1 {
    fn validate(&self) -> Result<(), SkillError> {
        if self.schema_version != 1 {
            return Err(SkillError::UnsupportedSchema(self.schema_version));
        }

        SkillPackageId::parse(&self.package_id)?;

        if self.name.is_empty() || self.name.chars().count() > 64 {
            return Err(SkillError::InvalidManifest(format!(
                "name must be 1-64 chars, got {}",
                self.name.chars().count()
            )));
        }

        if self.description.is_empty() || self.description.len() > 512 {
            return Err(SkillError::InvalidManifest(format!(
                "description must be 1-512 bytes, got {}",
                self.description.len()
            )));
        }

        if let Some(ref v) = self.declared_version {
            if v.len() > 64 {
                return Err(SkillError::InvalidManifest(format!(
                    "declared_version must be ≤ 64 bytes, got {}",
                    v.len()
                )));
            }
        }

        if self.main != "SKILL.md" {
            return Err(SkillError::InvalidManifest(format!(
                "main must be \"SKILL.md\", got \"{}\"",
                self.main
            )));
        }

        Ok(())
    }
}

// ========================================================================
// Catalog limits
// ========================================================================

#[derive(Debug, Clone)]
pub struct SkillCatalogLimits {
    pub max_entries: usize,
    pub max_manifest_bytes: usize,
    pub max_body_bytes: usize,
    pub max_metadata_bytes: usize,
}

impl SkillCatalogLimits {
    pub fn v1() -> Self {
        Self {
            max_entries: 1_000,
            max_manifest_bytes: 4 * 1024,
            max_body_bytes: 16 * 1024,
            max_metadata_bytes: 128 * 1024,
        }
    }
}

// ========================================================================
// Skill sources
// ========================================================================

pub enum SkillSource<'a> {
    Bundled(Vec<(&'a str, Vec<(&'a str, &'a [u8])>)>),
    HostRoot(HostSkillRoot),
}

// ========================================================================
// HostSkillRoot
// ========================================================================

#[cfg(unix)]
pub struct HostSkillRoot {
    root_fd: std::os::fd::OwnedFd,
    root_path: String,
}

#[cfg(unix)]
impl HostSkillRoot {
    pub fn open(root_path: &str) -> Result<Self, SkillError> {
        use rustix::fs::{open, openat, Mode, OFlags};

        for component in root_path.split('/') {
            if component == ".." {
                return Err(SkillError::InvalidComponent(root_path.to_string()));
            }
        }

        let normalized = root_path.trim_end_matches('/');
        if normalized.is_empty() {
            let fd = open(
                "/",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|e| SkillError::Io(e.to_string()))?;
            return Ok(Self {
                root_fd: fd,
                root_path: "/".to_string(),
            });
        }

        let (start_fd, components): (std::os::fd::OwnedFd, Vec<&str>) =
            if normalized.starts_with('/') {
                let fd = open(
                    "/",
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                    Mode::empty(),
                )
                .map_err(|e| SkillError::Io(e.to_string()))?;
                let components: Vec<&str> = normalized[1..]
                    .split('/')
                    .filter(|c| !c.is_empty() && *c != ".")
                    .collect();
                (fd, components)
            } else {
                let cwd = open(".", OFlags::RDONLY | OFlags::DIRECTORY, Mode::empty())
                    .map_err(|e| SkillError::Io(e.to_string()))?;
                let components: Vec<&str> = normalized
                    .split('/')
                    .filter(|c| !c.is_empty() && *c != ".")
                    .collect();
                (cwd, components)
            };

        let mut current_fd = start_fd;
        for component in &components {
            let new_fd = openat(
                &current_fd,
                *component,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|e| SkillError::Io(format!("openat({component}): {e}")))?;
            current_fd = new_fd;
        }

        Ok(Self {
            root_fd: current_fd,
            root_path: normalized.to_string(),
        })
    }
}

#[cfg(not(unix))]
pub struct HostSkillRoot {
    _private: (),
}

#[cfg(not(unix))]
impl HostSkillRoot {
    pub fn open(_root_path: &str) -> Result<Self, SkillError> {
        Err(SkillError::UnsupportedPlatform)
    }
}

// ========================================================================
// Catalog entry
// ========================================================================

#[derive(Debug, Clone)]
struct CatalogEntry {
    source_authority: SkillAuthorityId,
    source_tier: u8,
    source_index: usize,
    package_id: SkillPackageId,
    name: String,
    description: String,
    declared_version: Option<String>,
    body: Arc<str>,
    digest: String,
    source_path: String,
}

// ========================================================================
// StaticSkillCatalog
// ========================================================================

pub struct StaticSkillCatalog {
    entries: Vec<CatalogEntry>,
}

impl StaticSkillCatalog {
    pub fn build(sources: Vec<SkillSource<'_>>, limits: &SkillCatalogLimits) -> CatalogBuildReport {
        let mut entries = Vec::new();
        let mut omitted_count: u32 = 0;
        let mut diagnostics = Vec::new();

        for (source_idx, source) in sources.into_iter().enumerate() {
            match source {
                SkillSource::Bundled(bundles) => {
                    for (bundle_idx, (dir_name, files)) in bundles.into_iter().enumerate() {
                        match load_bundled_package(dir_name, files, limits, source_idx, bundle_idx)
                        {
                            Ok(entry) => {
                                if entries.len() >= limits.max_entries {
                                    omitted_count += 1;
                                } else {
                                    entries.push(entry);
                                }
                            }
                            Err(e) => {
                                diagnostics.push(CatalogDiagnostic::PackageLoadError {
                                    source_index: source_idx,
                                    package_hint: dir_name.to_string(),
                                    error: e.to_string(),
                                });
                            }
                        }
                    }
                }
                SkillSource::HostRoot(root) => {
                    #[cfg(unix)]
                    {
                        match load_host_packages(&root, limits, source_idx) {
                            Ok(host_entries) => {
                                for entry in host_entries {
                                    if entries.len() >= limits.max_entries {
                                        omitted_count += 1;
                                    } else {
                                        entries.push(entry);
                                    }
                                }
                            }
                            Err(e) => {
                                diagnostics.push(CatalogDiagnostic::PackageLoadError {
                                    source_index: source_idx,
                                    package_hint: root.root_path.clone(),
                                    error: e.to_string(),
                                });
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        diagnostics.push(CatalogDiagnostic::PackageLoadError {
                            source_index: source_idx,
                            package_hint: "host-root".to_string(),
                            error: SkillError::UnsupportedPlatform.to_string(),
                        });
                    }
                }
            }
        }

        entries.sort_by(|a, b| {
            a.source_tier
                .cmp(&b.source_tier)
                .then_with(|| a.source_index.cmp(&b.source_index))
                .then_with(|| a.package_id.cmp(&b.package_id))
        });

        let pre_truncate = entries.len();
        if pre_truncate > limits.max_entries {
            omitted_count += (pre_truncate - limits.max_entries) as u32;
            entries.truncate(limits.max_entries);
        }

        let mut total_metadata: usize = 0;
        for entry in &entries {
            total_metadata += entry.name.len()
                + entry.description.len()
                + entry.declared_version.as_ref().map_or(0, |v| v.len())
                + entry.digest.len();
        }

        if total_metadata > limits.max_metadata_bytes {
            diagnostics.push(CatalogDiagnostic::MetadataLimitReached {
                total_bytes: total_metadata,
            });
        }

        let mut seen = HashSet::new();
        let mut deduped = Vec::new();
        for entry in entries {
            if seen.insert(entry.package_id.clone()) {
                deduped.push(entry);
            } else {
                diagnostics.push(CatalogDiagnostic::DuplicateOmitted {
                    source_index: entry.source_index,
                    package_id: entry.package_id.as_str().to_string(),
                });
                omitted_count += 1;
            }
        }

        CatalogBuildReport {
            catalog: StaticSkillCatalog { entries: deduped },
            omitted_count,
            diagnostics,
        }
    }

    pub fn invoke(
        &self,
        request: &SkillInvocationRequest,
        resolved_at_unix_ms: i64,
    ) -> Result<SkillUse, SkillError> {
        let _ = &request.source_authority;
        let _ = &request.package_id;

        let entry = self
            .entries
            .iter()
            .find(|e| {
                e.source_authority == request.source_authority && e.package_id == request.package_id
            })
            .ok_or_else(|| SkillError::UnknownPackage {
                authority: request.source_authority.as_str().to_string(),
                package_id: request.package_id.as_str().to_string(),
            })?;

        if let Some(ref expected) = request.expected_digest {
            if *expected != entry.digest {
                return Err(SkillError::DigestMismatch {
                    expected: expected.clone(),
                    actual: entry.digest.clone(),
                });
            }
        }

        Ok(SkillUse {
            source_authority: entry.source_authority.clone(),
            package_id: entry.package_id.clone(),
            resource_id: SkillResourceId::parse("sk").unwrap(),
            digest: entry.digest.clone(),
            declared_version: entry.declared_version.clone(),
            invocation_kind: request.invocation_kind,
            resolved_at_unix_ms,
            body: entry.body.clone(),
        })
    }
}

// ========================================================================
// Catalog build report
// ========================================================================

pub struct CatalogBuildReport {
    pub catalog: StaticSkillCatalog,
    pub omitted_count: u32,
    pub diagnostics: Vec<CatalogDiagnostic>,
}

// ========================================================================
// Skill invocation
// ========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillInvocationKind {
    HostExplicit,
}

pub struct SkillInvocationRequest {
    pub source_authority: SkillAuthorityId,
    pub package_id: SkillPackageId,
    pub expected_digest: Option<String>,
    pub invocation_kind: SkillInvocationKind,
}

// ========================================================================
// SkillUse (immutable, redacted Debug)
// ========================================================================

#[derive(Clone, PartialEq, Eq)]
pub struct SkillUse {
    source_authority: SkillAuthorityId,
    package_id: SkillPackageId,
    resource_id: SkillResourceId,
    digest: String,
    declared_version: Option<String>,
    invocation_kind: SkillInvocationKind,
    resolved_at_unix_ms: i64,
    body: Arc<str>,
}

impl SkillUse {
    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn source_authority(&self) -> &SkillAuthorityId {
        &self.source_authority
    }

    pub fn package_id(&self) -> &SkillPackageId {
        &self.package_id
    }

    pub fn declared_version(&self) -> Option<&str> {
        self.declared_version.as_deref()
    }

    pub fn invocation_kind(&self) -> SkillInvocationKind {
        self.invocation_kind
    }

    pub fn receipt(&self) -> SkillUseReceiptV1 {
        SkillUseReceiptV1 {
            schema_version: 1,
            source_authority: self.source_authority.as_str().to_string(),
            package_id: self.package_id.as_str().to_string(),
            main_resource_id: self.resource_id.as_str().to_string(),
            package_digest: self.digest.clone(),
            declared_version: self.declared_version.clone(),
            invocation_kind: self.invocation_kind,
            resolved_at_unix_ms: self.resolved_at_unix_ms,
        }
    }
}

impl fmt::Debug for SkillUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SkillUse")
            .field("source_authority", &self.source_authority)
            .field("package_id", &self.package_id)
            .field("resource_id", &self.resource_id)
            .field("digest", &self.digest)
            .field("declared_version", &self.declared_version)
            .field("invocation_kind", &self.invocation_kind)
            .field("resolved_at_unix_ms", &self.resolved_at_unix_ms)
            .field("body", &"<redacted>")
            .finish()
    }
}

// ========================================================================
// SkillUseReceiptV1
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillUseReceiptV1 {
    pub schema_version: u32,
    pub source_authority: String,
    pub package_id: String,
    pub main_resource_id: String,
    pub package_digest: String,
    pub declared_version: Option<String>,
    pub invocation_kind: SkillInvocationKind,
    pub resolved_at_unix_ms: i64,
}

// ========================================================================
// Skill errors
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillError {
    #[error("invalid authority ID: {0}")]
    InvalidAuthorityId(String),
    #[error("invalid package ID: {0}")]
    InvalidPackageId(String),
    #[error("invalid resource ID: {0}")]
    InvalidResourceId(String),
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),
    #[error("unsupported schema version: {0}")]
    UnsupportedSchema(u32),
    #[error("invalid path component: {0}")]
    InvalidComponent(String),
    #[error("path traversal rejected: {0}")]
    PathTraversal(String),
    #[error("symlink rejected: {0}")]
    SymlinkRejected(String),
    #[error("special file rejected: {0}")]
    SpecialFileRejected(String),
    #[error("body too large: {0} bytes (limit {1})")]
    BodyTooLarge(usize, usize),
    #[error("manifest too large: {0} bytes (limit {1})")]
    ManifestTooLarge(usize, usize),
    #[error("invalid UTF-8 in {0}")]
    InvalidUtf8(String),
    #[error("unknown package: authority={authority}, package_id={package_id}")]
    UnknownPackage {
        authority: String,
        package_id: String,
    },
    #[error("digest mismatch: expected={expected}, actual={actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("I/O error: {0}")]
    Io(String),
    #[error("host-root loading not supported on this platform")]
    UnsupportedPlatform,
    #[error("duplicate package omitted: {0}")]
    DuplicateOmitted(String),
}

// ========================================================================
// Catalog diagnostics
// ========================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogDiagnostic {
    EntryLimitReached {
        omitted_count: u32,
        max_entries: usize,
    },
    PackageLoadError {
        source_index: usize,
        package_hint: String,
        error: String,
    },
    DuplicateOmitted {
        source_index: usize,
        package_id: String,
    },
    MetadataLimitReached {
        total_bytes: usize,
    },
}

// ========================================================================
// Private helpers: bundled loading
// ========================================================================

fn load_bundled_package(
    dir_name: &str,
    files: Vec<(&str, &[u8])>,
    limits: &SkillCatalogLimits,
    _source_idx: usize,
    bundle_idx: usize,
) -> Result<CatalogEntry, SkillError> {
    validate_single_component(dir_name)?;

    let mut manifest_bytes: Option<&[u8]> = None;
    let mut body_bytes: Option<&[u8]> = None;
    let mut file_count = 0usize;

    for (name, content) in &files {
        file_count += 1;
        match *name {
            "skill.toml" => {
                if manifest_bytes.is_some() {
                    return Err(SkillError::InvalidManifest(
                        "duplicate skill.toml".to_string(),
                    ));
                }
                manifest_bytes = Some(content);
            }
            "SKILL.md" => {
                if body_bytes.is_some() {
                    return Err(SkillError::InvalidManifest(
                        "duplicate SKILL.md".to_string(),
                    ));
                }
                body_bytes = Some(content);
            }
            _ => {
                return Err(SkillError::InvalidManifest(format!(
                    "unexpected file: {name}"
                )));
            }
        }
    }

    if file_count != 2 {
        return Err(SkillError::InvalidManifest(format!(
            "expected exactly 2 files, got {file_count}"
        )));
    }

    let manifest_bytes =
        manifest_bytes.ok_or_else(|| SkillError::InvalidManifest("missing skill.toml".into()))?;
    let body_raw =
        body_bytes.ok_or_else(|| SkillError::InvalidManifest("missing SKILL.md".into()))?;

    if manifest_bytes.len() > limits.max_manifest_bytes {
        return Err(SkillError::ManifestTooLarge(
            manifest_bytes.len(),
            limits.max_manifest_bytes,
        ));
    }

    if body_raw.len() > limits.max_body_bytes {
        return Err(SkillError::BodyTooLarge(
            body_raw.len(),
            limits.max_body_bytes,
        ));
    }

    let manifest_str = std::str::from_utf8(manifest_bytes)
        .map_err(|_| SkillError::InvalidUtf8("skill.toml".into()))?;
    let manifest: SkillManifestV1 =
        toml::from_str(manifest_str).map_err(|e| SkillError::InvalidManifest(e.to_string()))?;
    manifest.validate()?;

    let body_str =
        std::str::from_utf8(body_raw).map_err(|_| SkillError::InvalidUtf8("SKILL.md".into()))?;

    let digest = compute_package_digest(&manifest, body_raw);
    let package_id = SkillPackageId::parse(&manifest.package_id)?;

    Ok(CatalogEntry {
        source_authority: SkillAuthorityId::parse("rollshot.bundled").unwrap(),
        source_tier: 0,
        source_index: bundle_idx,
        package_id,
        name: manifest.name,
        description: manifest.description,
        declared_version: manifest.declared_version,
        body: Arc::from(body_str.to_owned()),
        digest,
        source_path: format!("bundled:{dir_name}"),
    })
}

// ========================================================================
// Private helpers: host loading (Unix)
// ========================================================================

#[cfg(unix)]
fn load_host_packages(
    root: &HostSkillRoot,
    limits: &SkillCatalogLimits,
    source_idx: usize,
) -> Result<Vec<CatalogEntry>, SkillError> {
    use rustix::fs::{openat, FileType, Mode, OFlags};

    let mut entries = Vec::new();

    let dir = std::fs::read_dir(&root.root_path)
        .map_err(|e| SkillError::Io(format!("read_dir({}): {e}", root.root_path)))?;

    let mut package_names: Vec<String> = Vec::new();
    for entry in dir {
        let entry = entry.map_err(|e| SkillError::Io(e.to_string()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        package_names.push(name);
    }

    package_names.sort();

    for pkg_name in &package_names {
        validate_single_component(pkg_name)?;

        let pkg_fd = match openat(
            &root.root_fd,
            pkg_name.as_str(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(e) => {
                return Err(SkillError::Io(format!("openat({pkg_name}): {e}")));
            }
        };

        let manifest_fd = match openat(
            &pkg_fd,
            "skill.toml",
            OFlags::RDONLY | OFlags::NOFOLLOW,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(_) => continue,
        };

        let manifest_stat =
            rustix::fs::fstat(&manifest_fd).map_err(|e| SkillError::Io(e.to_string()))?;
        if FileType::from_raw_mode(manifest_stat.st_mode) != FileType::RegularFile {
            return Err(SkillError::SpecialFileRejected(format!(
                "{pkg_name}/skill.toml"
            )));
        }

        let manifest_bytes =
            read_with_ceiling(&manifest_fd, limits.max_manifest_bytes + 1, "skill.toml")?;
        if manifest_bytes.len() > limits.max_manifest_bytes {
            return Err(SkillError::ManifestTooLarge(
                manifest_bytes.len(),
                limits.max_manifest_bytes,
            ));
        }

        let manifest_str = std::str::from_utf8(&manifest_bytes)
            .map_err(|_| SkillError::InvalidUtf8("skill.toml".into()))?;
        let manifest: SkillManifestV1 =
            toml::from_str(manifest_str).map_err(|e| SkillError::InvalidManifest(e.to_string()))?;
        manifest.validate()?;

        let body_fd = match openat(
            &pkg_fd,
            "SKILL.md",
            OFlags::RDONLY | OFlags::NOFOLLOW,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(_) => continue,
        };

        let body_stat = rustix::fs::fstat(&body_fd).map_err(|e| SkillError::Io(e.to_string()))?;
        if FileType::from_raw_mode(body_stat.st_mode) != FileType::RegularFile {
            return Err(SkillError::SpecialFileRejected(format!(
                "{pkg_name}/SKILL.md"
            )));
        }

        let body_bytes = read_with_ceiling(&body_fd, limits.max_body_bytes + 1, "SKILL.md")?;
        if body_bytes.len() > limits.max_body_bytes {
            return Err(SkillError::BodyTooLarge(
                body_bytes.len(),
                limits.max_body_bytes,
            ));
        }

        let body_str = std::str::from_utf8(&body_bytes)
            .map_err(|_| SkillError::InvalidUtf8("SKILL.md".into()))?;

        let digest = compute_package_digest(&manifest, &body_bytes);
        let package_id = SkillPackageId::parse(&manifest.package_id)?;

        entries.push(CatalogEntry {
            source_authority: SkillAuthorityId::parse("rollshot.host").unwrap(),
            source_tier: 1,
            source_index: source_idx,
            package_id,
            name: manifest.name,
            description: manifest.description,
            declared_version: manifest.declared_version,
            body: Arc::from(body_str.to_owned()),
            digest,
            source_path: format!("{}/{pkg_name}", root.root_path),
        });
    }

    Ok(entries)
}

#[cfg(unix)]
fn read_with_ceiling(
    fd: &impl std::os::fd::AsFd,
    ceiling: usize,
    label: &str,
) -> Result<Vec<u8>, SkillError> {
    use rustix::io::read;

    let mut buf = vec![0u8; ceiling];
    let mut total = 0usize;

    loop {
        if total >= ceiling {
            return Err(SkillError::Io(format!("{label}: exceeds {ceiling} bytes")));
        }
        match read(fd, &mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(SkillError::Io(format!("{label}: {e}"))),
        }
    }

    buf.truncate(total);
    Ok(buf)
}

// ========================================================================
// Private helpers: validation and digest
// ========================================================================

fn validate_single_component(component: &str) -> Result<(), SkillError> {
    if component.is_empty() {
        return Err(SkillError::InvalidComponent(component.to_string()));
    }
    if component == "." || component == ".." {
        return Err(SkillError::InvalidComponent(component.to_string()));
    }
    if component.contains('/') || component.contains('\\') {
        return Err(SkillError::InvalidComponent(component.to_string()));
    }
    if component.starts_with('.') {
        return Err(SkillError::InvalidComponent(component.to_string()));
    }
    Ok(())
}

fn compute_package_digest(manifest: &SkillManifestV1, body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rollshot-skill-v1\0");
    hasher.update(manifest.schema_version.to_be_bytes());
    hasher.update(manifest.package_id.as_bytes());
    hasher.update(manifest.name.as_bytes());
    hasher.update(manifest.description.as_bytes());
    if let Some(ref v) = manifest.declared_version {
        hasher.update(v.as_bytes());
    }
    hasher.update((body.len() as u64).to_be_bytes());
    hasher.update(body);
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ========================================================================
// Tests
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bundled_source(
        packages: Vec<(String, Vec<(String, Vec<u8>)>)>,
    ) -> Vec<(String, Vec<(String, Vec<u8>)>)> {
        packages
    }

    fn resolve_from_catalog(
        packages: Vec<(String, Vec<(String, Vec<u8>)>)>,
        package_id: &str,
    ) -> SkillUse {
        let limits = SkillCatalogLimits::v1();
        let owned = packages;
        let sources: Vec<SkillSource<'_>> = vec![SkillSource::Bundled(
            owned
                .iter()
                .map(|(dir, files)| {
                    (
                        dir.as_str(),
                        files
                            .iter()
                            .map(|(name, content)| (name.as_str(), content.as_slice()))
                            .collect(),
                    )
                })
                .collect(),
        )];
        let report = StaticSkillCatalog::build(sources, &limits);
        assert_eq!(report.omitted_count, 0, "unexpected omission");
        report
            .catalog
            .invoke(
                &SkillInvocationRequest {
                    source_authority: SkillAuthorityId::parse("rollshot.bundled").unwrap(),
                    package_id: SkillPackageId::parse(package_id).unwrap(),
                    expected_digest: None,
                    invocation_kind: SkillInvocationKind::HostExplicit,
                },
                100,
            )
            .unwrap()
    }

    fn single_bundled_pkg(
        package_id: &str,
        version: &str,
        body: &str,
    ) -> Vec<(String, Vec<(String, Vec<u8>)>)> {
        let manifest = format!(
            r#"schema_version = 1
package_id = "{package_id}"
name = "Test Skill"
description = "A test skill for validation."
declared_version = "{version}"
main = "SKILL.md"
"#
        );
        vec![(
            package_id.to_string(),
            vec![
                ("skill.toml".to_string(), manifest.into_bytes()),
                ("SKILL.md".to_string(), body.as_bytes().to_vec()),
            ],
        )]
    }

    fn build_catalog(packages: Vec<(String, Vec<(String, Vec<u8>)>)>) -> CatalogBuildReport {
        let limits = SkillCatalogLimits::v1();
        let sources: Vec<SkillSource<'_>> = vec![SkillSource::Bundled(
            packages
                .iter()
                .map(|(dir, files)| {
                    (
                        dir.as_str(),
                        files
                            .iter()
                            .map(|(name, content)| (name.as_str(), content.as_slice()))
                            .collect(),
                    )
                })
                .collect(),
        )];
        StaticSkillCatalog::build(sources, &limits)
    }

    // ---- Step 1: Parser and digest tests ----

    #[test]
    fn valid_manifest_parses_and_digests() {
        let pkg = single_bundled_pkg("smart-redaction", "1", "# Skill body");
        let report = build_catalog(pkg);
        assert_eq!(report.omitted_count, 0);
        assert!(report.diagnostics.is_empty());
        assert_eq!(report.catalog.entries.len(), 1);
    }

    #[test]
    fn unknown_manifest_field_rejects() {
        let manifest = br#"schema_version = 1
package_id = "smart-redaction"
name = "Test"
description = "desc"
main = "SKILL.md"
unknown_field = true"#;
        let pkg = vec![(
            "smart-redaction".to_string(),
            vec![
                ("skill.toml".to_string(), manifest.to_vec()),
                ("SKILL.md".to_string(), b"body".to_vec()),
            ],
        )];
        let report = build_catalog(pkg);
        assert!(report.catalog.entries.is_empty());
        assert_eq!(report.diagnostics.len(), 1);
    }

    #[test]
    fn unsupported_schema_rejects() {
        let manifest = br#"schema_version = 99
package_id = "smart-redaction"
name = "Test"
description = "desc"
main = "SKILL.md""#;
        let pkg = vec![(
            "smart-redaction".to_string(),
            vec![
                ("skill.toml".to_string(), manifest.to_vec()),
                ("SKILL.md".to_string(), b"body".to_vec()),
            ],
        )];
        let report = build_catalog(pkg);
        assert!(report.catalog.entries.is_empty());
    }

    #[test]
    fn invalid_package_id_rejects() {
        let manifest = br#"schema_version = 1
package_id = "Not-Valid!"
name = "Test"
description = "desc"
main = "SKILL.md""#;
        let pkg = vec![(
            "pkg".to_string(),
            vec![
                ("skill.toml".to_string(), manifest.to_vec()),
                ("SKILL.md".to_string(), b"body".to_vec()),
            ],
        )];
        let report = build_catalog(pkg);
        assert!(report.catalog.entries.is_empty());
    }

    #[test]
    fn missing_description_rejects() {
        let manifest = br#"schema_version = 1
package_id = "smart-redaction"
name = "Test"
description = ""
main = "SKILL.md""#;
        let pkg = vec![(
            "smart-redaction".to_string(),
            vec![
                ("skill.toml".to_string(), manifest.to_vec()),
                ("SKILL.md".to_string(), b"body".to_vec()),
            ],
        )];
        let report = build_catalog(pkg);
        assert!(report.catalog.entries.is_empty());
    }

    #[test]
    fn oversize_description_rejects() {
        let manifest = format!(
            r#"schema_version = 1
package_id = "smart-redaction"
name = "Test"
description = "{}"
main = "SKILL.md""#,
            "x".repeat(513)
        );
        let pkg = vec![(
            "smart-redaction".to_string(),
            vec![
                ("skill.toml".to_string(), manifest.into_bytes()),
                ("SKILL.md".to_string(), b"body".to_vec()),
            ],
        )];
        let report = build_catalog(pkg);
        assert!(report.catalog.entries.is_empty());
    }

    #[test]
    fn non_skill_md_main_rejects() {
        let manifest = br#"schema_version = 1
package_id = "smart-redaction"
name = "Test"
description = "desc"
main = "README.md""#;
        let pkg = vec![(
            "smart-redaction".to_string(),
            vec![
                ("skill.toml".to_string(), manifest.to_vec()),
                ("SKILL.md".to_string(), b"body".to_vec()),
            ],
        )];
        let report = build_catalog(pkg);
        assert!(report.catalog.entries.is_empty());
    }

    #[test]
    fn invalid_utf8_rejects() {
        let manifest = br#"schema_version = 1
package_id = "smart-redaction"
name = "Test"
description = "desc"
main = "SKILL.md""#;
        let invalid_body: Vec<u8> = vec![0xFF, 0xFE, 0xFD];
        let pkg = vec![(
            "smart-redaction".to_string(),
            vec![
                ("skill.toml".to_string(), manifest.to_vec()),
                ("SKILL.md".to_string(), invalid_body),
            ],
        )];
        let report = build_catalog(pkg);
        assert!(report.catalog.entries.is_empty());
    }

    #[test]
    fn oversize_description_at_513_bytes_rejects() {
        let manifest = format!(
            r#"schema_version = 1
package_id = "smart-redaction"
name = "Test"
description = "{}"
main = "SKILL.md""#,
            "x".repeat(513)
        );
        let pkg = vec![(
            "smart-redaction".to_string(),
            vec![
                ("skill.toml".to_string(), manifest.into_bytes()),
                ("SKILL.md".to_string(), b"body".to_vec()),
            ],
        )];
        let report = build_catalog(pkg);
        assert!(report.catalog.entries.is_empty());
    }

    #[test]
    fn exactly_at_body_limit_succeeds() {
        let body = "x".repeat(16 * 1024);
        let pkg = single_bundled_pkg("smart-redaction", "1", &body);
        let report = build_catalog(pkg);
        assert_eq!(report.omitted_count, 0);
        assert_eq!(report.catalog.entries.len(), 1);
    }

    #[test]
    fn over_body_limit_rejects() {
        let body = "x".repeat(16 * 1024 + 1);
        let pkg = single_bundled_pkg("smart-redaction", "1", &body);
        let report = build_catalog(pkg);
        assert!(report.catalog.entries.is_empty());
    }

    #[test]
    fn third_unexpected_package_file_rejects() {
        let manifest = br#"schema_version = 1
package_id = "smart-redaction"
name = "Test"
description = "desc"
main = "SKILL.md""#;
        let pkg = vec![(
            "smart-redaction".to_string(),
            vec![
                ("skill.toml".to_string(), manifest.to_vec()),
                ("SKILL.md".to_string(), b"body".to_vec()),
                ("extra.txt".to_string(), b"extra".to_vec()),
            ],
        )];
        let report = build_catalog(pkg);
        assert!(report.catalog.entries.is_empty());
    }

    #[test]
    fn malformed_optional_package_emits_diagnostic_and_continues() {
        let bad_manifest = br#"schema_version = 1
package_id = "bad-pkg"
name = "Bad"
description = "desc"
main = "WRONG.md""#;
        let good = single_bundled_pkg("smart-redaction", "1", "good body");
        let bad = vec![(
            "bad-pkg".to_string(),
            vec![
                ("skill.toml".to_string(), bad_manifest.to_vec()),
                ("SKILL.md".to_string(), b"body".to_vec()),
            ],
        )];
        let mut all = good;
        all.extend(bad);

        let limits = SkillCatalogLimits::v1();
        let sources: Vec<SkillSource<'_>> = vec![SkillSource::Bundled(
            all.iter()
                .map(|(dir, files)| {
                    (
                        dir.as_str(),
                        files
                            .iter()
                            .map(|(name, content)| (name.as_str(), content.as_slice()))
                            .collect(),
                    )
                })
                .collect(),
        )];
        let report = StaticSkillCatalog::build(sources, &limits);

        assert_eq!(report.catalog.entries.len(), 1);
        assert_eq!(
            report.catalog.entries[0].package_id.as_str(),
            "smart-redaction"
        );
        assert_eq!(report.diagnostics.len(), 1);
    }

    #[test]
    fn manifest_and_body_changes_change_domain_separated_digest() {
        let base = single_bundled_pkg("smart-redaction", "1", "body");
        let body_changed = single_bundled_pkg("smart-redaction", "1", "body changed");
        let version_changed = single_bundled_pkg("smart-redaction", "2", "body");
        assert_ne!(
            resolve_from_catalog(base, "smart-redaction").digest(),
            resolve_from_catalog(body_changed, "smart-redaction").digest()
        );
        assert_ne!(
            resolve_from_catalog(version_changed, "smart-redaction").digest(),
            resolve_from_catalog(
                single_bundled_pkg("smart-redaction", "1", "body"),
                "smart-redaction"
            )
            .digest()
        );
    }

    #[test]
    fn invocation_digest_mismatch_never_substitutes_current_body() {
        let report = build_catalog(single_bundled_pkg("smart-redaction", "1", "current body"));
        let error = report
            .catalog
            .invoke(
                &SkillInvocationRequest {
                    source_authority: SkillAuthorityId::parse("rollshot.bundled").unwrap(),
                    package_id: SkillPackageId::parse("smart-redaction").unwrap(),
                    expected_digest: Some("00".repeat(32)),
                    invocation_kind: SkillInvocationKind::HostExplicit,
                },
                123,
            )
            .unwrap_err();
        assert!(matches!(error, SkillError::DigestMismatch { .. }));
    }

    // ---- Step 2: Unix host-root adversarial tests ----

    #[cfg(unix)]
    mod unix_adversarial {
        use super::*;
        use std::os::unix::fs::symlink;
        use tempfile::TempDir;

        fn make_skill_dir(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
            let pkg_dir = dir.join(name);
            std::fs::create_dir_all(&pkg_dir).unwrap();
            let manifest = format!(
                r#"schema_version = 1
package_id = "{name}"
name = "Host Skill"
description = "A host skill."
main = "SKILL.md"
"#
            );
            std::fs::write(pkg_dir.join("skill.toml"), manifest.as_bytes()).unwrap();
            std::fs::write(pkg_dir.join("SKILL.md"), body.as_bytes()).unwrap();
            pkg_dir
        }

        #[test]
        fn package_directory_symlink_rejects() {
            let tmp = TempDir::new().unwrap();
            let real = make_skill_dir(tmp.path(), "real-pkg", "body");
            let link = tmp.path().join("link-pkg");
            symlink(&real, &link).unwrap();

            let root = HostSkillRoot::open(tmp.path().to_str().unwrap()).unwrap();
            let limits = SkillCatalogLimits::v1();
            let report = StaticSkillCatalog::build(vec![SkillSource::HostRoot(root)], &limits);
            assert!(
                report.catalog.entries.is_empty() || !report.diagnostics.is_empty(),
                "symlink directory should be rejected"
            );
        }

        #[test]
        fn skill_toml_symlink_rejects() {
            let tmp = TempDir::new().unwrap();
            let pkg_dir = tmp.path().join("pkg");
            std::fs::create_dir(&pkg_dir).unwrap();

            let real_manifest = tmp.path().join("real.toml");
            std::fs::write(
                &real_manifest,
                br#"schema_version = 1
package_id = "pkg"
name = "Host"
description = "desc"
main = "SKILL.md"
"#,
            )
            .unwrap();
            symlink(&real_manifest, pkg_dir.join("skill.toml")).unwrap();
            std::fs::write(pkg_dir.join("SKILL.md"), b"body").unwrap();

            let root = HostSkillRoot::open(tmp.path().to_str().unwrap()).unwrap();
            let limits = SkillCatalogLimits::v1();
            let report = StaticSkillCatalog::build(vec![SkillSource::HostRoot(root)], &limits);
            assert!(report.catalog.entries.is_empty());
        }

        #[test]
        fn skill_md_symlink_rejects() {
            let tmp = TempDir::new().unwrap();
            let pkg_dir = tmp.path().join("pkg");
            std::fs::create_dir(&pkg_dir).unwrap();

            let manifest = br#"schema_version = 1
package_id = "pkg"
name = "Host"
description = "desc"
main = "SKILL.md"
"#;
            std::fs::write(pkg_dir.join("skill.toml"), manifest).unwrap();
            let real_body = tmp.path().join("real.md");
            std::fs::write(&real_body, b"body").unwrap();
            symlink(&real_body, pkg_dir.join("SKILL.md")).unwrap();

            let root = HostSkillRoot::open(tmp.path().to_str().unwrap()).unwrap();
            let limits = SkillCatalogLimits::v1();
            let report = StaticSkillCatalog::build(vec![SkillSource::HostRoot(root)], &limits);
            assert!(report.catalog.entries.is_empty());
        }

        #[test]
        fn absolute_package_component_rejects() {
            assert!(validate_single_component("/absolute").is_err());
        }

        #[test]
        fn slash_in_component_rejects() {
            assert!(validate_single_component("a/b").is_err());
        }

        #[test]
        fn backslash_in_component_rejects() {
            assert!(validate_single_component("a\\b").is_err());
        }

        #[test]
        fn dot_component_rejects() {
            assert!(validate_single_component(".").is_err());
        }

        #[test]
        fn dotdot_component_rejects() {
            assert!(validate_single_component("..").is_err());
        }

        #[test]
        fn oversize_body_rejects_on_host() {
            let tmp = TempDir::new().unwrap();
            let body = "x".repeat(16 * 1024 + 1);
            make_skill_dir(tmp.path(), "big-pkg", &body);

            let root = HostSkillRoot::open(tmp.path().to_str().unwrap()).unwrap();
            let limits = SkillCatalogLimits::v1();
            let report = StaticSkillCatalog::build(vec![SkillSource::HostRoot(root)], &limits);
            assert!(report.catalog.entries.is_empty());
        }

        #[test]
        fn immutable_snapshot_after_backing_file_replacement() {
            let tmp = TempDir::new().unwrap();
            let body_path = make_skill_dir(tmp.path(), "replace-pkg", "original body");

            let root = HostSkillRoot::open(tmp.path().to_str().unwrap()).unwrap();
            let limits = SkillCatalogLimits::v1();
            let report = StaticSkillCatalog::build(vec![SkillSource::HostRoot(root)], &limits);
            assert_eq!(report.omitted_count, 0);
            assert_eq!(report.catalog.entries.len(), 1);

            let request = SkillInvocationRequest {
                source_authority: SkillAuthorityId::parse("rollshot.host").unwrap(),
                package_id: SkillPackageId::parse("replace-pkg").unwrap(),
                expected_digest: None,
                invocation_kind: SkillInvocationKind::HostExplicit,
            };

            let use_before = report.catalog.invoke(&request, 10).unwrap();

            // Replace the backing file
            std::fs::write(body_path.join("SKILL.md"), "replacement body").unwrap();

            let use_after = report
                .catalog
                .invoke(
                    &SkillInvocationRequest {
                        expected_digest: Some(use_before.digest().to_string()),
                        ..request
                    },
                    11,
                )
                .unwrap();

            assert_eq!(use_before.body(), use_after.body());
            assert_eq!(use_before.digest(), use_after.digest());
        }

        #[test]
        fn descriptor_relative_open_works() {
            let tmp = TempDir::new().unwrap();
            make_skill_dir(tmp.path(), "my-skill", "# My Skill\nContent here.");

            let root = HostSkillRoot::open(tmp.path().to_str().unwrap()).unwrap();
            let limits = SkillCatalogLimits::v1();
            let report = StaticSkillCatalog::build(vec![SkillSource::HostRoot(root)], &limits);
            assert_eq!(report.omitted_count, 0);
            assert_eq!(report.catalog.entries.len(), 1);

            let skill_use = report
                .catalog
                .invoke(
                    &SkillInvocationRequest {
                        source_authority: SkillAuthorityId::parse("rollshot.host").unwrap(),
                        package_id: SkillPackageId::parse("my-skill").unwrap(),
                        expected_digest: None,
                        invocation_kind: SkillInvocationKind::HostExplicit,
                    },
                    100,
                )
                .unwrap();
            assert_eq!(skill_use.body(), "# My Skill\nContent here.");
        }
    }

    // ---- Step 3: Precedence and 1,000-entry tests ----

    #[test]
    fn bundled_wins_over_host_root_duplicate() {
        let bundled = single_bundled_pkg("shared-skill", "1", "bundled body");

        let tmp = tempfile::TempDir::new().unwrap();
        let manifest = br#"schema_version = 1
package_id = "shared-skill"
name = "Host Version"
description = "Host copy."
main = "SKILL.md"
"#;
        let pkg_dir = tmp.path().join("shared-skill");
        std::fs::create_dir(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join("skill.toml"), manifest).unwrap();
        std::fs::write(pkg_dir.join("SKILL.md"), b"host body").unwrap();

        let root = HostSkillRoot::open(tmp.path().to_str().unwrap()).unwrap();
        let limits = SkillCatalogLimits::v1();
        let sources: Vec<SkillSource<'_>> = vec![
            SkillSource::Bundled(
                bundled
                    .iter()
                    .map(|(dir, files)| {
                        (
                            dir.as_str(),
                            files
                                .iter()
                                .map(|(name, content)| (name.as_str(), content.as_slice()))
                                .collect(),
                        )
                    })
                    .collect(),
            ),
            SkillSource::HostRoot(root),
        ];
        let report = StaticSkillCatalog::build(sources, &limits);

        assert_eq!(report.catalog.entries.len(), 1);
        assert_eq!(report.catalog.entries[0].body.as_ref(), "bundled body");
        assert_eq!(report.catalog.entries[0].source_tier, 0);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| matches!(d, CatalogDiagnostic::DuplicateOmitted { .. })));
    }

    #[test]
    fn earlier_host_root_wins_when_no_bundled() {
        let tmp1 = tempfile::TempDir::new().unwrap();
        let tmp2 = tempfile::TempDir::new().unwrap();

        let manifest = br#"schema_version = 1
package_id = "shared-skill"
name = "Host Version"
description = "Host copy."
main = "SKILL.md"
"#;

        for tmp in [&tmp1, &tmp2] {
            let pkg_dir = tmp.path().join("shared-skill");
            std::fs::create_dir(&pkg_dir).unwrap();
            std::fs::write(pkg_dir.join("skill.toml"), manifest).unwrap();
        }
        std::fs::write(
            tmp1.path().join("shared-skill").join("SKILL.md"),
            b"first host",
        )
        .unwrap();
        std::fs::write(
            tmp2.path().join("shared-skill").join("SKILL.md"),
            b"second host",
        )
        .unwrap();

        let root1 = HostSkillRoot::open(tmp1.path().to_str().unwrap()).unwrap();
        let root2 = HostSkillRoot::open(tmp2.path().to_str().unwrap()).unwrap();
        let limits = SkillCatalogLimits::v1();
        let report = StaticSkillCatalog::build(
            vec![SkillSource::HostRoot(root1), SkillSource::HostRoot(root2)],
            &limits,
        );

        assert_eq!(report.catalog.entries.len(), 1);
        assert_eq!(report.catalog.entries[0].body.as_ref(), "first host");
    }

    #[test]
    fn no_collision_is_silent() {
        let pkg = single_bundled_pkg("unique-skill", "1", "body");
        let report = build_catalog(pkg);
        assert!(report.diagnostics.is_empty());
        assert_eq!(report.omitted_count, 0);
    }

    #[test]
    fn scale_test_exactly_1000_entries() {
        let limits = SkillCatalogLimits::v1();
        let mut manifests = Vec::new();
        let mut bodies = Vec::new();
        let mut dir_names = Vec::new();

        for i in 0..1000 {
            let dir_name = format!("skill-{i:04}");
            let manifest = format!(
                r#"schema_version = 1
package_id = "skill-{i:04}"
name = "Skill {i}"
description = "Skill number {i}."
main = "SKILL.md"
"#
            );
            let body = format!("body {i}");
            manifests.push(manifest);
            bodies.push(body);
            dir_names.push(dir_name);
        }

        let mut packages: Vec<(&str, Vec<(&str, &[u8])>)> = Vec::new();
        for i in 0..1000 {
            packages.push((
                dir_names[i].as_str(),
                vec![
                    ("skill.toml", manifests[i].as_bytes()),
                    ("SKILL.md", bodies[i].as_bytes()),
                ],
            ));
        }

        let report = StaticSkillCatalog::build(vec![SkillSource::Bundled(packages)], &limits);
        assert_eq!(report.catalog.entries.len(), 1000);
        assert_eq!(report.omitted_count, 0);
    }

    #[test]
    fn scale_test_1001_entries_omits_one() {
        let limits = SkillCatalogLimits::v1();
        let mut manifests = Vec::new();
        let mut bodies = Vec::new();
        let mut dir_names = Vec::new();

        for i in 0..1001 {
            let dir_name = format!("skill-{i:04}");
            let manifest = format!(
                r#"schema_version = 1
package_id = "skill-{i:04}"
name = "Skill {i}"
description = "Skill number {i}."
main = "SKILL.md"
"#
            );
            let body = format!("body {i}");
            manifests.push(manifest);
            bodies.push(body);
            dir_names.push(dir_name);
        }

        let mut packages: Vec<(&str, Vec<(&str, &[u8])>)> = Vec::new();
        for i in 0..1001 {
            packages.push((
                dir_names[i].as_str(),
                vec![
                    ("skill.toml", manifests[i].as_bytes()),
                    ("SKILL.md", bodies[i].as_bytes()),
                ],
            ));
        }

        let report = StaticSkillCatalog::build(vec![SkillSource::Bundled(packages)], &limits);
        assert_eq!(report.catalog.entries.len(), 1000);
        assert_eq!(report.omitted_count, 1);
    }

    #[test]
    fn metadata_bytes_at_or_below_128kib() {
        let limits = SkillCatalogLimits::v1();
        let mut manifests = Vec::new();
        let mut bodies = Vec::new();
        let mut dir_names = Vec::new();

        for i in 0..1000 {
            let dir_name = format!("skill-{i:04}");
            let manifest = format!(
                r#"schema_version = 1
package_id = "skill-{i:04}"
name = "Skill {i}"
description = "Skill number {i}."
main = "SKILL.md"
"#
            );
            let body = format!("body {i}");
            manifests.push(manifest);
            bodies.push(body);
            dir_names.push(dir_name);
        }

        let mut packages: Vec<(&str, Vec<(&str, &[u8])>)> = Vec::new();
        for i in 0..1000 {
            packages.push((
                dir_names[i].as_str(),
                vec![
                    ("skill.toml", manifests[i].as_bytes()),
                    ("SKILL.md", bodies[i].as_bytes()),
                ],
            ));
        }

        let report = StaticSkillCatalog::build(vec![SkillSource::Bundled(packages)], &limits);

        let mut total_meta = 0usize;
        for entry in &report.catalog.entries {
            total_meta += entry.name.len()
                + entry.description.len()
                + entry.declared_version.as_ref().map_or(0, |v| v.len())
                + entry.digest.len();
        }
        assert!(
            total_meta <= 128 * 1024,
            "metadata {total_meta} exceeds 128 KiB"
        );
    }

    #[test]
    fn deterministic_digest_and_order_across_two_builds() {
        let limits = SkillCatalogLimits::v1();
        let mut manifests = Vec::new();
        let mut bodies = Vec::new();
        let mut dir_names = Vec::new();

        for i in 0..100 {
            let dir_name = format!("skill-{i:04}");
            let manifest = format!(
                r#"schema_version = 1
package_id = "skill-{i:04}"
name = "Skill {i}"
description = "Skill number {i}."
main = "SKILL.md"
"#
            );
            let body = format!("body {i}");
            manifests.push(manifest);
            bodies.push(body);
            dir_names.push(dir_name);
        }

        let build = || {
            let mut packages: Vec<(&str, Vec<(&str, &[u8])>)> = Vec::new();
            for i in 0..100 {
                packages.push((
                    dir_names[i].as_str(),
                    vec![
                        ("skill.toml", manifests[i].as_bytes()),
                        ("SKILL.md", bodies[i].as_bytes()),
                    ],
                ));
            }
            StaticSkillCatalog::build(vec![SkillSource::Bundled(packages)], &limits)
        };

        let r1 = build();
        let r2 = build();

        assert_eq!(r1.catalog.entries.len(), r2.catalog.entries.len());
        for (a, b) in r1.catalog.entries.iter().zip(r2.catalog.entries.iter()) {
            assert_eq!(a.digest, b.digest);
            assert_eq!(a.package_id, b.package_id);
            assert_eq!(a.source_tier, b.source_tier);
            assert_eq!(a.source_index, b.source_index);
        }
    }

    // ---- Redacted Debug and receipt tests ----

    #[test]
    fn skill_use_debug_redacts_body() {
        let pkg = single_bundled_pkg("smart-redaction", "1", "sensitive body content");
        let skill_use = resolve_from_catalog(pkg, "smart-redaction");

        let debug = format!("{skill_use:?}");
        assert!(!debug.contains("sensitive body content"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn receipt_excludes_body_and_paths() {
        let pkg = single_bundled_pkg("smart-redaction", "1", "body content");
        let skill_use = resolve_from_catalog(pkg, "smart-redaction");

        let receipt = skill_use.receipt();
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(!json.contains("body content"));
        assert!(!json.contains("/home/"));
        assert_eq!(receipt.schema_version, 1);
        assert_eq!(receipt.source_authority, "rollshot.bundled");
        assert_eq!(receipt.package_id, "smart-redaction");
    }

    // ---- Identity newtype tests ----

    #[test]
    fn valid_newtypes_parse() {
        assert!(SkillAuthorityId::parse("rollshot.bundled").is_ok());
        assert!(SkillAuthorityId::parse("my-host_123").is_ok());
        assert!(SkillPackageId::parse("smart-redaction").is_ok());
        assert!(SkillResourceId::parse("sk").is_ok());
        assert!(SkillResourceId::parse("file.md").is_ok());
    }

    #[test]
    fn invalid_newtypes_reject() {
        assert!(SkillAuthorityId::parse("").is_err());
        assert!(SkillAuthorityId::parse("Not Valid!").is_err());
        assert!(SkillAuthorityId::parse(&"x".repeat(65)).is_err());

        assert!(SkillPackageId::parse("").is_err());
        assert!(SkillPackageId::parse("UPPER").is_err());
        assert!(SkillPackageId::parse("has space").is_err());

        assert!(SkillResourceId::parse("").is_err());
        assert!(SkillResourceId::parse("UPPER").is_err());
        assert!(SkillResourceId::parse("has/slash").is_err());
    }

    // ---- Unknown authority/package ----

    #[test]
    fn unknown_authority_rejects() {
        let pkg = single_bundled_pkg("smart-redaction", "1", "body");
        let report = build_catalog(pkg);
        let err = report
            .catalog
            .invoke(
                &SkillInvocationRequest {
                    source_authority: SkillAuthorityId::parse("unknown.authority").unwrap(),
                    package_id: SkillPackageId::parse("smart-redaction").unwrap(),
                    expected_digest: None,
                    invocation_kind: SkillInvocationKind::HostExplicit,
                },
                100,
            )
            .unwrap_err();
        assert!(matches!(err, SkillError::UnknownPackage { .. }));
    }

    #[test]
    fn unknown_package_rejects() {
        let pkg = single_bundled_pkg("smart-redaction", "1", "body");
        let report = build_catalog(pkg);
        let err = report
            .catalog
            .invoke(
                &SkillInvocationRequest {
                    source_authority: SkillAuthorityId::parse("rollshot.bundled").unwrap(),
                    package_id: SkillPackageId::parse("nonexistent").unwrap(),
                    expected_digest: None,
                    invocation_kind: SkillInvocationKind::HostExplicit,
                },
                100,
            )
            .unwrap_err();
        assert!(matches!(err, SkillError::UnknownPackage { .. }));
    }

    // ---- Catalog limits ----

    #[test]
    fn v1_limits_are_correct() {
        let limits = SkillCatalogLimits::v1();
        assert_eq!(limits.max_entries, 1_000);
        assert_eq!(limits.max_manifest_bytes, 4 * 1024);
        assert_eq!(limits.max_body_bytes, 16 * 1024);
        assert_eq!(limits.max_metadata_bytes, 128 * 1024);
    }
}
