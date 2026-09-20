import { invoke } from "@tauri-apps/api/core";
import type {
  Attachment,
  AttachmentPreview,
  AttachmentRelation,
  AutomaticBackupStatus,
  BackupRecord,
  BlockCitationRecord,
  BlockVersion,
  CitationRecord,
  ChatGptPluginStatus,
  CreateBlockInput,
  EdgeType,
  GraphData,
  GraphPosition,
  HypothesisBlock,
  IntegrityReport,
  PortableExportRecord,
  RecoveryDraft,
  SaveBlockInput,
  SearchHit,
  SnapshotKind,
  SnapshotRecord,
  VariableRecord,
  VaultSummary,
} from "./types";

export const isDesktopRuntime = () => typeof window !== "undefined" && Boolean(window.__TAURI_INTERNALS__);

const call = async <T>(command: string, args: Record<string, unknown> = {}): Promise<T> => {
  if (!isDesktopRuntime()) {
    throw new Error("Durable Vault operations require the Sanctum desktop runtime.");
  }
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    if (typeof error === "object" && error && "message" in error) {
      throw new Error(String((error as { message: unknown }).message));
    }
    throw new Error(String(error));
  }
};

export const api = {
  createVault: (path: string, name: string) => call<VaultSummary>("create_vault", { path, name }),
  openVault: (path: string) => call<VaultSummary>("open_vault", { path }),
  closeVault: () => call<void>("close_vault"),
  summary: () => call<VaultSummary>("vault_summary"),
  listBlocks: () => call<HypothesisBlock[]>("list_blocks"),
  listDeletedBlocks: () => call<HypothesisBlock[]>("list_deleted_blocks"),
  createBlock: (input: CreateBlockInput) => call<HypothesisBlock>("create_block", { input }),
  getBlock: (blockId: string) => call<HypothesisBlock>("get_block", { blockId }),
  saveBlock: (input: SaveBlockInput) => call<HypothesisBlock>("save_block", { input }),
  persistRecoveryDraft: (input: SaveBlockInput) => call<RecoveryDraft>("persist_recovery_draft", { input }),
  recoveryDraft: (blockId: string) => call<RecoveryDraft | null>("recovery_draft", { blockId }),
  discardRecoveryDraft: (blockId: string, expectedSha256: string) =>
    call<boolean>("discard_recovery_draft", { blockId, expectedSha256 }),
  versions: (blockId: string) => call<BlockVersion[]>("versions", { blockId }),
  restoreVersion: (blockId: string, versionId: string, reason: string) =>
    call<HypothesisBlock>("restore_block_version", { blockId, versionId, reason }),
  branchBlock: (parentBlockId: string, expectedParentRowVersion: number, title: string, branchReason: string) =>
    call<HypothesisBlock>("branch_block", { input: { parentBlockId, expectedParentRowVersion, title, branchReason } }),
  softDeleteBlock: (blockId: string, expectedRowVersion: number) =>
    call<void>("soft_delete_block", { blockId, expectedRowVersion }),
  restoreDeletedBlock: (blockId: string, expectedRowVersion: number) =>
    call<void>("restore_deleted_block", { blockId, expectedRowVersion }),
  graph: () => call<GraphData>("graph"),
  createEdge: (sourceBlockId: string, targetBlockId: string, edgeType: EdgeType, note = "") =>
    call("create_edge", { input: { sourceBlockId, targetBlockId, edgeType, note } }),
  softDeleteEdge: (edgeId: string) => call<void>("soft_delete_edge", { edgeId }),
  setGraphPosition: (position: GraphPosition) => call<void>("set_graph_position", { position }),
  attachFile: (
    blockId: string,
    sourcePath: string,
    relation: AttachmentRelation,
    locator: Record<string, unknown> = {},
  ) => call<Attachment>("attach_file", { blockId, sourcePath, relation, locator }),
  attachments: (blockId: string) => call<Attachment[]>("attachments_for_block", { blockId }),
  deleteAttachment: (attachmentId: string) => call<void>("soft_delete_attachment", { attachmentId }),
  attachmentPreview: (attachmentId: string) => call<AttachmentPreview>("attachment_preview", { attachmentId }),
  openAttachment: (attachmentId: string) => call<void>("open_attachment", { attachmentId }),
  registerVariable: (symbol: string, definition: string, blockId: string, formula = "") =>
    call<VariableRecord>("register_variable_definition", { input: { symbol, definition, blockId, formula } }),
  variables: () => call<VariableRecord[]>("variables"),
  addCitation: (
    blockId: string,
    input: Omit<CitationRecord, "id">,
    quoteText = "",
    locator: Record<string, unknown> = {},
  ) => call<BlockCitationRecord>("add_citation", { blockId, input, quoteText, locator }),
  citations: (blockId: string) => call<BlockCitationRecord[]>("citations_for_block", { blockId }),
  search: (query: string, limit = 50) => call<SearchHit[]>("search", { query, limit }),
  integrity: () => call<IntegrityReport>("integrity_check"),
  createSnapshot: (kind: SnapshotKind = "Manual") => call<SnapshotRecord>("create_snapshot", { kind }),
  createDueSnapshots: () => call<SnapshotRecord[]>("create_due_snapshots"),
  snapshots: () => call<SnapshotRecord[]>("snapshots"),
  verifySnapshot: (snapshotId: string) => call("verify_snapshot", { snapshotId }),
  restoreSnapshot: (snapshotId: string, destination: string) =>
    call<string>("restore_snapshot_to", { snapshotId, destination }),
  createBackup: (destination: string, password: string) =>
    call<BackupRecord>("create_encrypted_backup", { destination, password }),
  exportPortable: (parentDirectory: string) =>
    call<PortableExportRecord>("export_portable", { parentDirectory }),
  automaticBackupStatus: () => call<AutomaticBackupStatus>("automatic_backup_status"),
  configureAutomaticBackup: (destinationDirectory: string, password: string) =>
    call<AutomaticBackupStatus>("configure_automatic_backup", { destinationDirectory, password }),
  disableAutomaticBackup: () => call<AutomaticBackupStatus>("disable_automatic_backup"),
  runDueAutomaticBackup: () => call<BackupRecord | null>("run_due_automatic_backup"),
  backups: () => call<BackupRecord[]>("backups"),
  verifyBackup: (archive: string, password: string) => call<void>("verify_encrypted_backup", { archive, password }),
  restoreBackup: (archive: string, password: string, destination: string) =>
    call<string>("restore_encrypted_backup_to", { archive, password, destination }),
  chatGptPluginStatus: () => call<ChatGptPluginStatus>("chatgpt_plugin_status"),
  installChatGptPlugin: () => call<ChatGptPluginStatus>("install_chatgpt_plugin"),
  openChatGptPlugin: () => call<void>("open_chatgpt_plugin"),
};
