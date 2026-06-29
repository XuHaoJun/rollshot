use rollshot_automation::{CapabilityName, ValidatedAutomation};
use serde::{Deserialize, Serialize};

/// On-disk envelope version for SP5 records, independent of the automation
/// schema versions embedded in the artifact.
pub const STORE_SCHEMA_VERSION: u16 = 1;

/// Opaque, caller-supplied preset identifier (used as a directory name).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PresetId(pub String);

/// Opaque, caller-supplied revision identifier (used as a file name stem).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RevisionId(pub String);

/// How a revision came to exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevisionOrigin {
    AgentRun,
    Import,
    Manual,
}

/// Provenance recorded alongside an immutable revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionProvenance {
    pub origin: RevisionOrigin,
    pub note: Option<String>,
    /// Reserved opaque hook for future SP6 session linkage (no migration later).
    pub source_run_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateHandleMetadata {
    pub alias: String,
    pub handle: String,
    pub display_name: String,
    pub sensitivity_sensitive: bool,
    pub source_agent_suggested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionCapabilityRequirement {
    pub capability: CapabilityName,
    pub alias: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionCapabilityMetadata {
    pub requirements: Vec<RevisionCapabilityRequirement>,
    pub template_handles: Vec<TemplateHandleMetadata>,
}

/// A preset: durable, user-authored configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub store_schema_version: u16,
    pub id: PresetId,
    pub name: String,
    pub original_intent: String,
    /// `None` until the user accepts a first revision.
    pub active_revision_id: Option<RevisionId>,
    pub created_at: String,
    pub updated_at: String,
}

/// An immutable automation revision wrapping a validated artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationRevision {
    pub store_schema_version: u16,
    pub id: RevisionId,
    pub preset_id: PresetId,
    pub parent_id: Option<RevisionId>,
    pub created_at: String,
    pub provenance: RevisionProvenance,
    pub artifact: ValidatedAutomation,
    #[serde(default)]
    pub capabilities: RevisionCapabilityMetadata,
}

/// Lightweight projection for listing presets without loading every artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetSummary {
    pub id: PresetId,
    pub name: String,
    pub active_revision_id: Option<RevisionId>,
    pub updated_at: String,
}

/// Lightweight projection for listing revisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionSummary {
    pub id: RevisionId,
    pub parent_id: Option<RevisionId>,
    pub created_at: String,
}
