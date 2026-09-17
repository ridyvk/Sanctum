use sanctum_core::{
    Attachment, AttachmentRelation, BackupRecord, BlockCitationRecord, BlockVersion,
    BranchBlockInput, CitationInput, CreateBlockInput, CreateEdgeInput, GraphData, GraphPosition,
    HypothesisBlock, IntegrityReport, RecoveryDraft, SaveBlockInput, SearchHit, SnapshotKind,
    SnapshotManifest, SnapshotRecord, VariableDefinitionInput, VariableRecord, Vault, VaultSummary,
};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct AppState {
    vault: Mutex<Option<Arc<Vault>>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    message: String,
}

type CommandResult<T> = std::result::Result<T, CommandError>;

impl From<sanctum_core::SanctumError> for CommandError {
    fn from(error: sanctum_core::SanctumError) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

fn active(state: &tauri::State<'_, AppState>) -> CommandResult<Arc<Vault>> {
    state
        .vault
        .lock()
        .expect("app state mutex poisoned")
        .clone()
        .ok_or_else(|| CommandError {
            message: "No Sanctum is open".into(),
        })
}

#[tauri::command]
fn create_vault(
    state: tauri::State<'_, AppState>,
    path: String,
    name: String,
) -> CommandResult<VaultSummary> {
    let vault = Arc::new(Vault::create(PathBuf::from(path), &name)?);
    let summary = vault.summary()?;
    *state.vault.lock().expect("app state mutex poisoned") = Some(vault);
    Ok(summary)
}

#[tauri::command]
fn open_vault(state: tauri::State<'_, AppState>, path: String) -> CommandResult<VaultSummary> {
    let vault = Arc::new(Vault::open(PathBuf::from(path))?);
    let summary = vault.summary()?;
    *state.vault.lock().expect("app state mutex poisoned") = Some(vault);
    Ok(summary)
}

#[tauri::command]
fn close_vault(state: tauri::State<'_, AppState>) {
    *state.vault.lock().expect("app state mutex poisoned") = None;
}

#[tauri::command]
fn vault_summary(state: tauri::State<'_, AppState>) -> CommandResult<VaultSummary> {
    Ok(active(&state)?.summary()?)
}

#[tauri::command]
fn list_blocks(state: tauri::State<'_, AppState>) -> CommandResult<Vec<HypothesisBlock>> {
    Ok(active(&state)?.list_blocks()?)
}

#[tauri::command]
fn list_deleted_blocks(state: tauri::State<'_, AppState>) -> CommandResult<Vec<HypothesisBlock>> {
    Ok(active(&state)?.list_deleted_blocks()?)
}

#[tauri::command]
fn create_block(
    state: tauri::State<'_, AppState>,
    input: CreateBlockInput,
) -> CommandResult<HypothesisBlock> {
    Ok(active(&state)?.create_block(input)?)
}

#[tauri::command]
fn get_block(
    state: tauri::State<'_, AppState>,
    block_id: String,
) -> CommandResult<HypothesisBlock> {
    Ok(active(&state)?.get_block(&block_id)?)
}

#[tauri::command]
fn save_block(
    state: tauri::State<'_, AppState>,
    input: SaveBlockInput,
) -> CommandResult<HypothesisBlock> {
    Ok(active(&state)?.save_block(input)?)
}

#[tauri::command]
fn persist_recovery_draft(
    state: tauri::State<'_, AppState>,
    input: SaveBlockInput,
) -> CommandResult<RecoveryDraft> {
    Ok(active(&state)?.persist_recovery_draft(input)?)
}

#[tauri::command]
fn recovery_draft(
    state: tauri::State<'_, AppState>,
    block_id: String,
) -> CommandResult<Option<RecoveryDraft>> {
    Ok(active(&state)?.recovery_draft(&block_id)?)
}

#[tauri::command]
fn discard_recovery_draft(
    state: tauri::State<'_, AppState>,
    block_id: String,
    expected_sha256: String,
) -> CommandResult<bool> {
    Ok(active(&state)?.discard_recovery_draft(&block_id, &expected_sha256)?)
}

#[tauri::command]
fn versions(
    state: tauri::State<'_, AppState>,
    block_id: String,
) -> CommandResult<Vec<BlockVersion>> {
    Ok(active(&state)?.versions(&block_id)?)
}

#[tauri::command]
fn restore_block_version(
    state: tauri::State<'_, AppState>,
    block_id: String,
    version_id: String,
    reason: String,
) -> CommandResult<HypothesisBlock> {
    Ok(active(&state)?.restore_block_version(&block_id, &version_id, &reason)?)
}

#[tauri::command]
fn branch_block(
    state: tauri::State<'_, AppState>,
    input: BranchBlockInput,
) -> CommandResult<HypothesisBlock> {
    Ok(active(&state)?.branch_block(input)?)
}

#[tauri::command]
fn soft_delete_block(
    state: tauri::State<'_, AppState>,
    block_id: String,
    expected_row_version: i64,
) -> CommandResult<()> {
    Ok(active(&state)?.soft_delete_block(&block_id, expected_row_version)?)
}

#[tauri::command]
fn restore_deleted_block(
    state: tauri::State<'_, AppState>,
    block_id: String,
    expected_row_version: i64,
) -> CommandResult<()> {
    Ok(active(&state)?.restore_deleted_block(&block_id, expected_row_version)?)
}

#[tauri::command]
fn graph(state: tauri::State<'_, AppState>) -> CommandResult<GraphData> {
    Ok(active(&state)?.graph()?)
}

#[tauri::command]
fn create_edge(
    state: tauri::State<'_, AppState>,
    input: CreateEdgeInput,
) -> CommandResult<sanctum_core::ResearchEdge> {
    Ok(active(&state)?.create_edge(input)?)
}

#[tauri::command]
fn soft_delete_edge(state: tauri::State<'_, AppState>, edge_id: String) -> CommandResult<()> {
    Ok(active(&state)?.soft_delete_edge(&edge_id)?)
}

#[tauri::command]
fn set_graph_position(
    state: tauri::State<'_, AppState>,
    position: GraphPosition,
) -> CommandResult<()> {
    Ok(active(&state)?.set_graph_position(position)?)
}

#[tauri::command]
fn attach_file(
    state: tauri::State<'_, AppState>,
    block_id: String,
    source_path: String,
    relation: AttachmentRelation,
    locator: Value,
) -> CommandResult<Attachment> {
    Ok(active(&state)?.attach_file(&block_id, source_path, relation, locator)?)
}

#[tauri::command]
fn attachments_for_block(
    state: tauri::State<'_, AppState>,
    block_id: String,
) -> CommandResult<Vec<Attachment>> {
    Ok(active(&state)?.attachments_for_block(&block_id)?)
}

#[tauri::command]
fn register_variable_definition(
    state: tauri::State<'_, AppState>,
    input: VariableDefinitionInput,
) -> CommandResult<VariableRecord> {
    Ok(active(&state)?.register_variable_definition(input)?)
}

#[tauri::command]
fn variables(state: tauri::State<'_, AppState>) -> CommandResult<Vec<VariableRecord>> {
    Ok(active(&state)?.variables()?)
}

#[tauri::command]
fn add_citation(
    state: tauri::State<'_, AppState>,
    block_id: String,
    input: CitationInput,
    quote_text: String,
    locator: Value,
) -> CommandResult<BlockCitationRecord> {
    Ok(active(&state)?.add_citation(&block_id, input, &quote_text, locator)?)
}

#[tauri::command]
fn citations_for_block(
    state: tauri::State<'_, AppState>,
    block_id: String,
) -> CommandResult<Vec<BlockCitationRecord>> {
    Ok(active(&state)?.citations_for_block(&block_id)?)
}

#[tauri::command]
fn search(
    state: tauri::State<'_, AppState>,
    query: String,
    limit: usize,
) -> CommandResult<Vec<SearchHit>> {
    Ok(active(&state)?.search(&query, limit)?)
}

#[tauri::command]
fn integrity_check(state: tauri::State<'_, AppState>) -> CommandResult<IntegrityReport> {
    Ok(active(&state)?.integrity_check()?)
}

#[tauri::command]
fn create_snapshot(
    state: tauri::State<'_, AppState>,
    kind: SnapshotKind,
) -> CommandResult<SnapshotRecord> {
    Ok(active(&state)?.create_snapshot(kind)?)
}

#[tauri::command]
fn create_due_snapshots(state: tauri::State<'_, AppState>) -> CommandResult<Vec<SnapshotRecord>> {
    Ok(active(&state)?.create_due_snapshots()?)
}

#[tauri::command]
fn snapshots(state: tauri::State<'_, AppState>) -> CommandResult<Vec<SnapshotRecord>> {
    Ok(active(&state)?.snapshots()?)
}

#[tauri::command]
fn verify_snapshot(
    state: tauri::State<'_, AppState>,
    snapshot_id: String,
) -> CommandResult<SnapshotManifest> {
    Ok(active(&state)?.verify_snapshot(&snapshot_id)?)
}

#[tauri::command]
fn restore_snapshot_to(
    state: tauri::State<'_, AppState>,
    snapshot_id: String,
    destination: String,
) -> CommandResult<String> {
    Ok(active(&state)?
        .restore_snapshot_to(&snapshot_id, destination)?
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
fn create_encrypted_backup(
    state: tauri::State<'_, AppState>,
    destination: String,
    password: String,
) -> CommandResult<BackupRecord> {
    Ok(active(&state)?.create_encrypted_backup(destination, &password)?)
}

#[tauri::command]
fn backups(state: tauri::State<'_, AppState>) -> CommandResult<Vec<BackupRecord>> {
    Ok(active(&state)?.backups()?)
}

#[tauri::command]
fn verify_encrypted_backup(archive: String, password: String) -> CommandResult<()> {
    Ok(Vault::verify_encrypted_backup(archive, &password)?)
}

#[tauri::command]
fn restore_encrypted_backup_to(
    archive: String,
    password: String,
    destination: String,
) -> CommandResult<String> {
    Ok(
        Vault::restore_encrypted_backup_to(archive, &password, destination)?
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            create_vault,
            open_vault,
            close_vault,
            vault_summary,
            list_blocks,
            list_deleted_blocks,
            create_block,
            get_block,
            save_block,
            persist_recovery_draft,
            recovery_draft,
            discard_recovery_draft,
            versions,
            restore_block_version,
            branch_block,
            soft_delete_block,
            restore_deleted_block,
            graph,
            create_edge,
            soft_delete_edge,
            set_graph_position,
            attach_file,
            attachments_for_block,
            register_variable_definition,
            variables,
            add_citation,
            citations_for_block,
            search,
            integrity_check,
            create_snapshot,
            create_due_snapshots,
            snapshots,
            verify_snapshot,
            restore_snapshot_to,
            create_encrypted_backup,
            backups,
            verify_encrypted_backup,
            restore_encrypted_backup_to
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Sanctum desktop runtime");
}
