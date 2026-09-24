//! Android keeps Vaults in its private app directory. File pickers on Android
//! return content URIs, so the frontend stages selected files here first.

use super::{active, safe_path_component, AppState, CommandError, CommandResult};
use sanctum_core::{Attachment, AttachmentRelation, BackupRecord, Vault, VaultSummary};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Manager;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MobileTransfer {
    id: String,
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MobileExport {
    path: String,
    file_name: String,
}

fn root(app: &tauri::AppHandle) -> CommandResult<PathBuf> {
    if !cfg!(target_os = "android") {
        return Err(CommandError { message: "Android版でのみ利用できる".into() });
    }
    Ok(app.path().app_local_data_dir().map_err(|error| error.to_string())?)
}

fn vaults(app: &tauri::AppHandle) -> CommandResult<PathBuf> {
    Ok(root(app)?.join("vaults"))
}

fn transfer_path(app: &tauri::AppHandle, id: &str, file_name: &str) -> CommandResult<PathBuf> {
    let id = Uuid::parse_str(id).map_err(|_| "無効な転送ID".to_owned())?;
    let clean = safe_path_component(file_name);
    if clean != file_name || clean == "attachment" || clean.chars().count() > 180 {
        return Err("無効なファイル名".to_owned().into());
    }
    Ok(root(app)?.join("imports").join(format!("{id}-{clean}")))
}

fn remove_if_present(path: &Path) -> CommandResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[tauri::command]
pub(crate) fn list_mobile_vaults(app: tauri::AppHandle) -> CommandResult<Vec<VaultSummary>> {
    let directory = vaults(&app)?;
    if !directory.exists() { return Ok(Vec::new()); }
    let mut found = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_dir() || path.extension().and_then(|ext| ext.to_str()) != Some("sanctum") {
            continue;
        }
        if path.file_stem().and_then(|stem| stem.to_str()).and_then(|stem| Uuid::parse_str(stem).ok()).is_none() {
            continue;
        }
        found.push(Vault::open(path)?.summary()?);
    }
    found.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(found)
}

#[tauri::command]
pub(crate) fn create_mobile_vault(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    name: String,
) -> CommandResult<VaultSummary> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err("研究テーマは1〜120文字で入力して".to_owned().into());
    }
    let directory = vaults(&app)?;
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{}.sanctum", Uuid::new_v4()));
    let vault = Arc::new(Vault::create(path, name)?);
    let summary = vault.summary()?;
    *state.vault.lock().expect("app state mutex poisoned") = Some(vault);
    Ok(summary)
}

#[tauri::command]
pub(crate) fn open_mobile_vault(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
) -> CommandResult<VaultSummary> {
    let path = PathBuf::from(path);
    let directory = vaults(&app)?;
    if path.parent() != Some(directory.as_path()) ||
        path.file_stem().and_then(|stem| stem.to_str()).and_then(|stem| Uuid::parse_str(stem).ok()).is_none() ||
        path.extension().and_then(|ext| ext.to_str()) != Some("sanctum") {
        return Err("無効なVaultの場所".to_owned().into());
    }
    let vault = Arc::new(Vault::open(path)?);
    let summary = vault.summary()?;
    *state.vault.lock().expect("app state mutex poisoned") = Some(vault);
    Ok(summary)
}

#[tauri::command]
pub(crate) fn prepare_mobile_import(
    app: tauri::AppHandle,
    file_name: String,
) -> CommandResult<MobileTransfer> {
    let directory = root(&app)?.join("imports");
    fs::create_dir_all(&directory)?;
    let id = Uuid::new_v4().to_string();
    let path = transfer_path(&app, &id, &file_name)?;
    Ok(MobileTransfer { id, path: path.to_string_lossy().into_owned() })
}

#[tauri::command]
pub(crate) fn discard_mobile_import(
    app: tauri::AppHandle,
    id: String,
    file_name: String,
) -> CommandResult<()> {
    remove_if_present(&transfer_path(&app, &id, &file_name)?)
}

#[tauri::command]
pub(crate) fn restore_mobile_backup(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    file_name: String,
    password: String,
) -> CommandResult<VaultSummary> {
    if !file_name.ends_with(".sanctum-backup") {
        return Err("Sanctumの暗号化Backupを選んで".to_owned().into());
    }
    let source = transfer_path(&app, &id, &file_name)?;
    let destination = vaults(&app)?.join(format!("{}.sanctum", Uuid::new_v4()));
    let password = Zeroizing::new(password);
    let restored = Vault::restore_encrypted_backup_to(&source, password.as_str(), &destination);
    remove_if_present(&source)?;
    let vault = Arc::new(Vault::open(restored?)?);
    let summary = vault.summary()?;
    *state.vault.lock().expect("app state mutex poisoned") = Some(vault);
    Ok(summary)
}

#[tauri::command]
pub(crate) fn attach_mobile_import(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    file_name: String,
    block_id: String,
    relation: AttachmentRelation,
) -> CommandResult<Attachment> {
    let source = transfer_path(&app, &id, &file_name)?;
    let result = active(&state)?.attach_file(&block_id, &source, relation, Value::Object(Default::default()));
    remove_if_present(&source)?;
    Ok(result?)
}

#[tauri::command]
pub(crate) fn create_mobile_backup(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    password: String,
) -> CommandResult<BackupRecord> {
    let directory = root(&app)?.join("exports");
    fs::create_dir_all(&directory)?;
    let destination = directory.join(format!("Sanctum-{}.sanctum-backup", Uuid::new_v4()));
    let password = Zeroizing::new(password);
    Ok(active(&state)?.create_encrypted_backup(destination, password.as_str())?)
}

#[tauri::command]
pub(crate) fn restore_mobile_snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    snapshot_id: String,
) -> CommandResult<VaultSummary> {
    let directory = vaults(&app)?;
    fs::create_dir_all(&directory)?;
    let destination = directory.join(format!("{}.sanctum", Uuid::new_v4()));
    let restored = active(&state)?.restore_snapshot_to(&snapshot_id, destination)?;
    Ok(Vault::open(restored)?.summary()?)
}

#[tauri::command]
pub(crate) fn export_mobile_attachment(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    attachment_id: String,
) -> CommandResult<MobileExport> {
    let vault = active(&state)?;
    let attachment = vault.attachment(&attachment_id)?;
    let directory = root(&app)?.join("exports");
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{}-{}", Uuid::new_v4(), safe_path_component(&attachment.display_name)));
    fs::copy(vault.attachment_object_path(&attachment_id)?, &path)?;
    Ok(MobileExport { path: path.to_string_lossy().into_owned(), file_name: attachment.display_name })
}

#[tauri::command]
pub(crate) fn discard_mobile_export(app: tauri::AppHandle, path: String) -> CommandResult<()> {
    let directory = root(&app)?.join("exports");
    let file = Path::new(&path);
    if file.parent() != Some(directory.as_path()) || file.file_name().is_none() {
        return Err("無効な書き出し先".to_owned().into());
    }
    remove_if_present(file)
}
