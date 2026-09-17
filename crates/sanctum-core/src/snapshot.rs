use crate::db;
use crate::domain::{SnapshotKind, SnapshotManifest, SnapshotRecord};
use crate::error::{Result, SanctumError};
use crate::journal;
use crate::vault::{assert_research_history_integrity, object_path, verify_vault_database};
use crate::{
    atomic_write, rename_noreplace, sha256_file, sync_directory, sync_file, sync_tree_directories,
    utc_now, Vault,
};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Utc};
use rusqlite::backup::Backup;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const MAX_DATABASE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

impl Vault {
    pub fn create_snapshot(&self, kind: SnapshotKind) -> Result<SnapshotRecord> {
        let id = Uuid::new_v4().to_string();
        let created_at = utc_now();
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        db::assert_database_integrity(&connection)?;
        assert_research_history_integrity(&connection)?;
        let (vault_id, revision): (String, i64) = connection.query_row(
            "SELECT vault_id,revision FROM vault_meta WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut statement = connection.prepare("SELECT sha256 FROM objects ORDER BY sha256")?;
        let object_hashes = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for hash in &object_hashes {
            let path = object_path(&self.root, hash);
            if !path.is_file() || sha256_file(&path)? != *hash {
                return Err(SanctumError::Integrity(format!(
                    "cannot snapshot because object {hash} is missing or corrupt"
                )));
            }
        }

        let staging = self
            .root
            .join("staging")
            .join(format!("snapshot-{id}.partial"));
        fs::create_dir(&staging)?;
        let raw_database = staging.join("sanctum.sqlite.raw");
        let compressed_database = staging.join("sanctum.sqlite.zst");
        let operation = (|| -> Result<(String, i64)> {
            backup_database(&connection, &raw_database, &vault_id, revision)?;
            let database_size = raw_database.metadata()?.len();
            if database_size > MAX_DATABASE_BYTES {
                return Err(SanctumError::InvalidInput(
                    "database exceeds snapshot safety limit".into(),
                ));
            }
            let database_sha256 = sha256_file(&raw_database)?;
            compress_file(&raw_database, &compressed_database)?;
            let manifest = SnapshotManifest {
                format_version: 1,
                snapshot_id: id.clone(),
                vault_id: vault_id.clone(),
                schema_version: db::SCHEMA_VERSION,
                revision,
                kind,
                created_at: created_at.clone(),
                database_sha256: database_sha256.clone(),
                database_byte_size: database_size as i64,
                object_hashes: object_hashes.clone(),
            };
            verify_compressed_database(&compressed_database, &manifest)?;
            atomic_write(
                &staging.join("manifest.json"),
                serde_json::to_string_pretty(&manifest)?.as_bytes(),
            )?;
            fs::remove_file(&raw_database)?;
            sync_directory(&staging)?;
            Ok((database_sha256, database_size as i64))
        })();
        let (database_sha256, _) = match operation {
            Ok(value) => value,
            Err(error) => {
                let quarantine = self
                    .root
                    .join("quarantine")
                    .join(format!("failed-snapshot-{id}"));
                let _ = fs::rename(&staging, quarantine);
                return Err(error);
            }
        };
        let directory_name = format!(
            "{}-r{}-{}",
            created_at.replace([':', '.'], "-"),
            revision,
            id
        );
        let final_path = self.root.join("snapshots").join(&directory_name);
        if final_path.exists() {
            return Err(SanctumError::RefuseOverwrite(final_path));
        }
        rename_noreplace(&staging, &final_path)?;
        sync_directory(&self.root.join("snapshots"))?;
        let relative_path = format!("snapshots/{directory_name}");
        let verified_at = utc_now();
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO snapshots(id,kind,revision,relative_path,database_sha256,created_at,verified_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![id, kind.as_str(), revision, relative_path, database_sha256, created_at, verified_at],
        )?;
        journal::append_event(
            &transaction,
            "SnapshotCreated",
            "Snapshot",
            &id,
            json!({
                "capturedRevision": revision, "kind": kind.as_str(), "relativePath": relative_path
            }),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        Ok(SnapshotRecord {
            id,
            kind,
            revision,
            path: final_path.to_string_lossy().into_owned(),
            database_sha256,
            created_at,
            verified_at,
        })
    }

    pub fn create_due_snapshots(&self) -> Result<Vec<SnapshotRecord>> {
        let (revision, previous) = {
            let connection = self.connection.lock().expect("vault mutex poisoned");
            let revision: i64 = connection.query_row(
                "SELECT revision FROM vault_meta WHERE singleton=1",
                [],
                |row| row.get(0),
            )?;
            let mut values = Vec::new();
            for kind in [
                SnapshotKind::TenMinute,
                SnapshotKind::Daily,
                SnapshotKind::Weekly,
            ] {
                let row: Option<(i64, String)> = connection.query_row(
                    "SELECT revision,created_at FROM snapshots WHERE kind=?1 ORDER BY created_at DESC LIMIT 1",
                    [kind.as_str()], |row| Ok((row.get(0)?,row.get(1)?))
                ).optional()?;
                let has_research_changes = match &row {
                    None => true,
                    Some((captured_revision, _)) => connection.query_row(
                        "SELECT EXISTS(
                           SELECT 1 FROM change_events
                           WHERE revision > ?1 AND event_type NOT IN ('SnapshotCreated','SnapshotRecordRecovered','BackupCreated')
                         )",
                        [captured_revision],
                        |row| row.get::<_, i64>(0),
                    )? != 0,
                };
                values.push((kind, row, has_research_changes));
            }
            (revision, values)
        };
        let now = Utc::now();
        let mut created = Vec::new();
        for (kind, row, has_research_changes) in previous {
            if !has_research_changes {
                continue;
            }
            if row
                .as_ref()
                .is_some_and(|(old_revision, _)| *old_revision == revision)
            {
                continue;
            }
            let due = match row {
                None => true,
                Some((_, timestamp)) => {
                    let previous = DateTime::parse_from_rfc3339(&timestamp)
                        .map_err(|error| {
                            SanctumError::Integrity(format!("invalid snapshot timestamp: {error}"))
                        })?
                        .with_timezone(&Utc);
                    match kind {
                        SnapshotKind::TenMinute => now - previous >= ChronoDuration::minutes(10),
                        SnapshotKind::Daily => now.date_naive() != previous.date_naive(),
                        SnapshotKind::Weekly => now.iso_week() != previous.iso_week(),
                        SnapshotKind::Manual => false,
                    }
                }
            };
            if due {
                created.push(self.create_snapshot(kind)?);
            }
        }
        Ok(created)
    }

    pub fn snapshots(&self) -> Result<Vec<SnapshotRecord>> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id,kind,revision,relative_path,database_sha256,created_at,verified_at
             FROM snapshots ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        let mut output = Vec::new();
        for row in rows {
            let (id, kind, revision, relative, hash, created, verified) = row?;
            output.push(SnapshotRecord {
                id,
                kind: SnapshotKind::try_from(kind.as_str()).map_err(SanctumError::Integrity)?,
                revision,
                path: self.root.join(relative).to_string_lossy().into_owned(),
                database_sha256: hash,
                created_at: created,
                verified_at: verified,
            });
        }
        Ok(output)
    }

    pub fn verify_snapshot(&self, snapshot_id: &str) -> Result<SnapshotManifest> {
        let relative: String = {
            let connection = self.connection.lock().expect("vault mutex poisoned");
            connection
                .query_row(
                    "SELECT relative_path FROM snapshots WHERE id=?1",
                    [snapshot_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| SanctumError::InvalidInput("snapshot not found".into()))?
        };
        verify_snapshot_directory(&self.root, &self.root.join(relative))
    }

    pub fn restore_snapshot_to(
        &self,
        snapshot_id: &str,
        destination: impl AsRef<Path>,
    ) -> Result<PathBuf> {
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(SanctumError::RefuseOverwrite(destination.to_path_buf()));
        }
        let relative: String = {
            let connection = self.connection.lock().expect("vault mutex poisoned");
            connection
                .query_row(
                    "SELECT relative_path FROM snapshots WHERE id=?1",
                    [snapshot_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| SanctumError::InvalidInput("snapshot not found".into()))?
        };
        let snapshot_dir = self.root.join(relative);
        let manifest = verify_snapshot_directory(&self.root, &snapshot_dir)?;
        restore_snapshot_directory(
            &self.root,
            &self.manifest,
            &snapshot_dir,
            &manifest,
            destination,
        )
    }
}

pub(crate) fn backup_database(
    source: &Connection,
    destination_path: &Path,
    expected_vault_id: &str,
    expected_revision: i64,
) -> Result<()> {
    if destination_path.exists() {
        return Err(SanctumError::RefuseOverwrite(
            destination_path.to_path_buf(),
        ));
    }
    db::assert_database_integrity(source)?;
    assert_research_history_integrity(source)?;
    let mut destination = Connection::open(destination_path)?;
    {
        let backup = Backup::new(source, &mut destination)?;
        backup.run_to_completion(64, Duration::from_millis(5), None)?;
    }
    destination.pragma_update(None, "foreign_keys", "ON")?;
    verify_vault_database(&destination, expected_vault_id, expected_revision)?;
    destination.execute_batch("PRAGMA journal_mode=DELETE;")?;
    drop(destination);
    sync_file(destination_path)?;
    Ok(())
}

fn compress_file(source: &Path, destination: &Path) -> Result<()> {
    let mut input = File::open(source)?;
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut encoder = zstd::Encoder::new(output, 9)?;
    std::io::copy(&mut input, &mut encoder)?;
    let output = encoder.finish()?;
    output.sync_all()?;
    Ok(())
}

pub(crate) fn decompress_file(source: &Path, destination: &Path, expected_size: u64) -> Result<()> {
    if expected_size > MAX_DATABASE_BYTES {
        return Err(SanctumError::InvalidInput(
            "declared database size exceeds safety limit".into(),
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| SanctumError::InvalidInput("destination has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let available = fs2::available_space(parent)?;
    if available < expected_size.saturating_add(64 * 1024 * 1024) {
        return Err(SanctumError::Io(std::io::Error::new(
            std::io::ErrorKind::StorageFull,
            "not enough free space to restore database",
        )));
    }
    let input = File::open(source)?;
    let decoder = zstd::Decoder::new(input)?;
    let mut limited = decoder.take(expected_size.saturating_add(1));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let written = std::io::copy(&mut limited, &mut output)?;
    output.sync_all()?;
    if written != expected_size {
        return Err(SanctumError::Integrity(format!(
            "decompressed database size is {written}, expected {expected_size}"
        )));
    }
    Ok(())
}

fn verify_compressed_database(path: &Path, manifest: &SnapshotManifest) -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("verified.sqlite");
    decompress_file(path, &database, manifest.database_byte_size as u64)?;
    if sha256_file(&database)? != manifest.database_sha256 {
        return Err(SanctumError::Integrity(
            "snapshot database hash mismatch".into(),
        ));
    }
    let connection = Connection::open(&database)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    verify_vault_database(&connection, &manifest.vault_id, manifest.revision)?;
    let mut statement = connection.prepare("SELECT sha256 FROM objects ORDER BY sha256")?;
    let database_hashes = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut manifest_hashes = manifest.object_hashes.clone();
    manifest_hashes.sort();
    manifest_hashes.dedup();
    if database_hashes != manifest_hashes || manifest_hashes.len() != manifest.object_hashes.len() {
        return Err(SanctumError::Integrity(
            "snapshot object manifest does not match the database".into(),
        ));
    }
    Ok(())
}

pub(crate) fn verify_snapshot_directory(
    vault_root: &Path,
    snapshot_dir: &Path,
) -> Result<SnapshotManifest> {
    if !snapshot_dir.is_dir() {
        return Err(SanctumError::Integrity(
            "snapshot directory is missing".into(),
        ));
    }
    let manifest: SnapshotManifest =
        serde_json::from_slice(&fs::read(snapshot_dir.join("manifest.json"))?)?;
    if manifest.format_version != 1 || manifest.schema_version != db::SCHEMA_VERSION {
        return Err(SanctumError::UnsupportedFormat(
            "snapshot format or schema is not supported".into(),
        ));
    }
    verify_compressed_database(&snapshot_dir.join("sanctum.sqlite.zst"), &manifest)?;
    for hash in &manifest.object_hashes {
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(SanctumError::Integrity(
                "snapshot manifest contains an invalid object hash".into(),
            ));
        }
        let path = object_path(vault_root, hash);
        if !path.is_file() || sha256_file(&path)? != *hash {
            return Err(SanctumError::Integrity(format!(
                "snapshot object {hash} is missing or corrupt"
            )));
        }
    }
    Ok(manifest)
}

pub(crate) fn reconcile_unrecorded_snapshots(
    connection: &mut Connection,
    vault_root: &Path,
) -> Result<()> {
    let snapshots_root = vault_root.join("snapshots");
    if !snapshots_root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&snapshots_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        if !manifest_path.is_file() {
            continue;
        }
        let candidate: SnapshotManifest = match serde_json::from_slice(&fs::read(&manifest_path)?) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let recorded: i64 = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM snapshots WHERE id=?1)",
            [&candidate.snapshot_id],
            |row| row.get(0),
        )?;
        if recorded != 0 {
            continue;
        }
        let manifest = match verify_snapshot_directory(vault_root, &entry.path()) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        let entry_path = entry.path();
        let relative_path = entry_path
            .strip_prefix(vault_root)
            .map_err(|_| SanctumError::Integrity("snapshot escaped its Vault".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        let verified_at = utc_now();
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO snapshots(id,kind,revision,relative_path,database_sha256,created_at,verified_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                manifest.snapshot_id,
                manifest.kind.as_str(),
                manifest.revision,
                relative_path,
                manifest.database_sha256,
                manifest.created_at,
                verified_at
            ],
        )?;
        journal::append_event(
            &transaction,
            "SnapshotRecordRecovered",
            "Snapshot",
            &manifest.snapshot_id,
            json!({"capturedRevision": manifest.revision, "relativePath": relative_path}),
        )?;
        transaction.commit()?;
    }
    journal::drain_outbox(connection, vault_root)
}

fn restore_snapshot_directory(
    source_root: &Path,
    vault_manifest: &crate::domain::VaultManifest,
    snapshot_dir: &Path,
    snapshot_manifest: &SnapshotManifest,
    destination: &Path,
) -> Result<PathBuf> {
    if destination.exists() {
        return Err(SanctumError::RefuseOverwrite(destination.to_path_buf()));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| SanctumError::InvalidInput("restore destination has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let stage = parent.join(format!(
        ".{}.restoring-{}",
        destination
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("sanctum"),
        Uuid::new_v4()
    ));
    fs::create_dir(&stage)?;
    let operation = (|| -> Result<()> {
        for directory in [
            "db",
            "objects/sha256",
            "journal",
            "snapshots",
            "staging",
            "quarantine",
        ] {
            fs::create_dir_all(stage.join(directory))?;
        }
        let database = stage.join("db/sanctum.sqlite");
        decompress_file(
            &snapshot_dir.join("sanctum.sqlite.zst"),
            &database,
            snapshot_manifest.database_byte_size as u64,
        )?;
        if sha256_file(&database)? != snapshot_manifest.database_sha256 {
            return Err(SanctumError::Integrity(
                "restored snapshot database hash mismatch".into(),
            ));
        }
        for hash in &snapshot_manifest.object_hashes {
            let source = object_path(source_root, hash);
            let target = object_path(&stage, hash);
            fs::create_dir_all(target.parent().expect("object parent"))?;
            fs::copy(&source, &target)?;
            sync_file(&target)?;
            if sha256_file(&target)? != *hash {
                return Err(SanctumError::Integrity(format!(
                    "restored object {hash} failed verification"
                )));
            }
        }
        atomic_write(
            &stage.join("manifest.json"),
            serde_json::to_string_pretty(vault_manifest)?.as_bytes(),
        )?;
        let connection = Connection::open(&database)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        verify_vault_database(
            &connection,
            &snapshot_manifest.vault_id,
            snapshot_manifest.revision,
        )?;
        journal::rebuild_journal(&connection, &stage)?;
        drop(connection);
        sync_tree_directories(&stage)?;
        Ok(())
    })();
    if let Err(error) = operation {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }
    rename_noreplace(&stage, destination)?;
    sync_directory(parent)?;
    Ok(destination.to_path_buf())
}
