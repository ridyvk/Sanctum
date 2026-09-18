use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VaultManifest {
    pub format_version: u32,
    pub vault_id: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VaultSummary {
    pub vault_id: String,
    pub name: String,
    pub path: String,
    pub created_at: String,
    pub revision: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlockKind {
    Hypothesis,
    Assumption,
    Method,
    Evidence,
}

impl BlockKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hypothesis => "Hypothesis",
            Self::Assumption => "Assumption",
            Self::Method => "Method",
            Self::Evidence => "Evidence",
        }
    }
}

impl TryFrom<&str> for BlockKind {
    type Error = String;
    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "Hypothesis" => Ok(Self::Hypothesis),
            "Assumption" => Ok(Self::Assumption),
            "Method" => Ok(Self::Method),
            "Evidence" => Ok(Self::Evidence),
            _ => Err(format!("unknown block kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlockStatus {
    Idea,
    Developing,
    Testing,
    Supported,
    #[serde(rename = "Weakly Supported")]
    WeaklySupported,
    Rejected,
    Archived,
}

impl BlockStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idea => "Idea",
            Self::Developing => "Developing",
            Self::Testing => "Testing",
            Self::Supported => "Supported",
            Self::WeaklySupported => "Weakly Supported",
            Self::Rejected => "Rejected",
            Self::Archived => "Archived",
        }
    }
}

impl TryFrom<&str> for BlockStatus {
    type Error = String;
    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "Idea" => Ok(Self::Idea),
            "Developing" => Ok(Self::Developing),
            "Testing" => Ok(Self::Testing),
            "Supported" => Ok(Self::Supported),
            "Weakly Supported" => Ok(Self::WeaklySupported),
            "Rejected" => Ok(Self::Rejected),
            "Archived" => Ok(Self::Archived),
            _ => Err(format!("unknown block status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BlockSnapshot {
    pub id: String,
    pub kind: BlockKind,
    pub title: String,
    pub body_markdown: String,
    pub research_notes_markdown: String,
    pub status: BlockStatus,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HypothesisBlock {
    #[serde(flatten)]
    pub snapshot: BlockSnapshot,
    pub current_version_id: String,
    pub row_version: i64,
    pub parent_block_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBlockInput {
    pub title: String,
    #[serde(default)]
    pub body_markdown: String,
    #[serde(default)]
    pub research_notes_markdown: String,
    pub kind: BlockKind,
    pub status: BlockStatus,
    #[serde(default)]
    pub tags: Vec<String>,
    pub change_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBlockInput {
    pub block_id: String,
    pub expected_row_version: i64,
    pub title: String,
    pub body_markdown: String,
    pub research_notes_markdown: String,
    pub kind: BlockKind,
    pub status: BlockStatus,
    #[serde(default)]
    pub tags: Vec<String>,
    pub change_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BlockVersion {
    pub id: String,
    pub block_id: String,
    pub version_index: i64,
    pub version_label: String,
    pub snapshot: BlockSnapshot,
    pub content_sha256: String,
    pub change_reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryDraft {
    pub block_id: String,
    pub base_row_version: i64,
    pub snapshot: BlockSnapshot,
    pub content_sha256: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchBlockInput {
    pub parent_block_id: String,
    pub expected_parent_row_version: i64,
    pub title: String,
    pub branch_reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EdgeType {
    Supports,
    Contradicts,
    #[serde(rename = "Depends on")]
    DependsOn,
    #[serde(rename = "Derived from")]
    DerivedFrom,
    Assumes,
    Extends,
    Tests,
    #[serde(rename = "Alternative to")]
    AlternativeTo,
    #[serde(rename = "Related to")]
    RelatedTo,
}

impl EdgeType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supports => "Supports",
            Self::Contradicts => "Contradicts",
            Self::DependsOn => "Depends on",
            Self::DerivedFrom => "Derived from",
            Self::Assumes => "Assumes",
            Self::Extends => "Extends",
            Self::Tests => "Tests",
            Self::AlternativeTo => "Alternative to",
            Self::RelatedTo => "Related to",
        }
    }
}

impl TryFrom<&str> for EdgeType {
    type Error = String;
    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "Supports" => Ok(Self::Supports),
            "Contradicts" => Ok(Self::Contradicts),
            "Depends on" => Ok(Self::DependsOn),
            "Derived from" => Ok(Self::DerivedFrom),
            "Assumes" => Ok(Self::Assumes),
            "Extends" => Ok(Self::Extends),
            "Tests" => Ok(Self::Tests),
            "Alternative to" => Ok(Self::AlternativeTo),
            "Related to" => Ok(Self::RelatedTo),
            _ => Err(format!("unknown edge type: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchEdge {
    pub id: String,
    pub source_block_id: String,
    pub target_block_id: String,
    pub edge_type: EdgeType,
    pub note: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEdgeInput {
    pub source_block_id: String,
    pub target_block_id: String,
    pub edge_type: EdgeType,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPosition {
    pub block_id: String,
    pub view_id: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphData {
    pub blocks: Vec<HypothesisBlock>,
    pub edges: Vec<ResearchEdge>,
    pub positions: Vec<GraphPosition>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AttachmentRelation {
    Supports,
    Contradicts,
    Background,
    Method,
    Dataset,
    Reference,
    Other,
}

impl AttachmentRelation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supports => "Supports",
            Self::Contradicts => "Contradicts",
            Self::Background => "Background",
            Self::Method => "Method",
            Self::Dataset => "Dataset",
            Self::Reference => "Reference",
            Self::Other => "Other",
        }
    }
}

impl TryFrom<&str> for AttachmentRelation {
    type Error = String;
    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "Supports" => Ok(Self::Supports),
            "Contradicts" => Ok(Self::Contradicts),
            "Background" => Ok(Self::Background),
            "Method" => Ok(Self::Method),
            "Dataset" => Ok(Self::Dataset),
            "Reference" => Ok(Self::Reference),
            "Other" => Ok(Self::Other),
            _ => Err(format!("unknown attachment relation: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub block_id: String,
    pub object_hash: String,
    pub relation_type: AttachmentRelation,
    pub display_name: String,
    pub media_type: Option<String>,
    pub byte_size: i64,
    pub locator_json: serde_json::Value,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableDefinitionInput {
    pub symbol: String,
    pub definition: String,
    pub block_id: String,
    #[serde(default)]
    pub formula: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableRecord {
    pub id: String,
    pub symbol: String,
    pub definitions: Vec<VariableDefinitionRecord>,
    pub has_conflict: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableDefinitionRecord {
    pub id: String,
    pub block_id: String,
    pub definition: String,
    pub formula: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationInput {
    pub citation_key: String,
    pub title: String,
    #[serde(default)]
    pub authors: String,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub raw_csl_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationRecord {
    pub id: String,
    pub citation_key: String,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub raw_csl_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockCitationRecord {
    pub link_id: String,
    pub block_id: String,
    pub citation: CitationRecord,
    pub quote_text: String,
    pub locator_json: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub block_id: String,
    pub title: String,
    pub excerpt: String,
    pub rank: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IntegritySeverity {
    Info,
    Warning,
    Fatal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityFinding {
    pub severity: IntegritySeverity,
    pub code: String,
    pub message: String,
    pub entity_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    pub checked_at: String,
    pub vault_revision: i64,
    pub healthy_hypotheses: i64,
    pub findings: Vec<IntegrityFinding>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SnapshotKind {
    TenMinute,
    Daily,
    Weekly,
    Manual,
}

impl SnapshotKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TenMinute => "TenMinute",
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Manual => "Manual",
        }
    }
}

impl TryFrom<&str> for SnapshotKind {
    type Error = String;
    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "TenMinute" => Ok(Self::TenMinute),
            "Daily" => Ok(Self::Daily),
            "Weekly" => Ok(Self::Weekly),
            "Manual" => Ok(Self::Manual),
            _ => Err(format!("unknown snapshot kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRecord {
    pub id: String,
    pub kind: SnapshotKind,
    pub revision: i64,
    pub path: String,
    pub database_sha256: String,
    pub created_at: String,
    pub verified_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub id: String,
    pub revision: i64,
    pub file_name: String,
    pub destination_path: String,
    pub archive_sha256: String,
    pub byte_size: i64,
    pub created_at: String,
    pub verified_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PortableExportRecord {
    pub destination_path: String,
    pub exported_at: String,
    pub source_revision: i64,
    pub block_count: usize,
    pub attachment_count: usize,
    pub citation_count: usize,
    pub manifest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomaticBackupConfig {
    pub enabled: bool,
    pub destination_directory: String,
    pub interval_hours: u32,
    pub last_success_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    pub format_version: u32,
    pub snapshot_id: String,
    pub vault_id: String,
    pub schema_version: i64,
    pub revision: i64,
    pub kind: SnapshotKind,
    pub created_at: String,
    pub database_sha256: String,
    pub database_byte_size: i64,
    pub object_hashes: Vec<String>,
}
