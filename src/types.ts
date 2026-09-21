export type BlockKind = "Hypothesis" | "Assumption" | "Method" | "Evidence";
export type BlockStatus =
  | "Idea"
  | "Developing"
  | "Testing"
  | "Supported"
  | "Weakly Supported"
  | "Rejected"
  | "Archived";
export type EdgeType =
  | "Supports"
  | "Contradicts"
  | "Depends on"
  | "Derived from"
  | "Assumes"
  | "Extends"
  | "Tests"
  | "Alternative to"
  | "Related to";
export type AttachmentRelation =
  | "Supports"
  | "Contradicts"
  | "Background"
  | "Method"
  | "Dataset"
  | "Reference"
  | "Other";
export type SnapshotKind = "TenMinute" | "Daily" | "Weekly" | "Manual";

export interface VaultSummary {
  vaultId: string;
  name: string;
  path: string;
  createdAt: string;
  revision: number;
}

export interface BlockSnapshot {
  id: string;
  kind: BlockKind;
  title: string;
  bodyMarkdown: string;
  researchNotesMarkdown: string;
  status: BlockStatus;
  tags: string[];
}

export interface HypothesisBlock extends BlockSnapshot {
  currentVersionId: string;
  rowVersion: number;
  parentBlockId: string | null;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
}

export interface CreateBlockInput {
  title: string;
  bodyMarkdown: string;
  researchNotesMarkdown: string;
  kind: BlockKind;
  status: BlockStatus;
  tags: string[];
  changeReason: string;
}

export interface SaveBlockInput extends CreateBlockInput {
  blockId: string;
  expectedRowVersion: number;
}

export interface RecoveryDraft {
  blockId: string;
  baseRowVersion: number;
  snapshot: BlockSnapshot;
  contentSha256: string;
  updatedAt: string;
}

export interface BlockVersion {
  id: string;
  blockId: string;
  versionIndex: number;
  versionLabel: string;
  snapshot: BlockSnapshot;
  contentSha256: string;
  changeReason: string;
  createdAt: string;
}

export interface ResearchEdge {
  id: string;
  sourceBlockId: string;
  targetBlockId: string;
  edgeType: EdgeType;
  note: string;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
}

export interface GraphPosition {
  blockId: string;
  viewId: string;
  x: number;
  y: number;
}

export interface GraphData {
  blocks: HypothesisBlock[];
  edges: ResearchEdge[];
  positions: GraphPosition[];
}

export interface Attachment {
  id: string;
  blockId: string;
  objectHash: string;
  relationType: AttachmentRelation;
  displayName: string;
  mediaType: string | null;
  byteSize: number;
  locatorJson: Record<string, unknown>;
  createdAt: string;
  deletedAt: string | null;
}

export interface AttachmentPreview {
  mediaType: string;
  dataBase64: string;
}

export interface VariableDefinition {
  id: string;
  blockId: string;
  definition: string;
  formula: string;
  createdAt: string;
}

export interface VariableRecord {
  id: string;
  symbol: string;
  definitions: VariableDefinition[];
  hasConflict: boolean;
}

export interface CitationRecord {
  id: string;
  citationKey: string;
  title: string;
  authors: string;
  year: number | null;
  doi: string | null;
  url: string | null;
  rawCslJson: Record<string, unknown>;
}

export interface BlockCitationRecord {
  linkId: string;
  blockId: string;
  citation: CitationRecord;
  quoteText: string;
  locatorJson: Record<string, unknown>;
  createdAt: string;
}

export interface SearchHit {
  blockId: string;
  title: string;
  excerpt: string;
  rank: number;
}

export interface IntegrityFinding {
  severity: "info" | "warning" | "fatal";
  code: string;
  message: string;
  entityId: string | null;
}

export interface IntegrityReport {
  checkedAt: string;
  vaultRevision: number;
  healthyHypotheses: number;
  findings: IntegrityFinding[];
}

export interface SnapshotRecord {
  id: string;
  kind: SnapshotKind;
  revision: number;
  path: string;
  databaseSha256: string;
  createdAt: string;
  verifiedAt: string;
}

export interface BackupRecord {
  id: string;
  revision: number;
  fileName: string;
  destinationPath: string;
  archiveSha256: string;
  byteSize: number;
  createdAt: string;
  verifiedAt: string;
}

export interface PortableExportRecord {
  destinationPath: string;
  exportedAt: string;
  sourceRevision: number;
  blockCount: number;
  attachmentCount: number;
  citationCount: number;
  manifestSha256: string;
}

export interface AutomaticBackupConfig {
  enabled: boolean;
  destinationDirectory: string;
  intervalHours: number;
  lastSuccessAt: string | null;
  updatedAt: string;
}

export interface AutomaticBackupStatus {
  config: AutomaticBackupConfig;
  hasCredential: boolean;
  due: boolean;
}

export interface ChatGptPluginStatus {
  installed: boolean;
  pluginPath: string;
  marketplacePath: string;
  deepLink: string;
}

export interface ChatGptTunnelStatus {
  configured: boolean;
  running: boolean;
  ready: boolean;
  tunnelId: string | null;
  clientPath: string | null;
  hasCredential: boolean;
  adminUiUrl: string | null;
  mcpUrl: string;
  lastError: string | null;
}
