mod chatgpt_tunnel;
mod credentials;
mod mcp;
mod mobile;
mod plugin_install;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use sanctum_core::{
    Attachment, AttachmentRelation, AutomaticBackupConfig, BackupRecord, BlockCitationRecord, BlockVersion,
    BranchBlockInput, CitationInput, CreateBlockInput, CreateEdgeInput, GraphData, GraphPosition,
    HypothesisBlock, IntegrityReport, RecoveryDraft, SaveBlockInput, SearchHit, SnapshotKind,
    SnapshotManifest, SnapshotRecord, VariableDefinitionInput, VariableRecord, Vault, VaultSummary,
    PortableExportRecord,
};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use zeroize::Zeroizing;

#[derive(Default)]
struct AppState {
    vault: mcp::SharedVault,
    tunnel: chatgpt_tunnel::TunnelManager,
    mcp_available: bool,
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

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentPreview {
    media_type: String,
    data_base64: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutomaticBackupStatus {
    config: AutomaticBackupConfig,
    has_credential: bool,
    due: bool,
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
fn soft_delete_attachment(
    state: tauri::State<'_, AppState>,
    attachment_id: String,
) -> CommandResult<()> {
    Ok(active(&state)?.soft_delete_attachment(&attachment_id)?)
}

#[tauri::command]
fn attachment_preview(
    state: tauri::State<'_, AppState>,
    attachment_id: String,
) -> CommandResult<AttachmentPreview> {
    const MAX_PREVIEW_BYTES: i64 = 12 * 1024 * 1024;
    let vault = active(&state)?;
    let attachment = vault.attachment(&attachment_id)?;
    let media_type = attachment.media_type.ok_or_else(|| CommandError {
        message: "このファイル形式はプレビューできない".into(),
    })?;
    if !matches!(media_type.as_str(), "image/png" | "image/jpeg" | "image/webp") {
        return Err(CommandError {
            message: "画像以外は外部アプリで開いて".into(),
        });
    }
    if attachment.byte_size > MAX_PREVIEW_BYTES {
        return Err(CommandError {
            message: "12 MBを超える画像は外部アプリで開いて".into(),
        });
    }
    let path = vault.attachment_object_path(&attachment_id)?;
    Ok(AttachmentPreview {
        media_type,
        data_base64: BASE64.encode(fs::read(path)?),
    })
}

#[tauri::command]
fn open_attachment(
    state: tauri::State<'_, AppState>,
    attachment_id: String,
) -> CommandResult<()> {
    let vault = active(&state)?;
    let attachment = vault.attachment(&attachment_id)?;
    let source = vault.attachment_object_path(&attachment_id)?;
    let directory = std::env::temp_dir()
        .join("Sanctum")
        .join(safe_path_component(&attachment.id));
    fs::create_dir_all(&directory)?;
    let target = directory.join(format!(
        "{}-{}",
        attachment.id.get(..8).unwrap_or(&attachment.id),
        safe_path_component(&attachment.display_name)
    ));
    fs::copy(source, &target)?;
    open_with_default_application(&target)?;
    Ok(())
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
    let password = Zeroizing::new(password);
    Ok(active(&state)?.create_encrypted_backup(destination, password.as_str())?)
}

#[tauri::command]
fn export_portable(
    state: tauri::State<'_, AppState>,
    parent_directory: String,
) -> CommandResult<PortableExportRecord> {
    Ok(active(&state)?.export_portable_to(parent_directory)?)
}

#[tauri::command]
fn automatic_backup_status(
    state: tauri::State<'_, AppState>,
) -> CommandResult<AutomaticBackupStatus> {
    let vault = active(&state)?;
    let config = vault.automatic_backup_config()?;
    let target = automatic_backup_credential_target(&vault)?;
    let due = vault.automatic_backup_due(&config)?;
    Ok(AutomaticBackupStatus {
        config,
        has_credential: credentials::has_password(&target),
        due,
    })
}

#[tauri::command]
fn configure_automatic_backup(
    state: tauri::State<'_, AppState>,
    destination_directory: String,
    password: String,
) -> CommandResult<AutomaticBackupStatus> {
    let password = Zeroizing::new(password);
    if password.chars().count() < 12 {
        return Err(CommandError {
            message: "Backupのパスワードは12文字以上にして".into(),
        });
    }
    let vault = active(&state)?;
    let destination = PathBuf::from(&destination_directory);
    if !destination.is_dir() {
        return Err(CommandError {
            message: "自動Backupの保存先フォルダが見つからない".into(),
        });
    }
    let target = automatic_backup_credential_target(&vault)?;
    credentials::write_password(&target, password.as_str())?;
    let config_result = vault.save_automatic_backup_config(true, &destination, None);
    let config = match config_result {
        Ok(config) => config,
        Err(error) => {
            let _ = credentials::delete_password(&target);
            return Err(error.into());
        }
    };
    Ok(AutomaticBackupStatus {
        due: vault.automatic_backup_due(&config)?,
        config,
        has_credential: true,
    })
}

#[tauri::command]
fn disable_automatic_backup(
    state: tauri::State<'_, AppState>,
) -> CommandResult<AutomaticBackupStatus> {
    let vault = active(&state)?;
    let previous = vault.automatic_backup_config()?;
    let target = automatic_backup_credential_target(&vault)?;
    credentials::delete_password(&target)?;
    let config = vault.save_automatic_backup_config(
        false,
        PathBuf::from(&previous.destination_directory),
        previous.last_success_at,
    )?;
    Ok(AutomaticBackupStatus {
        config,
        has_credential: false,
        due: false,
    })
}

#[tauri::command]
fn run_due_automatic_backup(
    state: tauri::State<'_, AppState>,
) -> CommandResult<Option<BackupRecord>> {
    let vault = active(&state)?;
    let config = vault.automatic_backup_config()?;
    if !vault.automatic_backup_due(&config)? {
        return Ok(None);
    }
    let target = automatic_backup_credential_target(&vault)?;
    let password = credentials::read_password(&target).map_err(|_| CommandError {
        message: "自動BackupのパスワードをWindowsから取得できない。復旧画面で設定し直して".into(),
    })?;
    let destination = vault.next_automatic_backup_path(&config)?;
    let record = vault.create_encrypted_backup(destination, password.as_str())?;
    vault.save_automatic_backup_config(
        true,
        PathBuf::from(&config.destination_directory),
        Some(record.created_at.clone()),
    )?;
    Ok(Some(record))
}

#[tauri::command]
fn backups(state: tauri::State<'_, AppState>) -> CommandResult<Vec<BackupRecord>> {
    Ok(active(&state)?.backups()?)
}

#[tauri::command]
fn verify_encrypted_backup(archive: String, password: String) -> CommandResult<()> {
    let password = Zeroizing::new(password);
    Ok(Vault::verify_encrypted_backup(archive, password.as_str())?)
}

#[tauri::command]
fn restore_encrypted_backup_to(
    archive: String,
    password: String,
    destination: String,
) -> CommandResult<String> {
    let password = Zeroizing::new(password);
    Ok(
        Vault::restore_encrypted_backup_to(archive, password.as_str(), destination)?
            .to_string_lossy()
            .into_owned(),
    )
}

#[tauri::command]
fn chatgpt_plugin_status() -> CommandResult<plugin_install::ChatGptPluginStatus> {
    Ok(plugin_install::status()?)
}

#[tauri::command]
fn install_chatgpt_plugin() -> CommandResult<plugin_install::ChatGptPluginStatus> {
    Ok(plugin_install::install()?)
}

#[tauri::command]
fn open_chatgpt_plugin() -> CommandResult<()> {
    let status = plugin_install::status()?;
    if !status.installed {
        return Err(CommandError {
            message: "先にSanctumのChatGPT接続を登録して".into(),
        });
    }
    open_uri_with_default_application(&status.deep_link)?;
    Ok(())
}

#[tauri::command]
fn chatgpt_tunnel_status(
    state: tauri::State<'_, AppState>,
) -> CommandResult<chatgpt_tunnel::ChatGptTunnelStatus> {
    let mut status = state.tunnel.status()?;
    if !state.mcp_available {
        status.last_error = Some(
            "Sanctumのローカル接続を開始できない。Sanctumを一つだけ起動して".into(),
        );
    }
    Ok(status)
}

#[tauri::command]
fn configure_chatgpt_tunnel(
    state: tauri::State<'_, AppState>,
    tunnel_id: String,
    runtime_api_key: String,
    client_path: String,
) -> CommandResult<chatgpt_tunnel::ChatGptTunnelStatus> {
    ensure_mcp_available(&state)?;
    Ok(state
        .tunnel
        .configure(tunnel_id, runtime_api_key, client_path)?)
}

#[tauri::command]
fn start_chatgpt_tunnel(
    state: tauri::State<'_, AppState>,
) -> CommandResult<chatgpt_tunnel::ChatGptTunnelStatus> {
    ensure_mcp_available(&state)?;
    Ok(state.tunnel.start()?)
}

#[tauri::command]
fn stop_chatgpt_tunnel(
    state: tauri::State<'_, AppState>,
) -> CommandResult<chatgpt_tunnel::ChatGptTunnelStatus> {
    Ok(state.tunnel.stop()?)
}

#[tauri::command]
fn forget_chatgpt_tunnel(
    state: tauri::State<'_, AppState>,
) -> CommandResult<chatgpt_tunnel::ChatGptTunnelStatus> {
    Ok(state.tunnel.forget()?)
}

#[tauri::command]
fn open_chatgpt_tunnel_settings() -> CommandResult<()> {
    open_uri_with_default_application(chatgpt_tunnel::PLATFORM_TUNNELS_URL)?;
    Ok(())
}

#[tauri::command]
fn open_chatgpt_connectors() -> CommandResult<()> {
    open_uri_with_default_application(chatgpt_tunnel::CHATGPT_CONNECTORS_URL)?;
    Ok(())
}

#[tauri::command]
fn open_chatgpt_tunnel_admin(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    let status = state.tunnel.status()?;
    let url = status.admin_ui_url.ok_or_else(|| CommandError {
        message: "Tunnelの状態画面はまだ起動していない".into(),
    })?;
    open_uri_with_default_application(&url)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut state = AppState::default();
    #[cfg(desktop)]
    {
    state.mcp_available = match mcp::start(state.vault.clone()) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("Sanctum MCP could not start on {}: {error}", mcp::MCP_ADDRESS);
            false
        }
    };
    if state.mcp_available {
        state.tunnel.start_if_enabled();
    }
    }
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init());
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build());
    let app = builder
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            mobile::list_mobile_vaults,
            mobile::create_mobile_vault,
            mobile::open_mobile_vault,
            mobile::prepare_mobile_import,
            mobile::restore_mobile_backup,
            mobile::attach_mobile_import,
            mobile::discard_mobile_import,
            mobile::create_mobile_backup,
            mobile::restore_mobile_snapshot,
            mobile::export_mobile_attachment,
            mobile::discard_mobile_export,
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
            soft_delete_attachment,
            attachment_preview,
            open_attachment,
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
            export_portable,
            automatic_backup_status,
            configure_automatic_backup,
            disable_automatic_backup,
            run_due_automatic_backup,
            backups,
            verify_encrypted_backup,
            restore_encrypted_backup_to,
            chatgpt_plugin_status,
            install_chatgpt_plugin,
            open_chatgpt_plugin,
            chatgpt_tunnel_status,
            configure_chatgpt_tunnel,
            start_chatgpt_tunnel,
            stop_chatgpt_tunnel,
            forget_chatgpt_tunnel,
            open_chatgpt_tunnel_settings,
            open_chatgpt_connectors,
            open_chatgpt_tunnel_admin
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Sanctum runtime");
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            use tauri::Manager;
            app_handle.state::<AppState>().tunnel.shutdown();
        }
    });
}

fn ensure_mcp_available(state: &tauri::State<'_, AppState>) -> CommandResult<()> {
    if state.mcp_available {
        Ok(())
    } else {
        Err(CommandError {
            message: "Sanctumのローカル接続を開始できない。Sanctumを一つだけ起動して".into(),
        })
    }
}

fn automatic_backup_credential_target(vault: &Vault) -> CommandResult<String> {
    Ok(format!(
        "Sanctum/{}/automatic-backup",
        vault.summary()?.vault_id
    ))
}

fn safe_path_component(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            character if character.is_control() => '-',
            character => character,
        })
        .collect::<String>();
    let value = value.trim_matches(|character: char| character == ' ' || character == '.');
    if value.is_empty() {
        "attachment".into()
    } else {
        value.chars().take(180).collect()
    }
}

#[cfg(windows)]
fn open_with_default_application(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = "open\0".encode_utf16().collect::<Vec<_>>();
    let file = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "default application could not open the attachment (ShellExecuteW {result})"
        )))
    }
}

#[cfg(windows)]
fn open_uri_with_default_application(uri: &str) -> std::io::Result<()> {
    use std::ptr;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = "open\0".encode_utf16().collect::<Vec<_>>();
    let target = uri.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "ChatGPT could not open the plugin link (ShellExecuteW {result})"
        )))
    }
}

#[cfg(target_os = "macos")]
fn open_with_default_application(path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("open").arg(path).spawn()?.wait()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_uri_with_default_application(uri: &str) -> std::io::Result<()> {
    std::process::Command::new("open").arg(uri).spawn()?.wait()?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_with_default_application(path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()?
        .wait()?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_uri_with_default_application(uri: &str) -> std::io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(uri)
        .spawn()?
        .wait()?;
    Ok(())
}
