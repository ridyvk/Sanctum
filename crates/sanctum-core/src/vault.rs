use crate::db;
use crate::domain::*;
use crate::error::{Result, SanctumError};
use crate::journal;
use crate::{
    atomic_write, rename_noreplace, sha256_bytes, sha256_file, sync_directory, sync_file, utc_now,
};
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

type BlockDatabaseRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    String,
    String,
    Option<String>,
);

type CitationJoinRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<i64>,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    String,
);

pub struct Vault {
    pub(crate) root: PathBuf,
    pub(crate) connection: Mutex<Connection>,
    lock: File,
    pub(crate) manifest: VaultManifest,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Vault")
            .field("root", &self.root)
            .field("manifest", &self.manifest)
            .finish_non_exhaustive()
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = self.lock.unlock();
    }
}

impl Vault {
    pub fn create(path: impl AsRef<Path>, name: &str) -> Result<Self> {
        let path = path.as_ref();
        let name = name.trim();
        if name.is_empty() {
            return Err(SanctumError::InvalidInput("vault name is empty".into()));
        }
        if path.exists() {
            return Err(SanctumError::VaultExists(path.to_path_buf()));
        }
        let parent = path
            .parent()
            .ok_or_else(|| SanctumError::InvalidInput("vault path has no parent".into()))?;
        fs::create_dir_all(parent)?;
        let staging = parent.join(format!(
            ".{}.creating-{}",
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("sanctum"),
            Uuid::new_v4()
        ));
        fs::create_dir(&staging)?;
        let result = (|| -> Result<()> {
            for directory in [
                "db",
                "objects/sha256",
                "journal",
                "snapshots",
                "staging",
                "quarantine",
            ] {
                fs::create_dir_all(staging.join(directory))?;
            }
            let manifest = VaultManifest {
                format_version: 1,
                vault_id: Uuid::new_v4().to_string(),
                name: name.to_owned(),
                created_at: utc_now(),
            };
            atomic_write(
                &staging.join("manifest.json"),
                serde_json::to_string_pretty(&manifest)?.as_bytes(),
            )?;
            let database_path = staging.join("db/sanctum.sqlite");
            let mut connection = db::create_new(&database_path)?;
            db::initialize_or_migrate(&mut connection)?;
            connection.execute(
                "INSERT INTO vault_meta(singleton,vault_id,name,created_at,revision)
                 VALUES(1,?1,?2,?3,0)",
                params![manifest.vault_id, manifest.name, manifest.created_at],
            )?;
            connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            drop(connection);
            sync_file(&database_path)?;
            sync_directory(&staging.join("db"))?;
            sync_directory(&staging)?;
            Ok(())
        })();
        if let Err(error) = result {
            let quarantine = parent.join(format!(".sanctum-failed-create-{}", Uuid::new_v4()));
            let _ = fs::rename(&staging, &quarantine);
            return Err(error);
        }
        rename_noreplace(&staging, path)?;
        sync_directory(parent)?;
        Self::open(path)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        let manifest_path = root.join("manifest.json");
        let database_path = root.join("db/sanctum.sqlite");
        if !root.is_dir() || !manifest_path.is_file() || !database_path.is_file() {
            return Err(SanctumError::VaultMissing(root));
        }
        let manifest: VaultManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        if manifest.format_version != 1 {
            return Err(SanctumError::UnsupportedFormat(format!(
                "vault format {}",
                manifest.format_version
            )));
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("vault.lock"))?;
        lock.try_lock_exclusive()
            .map_err(|_| SanctumError::VaultLocked)?;

        let mut connection = db::open_existing(&database_path)?;
        db::initialize_or_migrate(&mut connection)?;
        let identity: (String, String) = connection.query_row(
            "SELECT vault_id,name FROM vault_meta WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if identity.0 != manifest.vault_id || identity.1 != manifest.name {
            return Err(SanctumError::Integrity(
                "vault manifest and database identity disagree".into(),
            ));
        }
        db::assert_database_integrity(&connection)?;
        journal::drain_outbox(&mut connection, &root)?;
        if let Some(first) = journal::verify_journal(&connection, &root)?.first() {
            return Err(SanctumError::Integrity(first.clone()));
        }
        assert_research_history_integrity(&connection)?;
        crate::snapshot::reconcile_unrecorded_snapshots(&mut connection, &root)?;

        Ok(Self {
            root,
            connection: Mutex::new(connection),
            lock,
            manifest,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn summary(&self) -> Result<VaultSummary> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let revision: i64 = connection.query_row(
            "SELECT revision FROM vault_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(VaultSummary {
            vault_id: self.manifest.vault_id.clone(),
            name: self.manifest.name.clone(),
            path: self.root.to_string_lossy().into_owned(),
            created_at: self.manifest.created_at.clone(),
            revision,
        })
    }

    pub fn create_block(&self, input: CreateBlockInput) -> Result<HypothesisBlock> {
        validate_title_reason(&input.title, &input.change_reason)?;
        let id = Uuid::new_v4().to_string();
        let now = utc_now();
        let snapshot = BlockSnapshot {
            id: id.clone(),
            kind: input.kind,
            title: input.title.trim().to_owned(),
            body_markdown: input.body_markdown,
            research_notes_markdown: input.research_notes_markdown,
            status: input.status,
            tags: normalize_tags(input.tags),
        };
        let version_id = Uuid::new_v4().to_string();
        let snapshot_json = serde_json::to_string(&snapshot)?;
        let hash = sha256_bytes(snapshot_json.as_bytes());
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO blocks(id,kind,title,body_markdown,research_notes_markdown,status,
              current_version_id,parent_block_id,row_version,created_at,updated_at,deleted_at)
             VALUES(?1,?2,?3,?4,?5,?6,NULL,NULL,1,?7,?7,NULL)",
            params![
                id,
                snapshot.kind.as_str(),
                snapshot.title,
                snapshot.body_markdown,
                snapshot.research_notes_markdown,
                snapshot.status.as_str(),
                now
            ],
        )?;
        transaction.execute(
            "INSERT INTO block_versions(id,block_id,version_index,version_label,snapshot_json,
              content_sha256,change_reason,created_at) VALUES(?1,?2,1,'v1.0',?3,?4,?5,?6)",
            params![
                version_id,
                id,
                snapshot_json,
                hash,
                input.change_reason.trim(),
                now
            ],
        )?;
        transaction.execute(
            "UPDATE blocks SET current_version_id=?1 WHERE id=?2",
            params![version_id, id],
        )?;
        replace_tags(&transaction, &id, &snapshot.tags, &now)?;
        refresh_fts(&transaction, &id)?;
        journal::append_event(
            &transaction,
            "BlockCreated",
            "Block",
            &id,
            json!({"versionId": version_id, "reason": input.change_reason}),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        load_block(&connection, &id, true)
    }

    pub fn get_block(&self, block_id: &str) -> Result<HypothesisBlock> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        load_block(&connection, block_id, false)
    }

    pub fn list_blocks(&self) -> Result<Vec<HypothesisBlock>> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        list_blocks_where(&connection, "deleted_at IS NULL")
    }

    pub fn list_deleted_blocks(&self) -> Result<Vec<HypothesisBlock>> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        list_blocks_where(&connection, "deleted_at IS NOT NULL")
    }

    pub fn save_block(&self, input: SaveBlockInput) -> Result<HypothesisBlock> {
        validate_title_reason(&input.title, &input.change_reason)?;
        let snapshot = snapshot_from_save(&input);
        let snapshot_json = serde_json::to_string(&snapshot)?;
        let content_hash = sha256_bytes(snapshot_json.as_bytes());
        let now = utc_now();
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        let (actual, current_content_hash): (i64, String) = transaction
            .query_row(
                "SELECT b.row_version,v.content_sha256
                 FROM blocks b JOIN block_versions v ON v.id=b.current_version_id
                 WHERE b.id=?1 AND b.deleted_at IS NULL",
                [&input.block_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| SanctumError::BlockNotFound(input.block_id.clone()))?;
        if actual != input.expected_row_version {
            return Err(SanctumError::Conflict {
                block_id: input.block_id,
                expected: input.expected_row_version,
                actual,
            });
        }
        if current_content_hash == content_hash {
            transaction.execute(
                "DELETE FROM block_drafts WHERE block_id=?1 AND content_sha256=?2",
                params![snapshot.id, content_hash],
            )?;
            transaction.commit()?;
            return load_block(&connection, &snapshot.id, true);
        }
        let next_index: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(version_index),0)+1 FROM block_versions WHERE block_id=?1",
            [&snapshot.id],
            |row| row.get(0),
        )?;
        let version_id = Uuid::new_v4().to_string();
        let version_label = format!("v1.{}", next_index - 1);
        transaction.execute(
            "INSERT INTO block_versions(id,block_id,version_index,version_label,snapshot_json,
              content_sha256,change_reason,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                version_id,
                snapshot.id,
                next_index,
                version_label,
                snapshot_json,
                content_hash,
                input.change_reason.trim(),
                now
            ],
        )?;
        let changed = transaction.execute(
            "UPDATE blocks SET kind=?1,title=?2,body_markdown=?3,research_notes_markdown=?4,
             status=?5,current_version_id=?6,row_version=row_version+1,updated_at=?7
             WHERE id=?8 AND row_version=?9 AND deleted_at IS NULL",
            params![
                snapshot.kind.as_str(),
                snapshot.title,
                snapshot.body_markdown,
                snapshot.research_notes_markdown,
                snapshot.status.as_str(),
                version_id,
                now,
                snapshot.id,
                input.expected_row_version
            ],
        )?;
        if changed != 1 {
            return Err(SanctumError::Conflict {
                block_id: snapshot.id,
                expected: input.expected_row_version,
                actual,
            });
        }
        replace_tags(&transaction, &snapshot.id, &snapshot.tags, &now)?;
        transaction.execute(
            "DELETE FROM block_drafts WHERE block_id=?1 AND content_sha256=?2",
            params![snapshot.id, content_hash],
        )?;
        refresh_fts(&transaction, &snapshot.id)?;
        journal::append_event(
            &transaction,
            "BlockSaved",
            "Block",
            &snapshot.id,
            json!({"versionId": version_id, "reason": input.change_reason}),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        load_block(&connection, &snapshot.id, true)
    }

    pub fn persist_recovery_draft(&self, input: SaveBlockInput) -> Result<RecoveryDraft> {
        if input.title.trim().is_empty() {
            return Err(SanctumError::InvalidInput("block title is empty".into()));
        }
        let snapshot = snapshot_from_save(&input);
        let snapshot_json = serde_json::to_string(&snapshot)?;
        let content_sha256 = sha256_bytes(snapshot_json.as_bytes());
        let updated_at = utc_now();
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        let transaction = connection.transaction()?;
        let actual: i64 = transaction
            .query_row(
                "SELECT row_version FROM blocks WHERE id=?1 AND deleted_at IS NULL",
                [&input.block_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| SanctumError::BlockNotFound(input.block_id.clone()))?;
        if actual != input.expected_row_version {
            return Err(SanctumError::Conflict {
                block_id: input.block_id,
                expected: input.expected_row_version,
                actual,
            });
        }
        transaction.execute(
            "INSERT INTO block_drafts(block_id,base_row_version,snapshot_json,content_sha256,updated_at)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(block_id) DO UPDATE SET
               base_row_version=excluded.base_row_version,
               snapshot_json=excluded.snapshot_json,
               content_sha256=excluded.content_sha256,
               updated_at=excluded.updated_at",
            params![snapshot.id, input.expected_row_version, snapshot_json, content_sha256, updated_at],
        )?;
        transaction.commit()?;
        Ok(RecoveryDraft {
            block_id: snapshot.id.clone(),
            base_row_version: input.expected_row_version,
            snapshot,
            content_sha256,
            updated_at,
        })
    }

    pub fn recovery_draft(&self, block_id: &str) -> Result<Option<RecoveryDraft>> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let row: Option<(i64, String, String, String)> = connection
            .query_row(
                "SELECT base_row_version,snapshot_json,content_sha256,updated_at
                 FROM block_drafts WHERE block_id=?1",
                [block_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        row.map(
            |(base_row_version, snapshot_json, content_sha256, updated_at)| {
                Ok(RecoveryDraft {
                    block_id: block_id.to_owned(),
                    base_row_version,
                    snapshot: serde_json::from_str(&snapshot_json)?,
                    content_sha256,
                    updated_at,
                })
            },
        )
        .transpose()
    }

    pub fn discard_recovery_draft(&self, block_id: &str, expected_sha256: &str) -> Result<bool> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        Ok(connection.execute(
            "DELETE FROM block_drafts WHERE block_id=?1 AND content_sha256=?2",
            params![block_id, expected_sha256],
        )? == 1)
    }

    pub fn versions(&self, block_id: &str) -> Result<Vec<BlockVersion>> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id,block_id,version_index,version_label,snapshot_json,content_sha256,
                    change_reason,created_at
             FROM block_versions WHERE block_id=?1 ORDER BY version_index DESC",
        )?;
        let rows = statement.query_map([block_id], version_from_row)?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub fn restore_block_version(
        &self,
        block_id: &str,
        version_id: &str,
        reason: &str,
    ) -> Result<HypothesisBlock> {
        if reason.trim().is_empty() {
            return Err(SanctumError::InvalidInput("restore reason is empty".into()));
        }
        let (snapshot, row_version) = {
            let connection = self.connection.lock().expect("vault mutex poisoned");
            let snapshot_json: String = connection
                .query_row(
                    "SELECT snapshot_json FROM block_versions WHERE id=?1 AND block_id=?2",
                    params![version_id, block_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| {
                    SanctumError::InvalidInput("version does not belong to block".into())
                })?;
            let row_version: i64 = connection.query_row(
                "SELECT row_version FROM blocks WHERE id=?1 AND deleted_at IS NULL",
                [block_id],
                |row| row.get(0),
            )?;
            (
                serde_json::from_str::<BlockSnapshot>(&snapshot_json)?,
                row_version,
            )
        };
        self.save_block(SaveBlockInput {
            block_id: block_id.to_owned(),
            expected_row_version: row_version,
            title: snapshot.title,
            body_markdown: snapshot.body_markdown,
            research_notes_markdown: snapshot.research_notes_markdown,
            kind: snapshot.kind,
            status: snapshot.status,
            tags: snapshot.tags,
            change_reason: reason.to_owned(),
        })
    }

    pub fn branch_block(&self, input: BranchBlockInput) -> Result<HypothesisBlock> {
        validate_title_reason(&input.title, &input.branch_reason)?;
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        let parent = load_block(&transaction, &input.parent_block_id, false)?;
        if parent.row_version != input.expected_parent_row_version {
            return Err(SanctumError::Conflict {
                block_id: input.parent_block_id,
                expected: input.expected_parent_row_version,
                actual: parent.row_version,
            });
        }
        let child_id = Uuid::new_v4().to_string();
        let version_id = Uuid::new_v4().to_string();
        let now = utc_now();
        let mut snapshot = parent.snapshot.clone();
        snapshot.id = child_id.clone();
        snapshot.title = input.title.trim().to_owned();
        let snapshot_json = serde_json::to_string(&snapshot)?;
        let hash = sha256_bytes(snapshot_json.as_bytes());
        transaction.execute(
            "INSERT INTO blocks(id,kind,title,body_markdown,research_notes_markdown,status,
              current_version_id,parent_block_id,row_version,created_at,updated_at,deleted_at)
             VALUES(?1,?2,?3,?4,?5,?6,NULL,?7,1,?8,?8,NULL)",
            params![
                child_id,
                snapshot.kind.as_str(),
                snapshot.title,
                snapshot.body_markdown,
                snapshot.research_notes_markdown,
                snapshot.status.as_str(),
                input.parent_block_id,
                now
            ],
        )?;
        transaction.execute(
            "INSERT INTO block_versions(id,block_id,version_index,version_label,snapshot_json,
              content_sha256,change_reason,created_at) VALUES(?1,?2,1,'v1.0',?3,?4,?5,?6)",
            params![
                version_id,
                child_id,
                snapshot_json,
                hash,
                input.branch_reason.trim(),
                now
            ],
        )?;
        transaction.execute(
            "UPDATE blocks SET current_version_id=?1 WHERE id=?2",
            params![version_id, child_id],
        )?;
        transaction.execute(
            "INSERT INTO block_lineage(child_block_id,parent_block_id,parent_version_id,branch_reason,created_at)
             VALUES(?1,?2,?3,?4,?5)",
            params![child_id, input.parent_block_id, parent.current_version_id, input.branch_reason.trim(), now],
        )?;
        replace_tags(&transaction, &child_id, &snapshot.tags, &now)?;
        refresh_fts(&transaction, &child_id)?;
        journal::append_event(
            &transaction,
            "BlockBranched",
            "Block",
            &child_id,
            json!({"parentBlockId": input.parent_block_id, "parentVersionId": parent.current_version_id}),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        load_block(&connection, &child_id, true)
    }

    pub fn soft_delete_block(&self, block_id: &str, expected_row_version: i64) -> Result<()> {
        self.set_block_deleted(block_id, expected_row_version, true)
    }

    pub fn restore_deleted_block(&self, block_id: &str, expected_row_version: i64) -> Result<()> {
        self.set_block_deleted(block_id, expected_row_version, false)
    }

    fn set_block_deleted(
        &self,
        block_id: &str,
        expected_row_version: i64,
        deleted: bool,
    ) -> Result<()> {
        let now = utc_now();
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        let changed = if deleted {
            transaction.execute(
                "UPDATE blocks SET deleted_at=?1,updated_at=?1,row_version=row_version+1
                 WHERE id=?2 AND row_version=?3 AND deleted_at IS NULL",
                params![now, block_id, expected_row_version],
            )?
        } else {
            transaction.execute(
                "UPDATE blocks SET deleted_at=NULL,updated_at=?1,row_version=row_version+1
                 WHERE id=?2 AND row_version=?3 AND deleted_at IS NOT NULL",
                params![now, block_id, expected_row_version],
            )?
        };
        if changed != 1 {
            let actual: Option<i64> = transaction
                .query_row(
                    "SELECT row_version FROM blocks WHERE id=?1",
                    [block_id],
                    |row| row.get(0),
                )
                .optional()?;
            return match actual {
                Some(actual) => Err(SanctumError::Conflict {
                    block_id: block_id.to_owned(),
                    expected: expected_row_version,
                    actual,
                }),
                None => Err(SanctumError::BlockNotFound(block_id.to_owned())),
            };
        }
        refresh_fts(&transaction, block_id)?;
        journal::append_event(
            &transaction,
            if deleted {
                "BlockSoftDeleted"
            } else {
                "BlockRestored"
            },
            "Block",
            block_id,
            json!({"rowVersion": expected_row_version + 1}),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        Ok(())
    }

    pub fn create_edge(&self, input: CreateEdgeInput) -> Result<ResearchEdge> {
        if input.source_block_id == input.target_block_id {
            return Err(SanctumError::InvalidInput(
                "self edges are not allowed".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        let now = utc_now();
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        ensure_active_block(&transaction, &input.source_block_id)?;
        ensure_active_block(&transaction, &input.target_block_id)?;
        transaction.execute(
            "INSERT INTO edges(id,source_block_id,target_block_id,edge_type,note,created_at,updated_at,deleted_at)
             VALUES(?1,?2,?3,?4,?5,?6,?6,NULL)",
            params![id, input.source_block_id, input.target_block_id, input.edge_type.as_str(), input.note, now],
        )?;
        journal::append_event(
            &transaction,
            "EdgeCreated",
            "Edge",
            &id,
            json!({
                "sourceBlockId": input.source_block_id,
                "targetBlockId": input.target_block_id,
                "edgeType": input.edge_type.as_str()
            }),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        Ok(ResearchEdge {
            id,
            source_block_id: input.source_block_id,
            target_block_id: input.target_block_id,
            edge_type: input.edge_type,
            note: input.note,
            created_at: now.clone(),
            updated_at: now,
            deleted_at: None,
        })
    }

    pub fn soft_delete_edge(&self, edge_id: &str) -> Result<()> {
        let now = utc_now();
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        if transaction.execute(
            "UPDATE edges SET deleted_at=?1,updated_at=?1 WHERE id=?2 AND deleted_at IS NULL",
            params![now, edge_id],
        )? != 1
        {
            return Err(SanctumError::InvalidInput("active edge not found".into()));
        }
        journal::append_event(&transaction, "EdgeSoftDeleted", "Edge", edge_id, json!({}))?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        Ok(())
    }

    pub fn graph(&self) -> Result<GraphData> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let blocks = list_blocks_where(&connection, "deleted_at IS NULL")?;
        let mut statement = connection.prepare(
            "SELECT id,source_block_id,target_block_id,edge_type,note,created_at,updated_at,deleted_at
             FROM edges WHERE deleted_at IS NULL ORDER BY created_at",
        )?;
        let edge_rows = statement.query_map([], |row| {
            let edge_type: String = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                edge_type,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?;
        let mut edges = Vec::new();
        for row in edge_rows {
            let (id, source, target, kind, note, created, updated, deleted) = row?;
            edges.push(ResearchEdge {
                id,
                source_block_id: source,
                target_block_id: target,
                edge_type: EdgeType::try_from(kind.as_str()).map_err(SanctumError::Integrity)?,
                note,
                created_at: created,
                updated_at: updated,
                deleted_at: deleted,
            });
        }
        drop(statement);
        let mut statement =
            connection.prepare("SELECT block_id,view_id,x,y FROM graph_positions")?;
        let rows = statement.query_map([], |row| {
            Ok(GraphPosition {
                block_id: row.get(0)?,
                view_id: row.get(1)?,
                x: row.get(2)?,
                y: row.get(3)?,
            })
        })?;
        let positions = rows
            .map(|row| row.map_err(Into::into))
            .collect::<Result<Vec<_>>>()?;
        Ok(GraphData {
            blocks,
            edges,
            positions,
        })
    }

    pub fn set_graph_position(&self, position: GraphPosition) -> Result<()> {
        if !position.x.is_finite() || !position.y.is_finite() || position.view_id.trim().is_empty()
        {
            return Err(SanctumError::InvalidInput("invalid graph position".into()));
        }
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        ensure_active_block(&transaction, &position.block_id)?;
        transaction.execute(
            "INSERT INTO graph_positions(block_id,view_id,x,y,updated_at) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(block_id,view_id) DO UPDATE SET x=excluded.x,y=excluded.y,updated_at=excluded.updated_at",
            params![position.block_id, position.view_id, position.x, position.y, utc_now()],
        )?;
        journal::append_event(
            &transaction,
            "GraphPositionChanged",
            "Block",
            &position.block_id,
            json!({"viewId": position.view_id, "x": position.x, "y": position.y}),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        Ok(())
    }

    pub fn register_variable_definition(
        &self,
        input: VariableDefinitionInput,
    ) -> Result<VariableRecord> {
        let symbol = input.symbol.trim();
        let definition = input.definition.trim();
        if symbol.is_empty() || definition.is_empty() {
            return Err(SanctumError::InvalidInput(
                "variable symbol and definition are required".into(),
            ));
        }
        let now = utc_now();
        let variable_id = Uuid::new_v4().to_string();
        let definition_id = Uuid::new_v4().to_string();
        let normalized = normalize_definition(definition);
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        ensure_active_block(&transaction, &input.block_id)?;
        transaction.execute(
            "INSERT INTO variables(id,symbol,created_at,deleted_at) VALUES(?1,?2,?3,NULL)
             ON CONFLICT(symbol) DO NOTHING",
            params![variable_id, symbol, now],
        )?;
        let actual_variable_id: String = transaction.query_row(
            "SELECT id FROM variables WHERE symbol=?1 COLLATE NOCASE AND deleted_at IS NULL",
            [symbol],
            |row| row.get(0),
        )?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT id FROM variable_definitions
                 WHERE variable_id=?1 AND block_id=?2 AND normalized_definition=?3 AND deleted_at IS NULL",
                params![actual_variable_id, input.block_id, normalized],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_none() {
            transaction.execute(
                "INSERT INTO variable_definitions(id,variable_id,block_id,definition,normalized_definition,formula,created_at,deleted_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,NULL)",
                params![definition_id, actual_variable_id, input.block_id, definition, normalized, input.formula, now],
            )?;
            journal::append_event(
                &transaction,
                "VariableDefined",
                "Variable",
                &actual_variable_id,
                json!({
                    "blockId": input.block_id, "symbol": symbol
                }),
            )?;
        }
        refresh_fts(&transaction, &input.block_id)?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        load_variable(&connection, &actual_variable_id)
    }

    pub fn variables(&self) -> Result<Vec<VariableRecord>> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id FROM variables WHERE deleted_at IS NULL ORDER BY symbol COLLATE NOCASE",
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter()
            .map(|id| load_variable(&connection, id))
            .collect()
    }

    pub fn add_citation(
        &self,
        block_id: &str,
        input: CitationInput,
        quote_text: &str,
        locator: Value,
    ) -> Result<BlockCitationRecord> {
        if input.citation_key.trim().is_empty() || input.title.trim().is_empty() {
            return Err(SanctumError::InvalidInput(
                "citation key and title are required".into(),
            ));
        }
        let now = utc_now();
        let proposed_id = Uuid::new_v4().to_string();
        let link_id = Uuid::new_v4().to_string();
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        ensure_active_block(&transaction, block_id)?;
        transaction.execute(
            "INSERT INTO citations(id,citation_key,title,authors,year,doi,url,raw_csl_json,created_at,updated_at,deleted_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9,NULL)
             ON CONFLICT(citation_key) DO UPDATE SET title=excluded.title,authors=excluded.authors,
             year=excluded.year,doi=excluded.doi,url=excluded.url,raw_csl_json=excluded.raw_csl_json,
             updated_at=excluded.updated_at,deleted_at=NULL",
            params![
                proposed_id, input.citation_key.trim(), input.title.trim(), input.authors.trim(), input.year,
                input.doi, input.url, serde_json::to_string(&input.raw_csl_json)?, now
            ],
        )?;
        let citation_id: String = transaction.query_row(
            "SELECT id FROM citations WHERE citation_key=?1 COLLATE NOCASE",
            [input.citation_key.trim()],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO block_citations(id,block_id,citation_id,quote_text,locator_json,created_at,deleted_at)
             VALUES(?1,?2,?3,?4,?5,?6,NULL)",
            params![link_id, block_id, citation_id, quote_text, serde_json::to_string(&locator)?, now],
        )?;
        let mut linked_statement = transaction.prepare(
            "SELECT DISTINCT block_id FROM block_citations
             WHERE citation_id=?1 AND deleted_at IS NULL",
        )?;
        let linked_blocks = linked_statement
            .query_map([&citation_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(linked_statement);
        for linked_block_id in linked_blocks {
            refresh_fts(&transaction, &linked_block_id)?;
        }
        journal::append_event(
            &transaction,
            "CitationLinked",
            "Citation",
            &citation_id,
            json!({"blockId": block_id}),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        load_block_citation(&connection, &link_id)
    }

    pub fn citations_for_block(&self, block_id: &str) -> Result<Vec<BlockCitationRecord>> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id FROM block_citations WHERE block_id=?1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )?;
        let ids = statement
            .query_map([block_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter()
            .map(|id| load_block_citation(&connection, id))
            .collect()
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let terms = query
            .split_whitespace()
            .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
            .collect::<Vec<_>>();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let fts_query = terms.join(" AND ");
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT block_id,title,snippet(block_fts,2,'‹','›',' … ',18),bm25(block_fts)
             FROM block_fts WHERE block_fts MATCH ?1 LIMIT ?2",
        )?;
        let rows = statement.query_map(params![fts_query, limit.min(200) as i64], |row| {
            Ok(SearchHit {
                block_id: row.get(0)?,
                title: row.get(1)?,
                excerpt: row.get(2)?,
                rank: row.get(3)?,
            })
        })?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub fn integrity_check(&self) -> Result<IntegrityReport> {
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let revision: i64 = connection.query_row(
            "SELECT revision FROM vault_meta WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        let mut findings = Vec::new();
        for message in db::database_findings(&connection)? {
            findings.push(fatal("database_integrity", message, None));
        }
        for message in journal::verify_journal(&connection, &self.root)? {
            findings.push(fatal("journal_integrity", message, None));
        }
        findings.extend(research_history_findings(&connection)?);

        let mut statement = connection.prepare("SELECT sha256 FROM objects ORDER BY sha256")?;
        let object_hashes = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for hash in object_hashes {
            let path = object_path(&self.root, &hash);
            if !path.is_file() {
                findings.push(fatal(
                    "missing_object",
                    format!("attachment object {hash} is missing"),
                    Some(hash),
                ));
            } else if sha256_file(&path)? != hash {
                findings.push(fatal(
                    "object_hash_mismatch",
                    format!("attachment object {hash} failed SHA-256 verification"),
                    Some(hash),
                ));
            }
        }

        let mut statement = connection.prepare(
            "SELECT e.id,source.title,target.title FROM edges e
             JOIN blocks source ON source.id=e.source_block_id
             JOIN blocks target ON target.id=e.target_block_id
             WHERE e.deleted_at IS NULL AND (source.deleted_at IS NOT NULL OR target.deleted_at IS NOT NULL)",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, source, target) = row?;
            findings.push(warning(
                "dangling_edge",
                format!("Active relation “{source}” → “{target}” references a deleted block"),
                Some(id),
            ));
        }
        drop(statement);

        let mut statement = connection.prepare(
            "SELECT b.id,b.title FROM blocks b
             WHERE b.deleted_at IS NULL AND b.kind='Hypothesis'
             AND NOT EXISTS(SELECT 1 FROM edges e WHERE e.target_block_id=b.id AND e.edge_type='Supports' AND e.deleted_at IS NULL)
             AND NOT EXISTS(SELECT 1 FROM attachments a WHERE a.block_id=b.id AND a.relation_type='Supports' AND a.deleted_at IS NULL)",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, title) = row?;
            findings.push(warning(
                "unsupported_hypothesis",
                format!("“{title}” has no supporting evidence"),
                Some(id),
            ));
        }
        drop(statement);

        let mut statement = connection.prepare(
            "SELECT DISTINCT source.id,source.title,target.title FROM edges e
             JOIN blocks source ON source.id=e.source_block_id
             JOIN blocks target ON target.id=e.target_block_id
             WHERE e.edge_type='Depends on' AND e.deleted_at IS NULL
               AND source.deleted_at IS NULL AND target.deleted_at IS NULL AND target.status='Rejected'",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, source, target) = row?;
            findings.push(warning(
                "depends_on_rejected",
                format!("“{source}” depends on rejected “{target}”"),
                Some(id),
            ));
        }
        drop(statement);

        let mut statement = connection.prepare(
            "SELECT b.id,b.title FROM blocks b WHERE b.deleted_at IS NULL
             AND NOT EXISTS(SELECT 1 FROM edges e WHERE e.deleted_at IS NULL AND (e.source_block_id=b.id OR e.target_block_id=b.id))
             AND NOT EXISTS(SELECT 1 FROM block_lineage l WHERE l.child_block_id=b.id OR l.parent_block_id=b.id)",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, title) = row?;
            findings.push(warning(
                "isolated_block",
                format!("“{title}” is isolated in the research graph"),
                Some(id),
            ));
        }
        drop(statement);

        let mut statement = connection.prepare(
            "SELECT v.id,v.symbol FROM variables v JOIN variable_definitions d ON d.variable_id=v.id
             WHERE v.deleted_at IS NULL AND d.deleted_at IS NULL GROUP BY v.id,v.symbol
             HAVING COUNT(DISTINCT d.normalized_definition) > 1",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, symbol) = row?;
            findings.push(warning(
                "variable_definition_conflict",
                format!("Variable definition conflict: {symbol}"),
                Some(id),
            ));
        }
        drop(statement);

        let mut statement = connection.prepare(
            "SELECT id,citation_key FROM citations WHERE deleted_at IS NULL
             AND (length(trim(title))=0 OR (doi IS NULL AND url IS NULL AND length(trim(raw_csl_json))<=2))",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, key) = row?;
            findings.push(warning(
                "broken_citation",
                format!("Citation {key} has no resolvable DOI, URL, or CSL metadata"),
                Some(id),
            ));
        }
        drop(statement);

        let mut statement =
            connection.prepare("SELECT id,relative_path FROM snapshots ORDER BY created_at")?;
        let snapshot_rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for (id, relative) in snapshot_rows {
            if let Err(error) =
                crate::snapshot::verify_snapshot_directory(&self.root, &self.root.join(relative))
            {
                findings.push(fatal(
                    "snapshot_integrity",
                    format!("Snapshot {id} failed verification: {error}"),
                    Some(id),
                ));
            }
        }

        let backup: Option<(String, String, String, i64)> = connection.query_row(
            "SELECT destination_path,archive_sha256,verified_at,revision FROM backups ORDER BY created_at DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        match &backup {
            None => findings.push(warning(
                "no_external_backup",
                "No verified external backup has been recorded".into(),
                None,
            )),
            Some((path, _, _, _)) if !Path::new(path).is_file() => findings.push(warning(
                "backup_missing",
                "The latest verified backup file is no longer present".into(),
                None,
            )),
            Some((path, expected_hash, _, _))
                if sha256_file(Path::new(path))? != *expected_hash =>
            {
                findings.push(fatal(
                    "backup_hash_mismatch",
                    "The latest verified backup file has changed".into(),
                    None,
                ));
            }
            Some(_) => {}
        }
        if let Some((_, _, verified_at, captured_revision)) = backup {
            let research_changes: i64 = connection.query_row(
                "SELECT COUNT(*) FROM change_events
                 WHERE revision > ?1 AND event_type NOT IN ('BackupCreated','SnapshotCreated','SnapshotRecordRecovered')",
                [captured_revision],
                |row| row.get(0),
            )?;
            let newer_drafts: i64 = connection.query_row(
                "SELECT COUNT(*) FROM block_drafts WHERE updated_at > ?1",
                [verified_at],
                |row| row.get(0),
            )?;
            if research_changes > 0 || newer_drafts > 0 {
                findings.push(warning(
                    "unbacked_changes",
                    format!(
                        "{research_changes} committed change(s) and {newer_drafts} recovery draft(s) are newer than the latest external backup"
                    ),
                    None,
                ));
            }
        }

        let hypothesis_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM blocks WHERE deleted_at IS NULL AND kind='Hypothesis'",
            [],
            |row| row.get(0),
        )?;
        let unhealthy: HashSet<String> = findings
            .iter()
            .filter_map(|finding| {
                if matches!(
                    finding.severity,
                    IntegritySeverity::Warning | IntegritySeverity::Fatal
                ) {
                    finding.entity_id.clone()
                } else {
                    None
                }
            })
            .collect();
        let unhealthy_hypotheses: i64 = if unhealthy.is_empty() {
            0
        } else {
            let mut count = 0;
            for id in unhealthy {
                count += connection.query_row(
                    "SELECT COUNT(*) FROM blocks WHERE id=?1 AND kind='Hypothesis' AND deleted_at IS NULL",
                    [id], |row| row.get::<_, i64>(0)
                )?;
            }
            count
        };
        Ok(IntegrityReport {
            checked_at: utc_now(),
            vault_revision: revision,
            healthy_hypotheses: (hypothesis_count - unhealthy_hypotheses).max(0),
            findings,
        })
    }
}

fn validate_title_reason(title: &str, reason: &str) -> Result<()> {
    if title.trim().is_empty() {
        return Err(SanctumError::InvalidInput("block title is empty".into()));
    }
    if reason.trim().is_empty() {
        return Err(SanctumError::InvalidInput(
            "change reason is required".into(),
        ));
    }
    Ok(())
}

fn snapshot_from_save(input: &SaveBlockInput) -> BlockSnapshot {
    BlockSnapshot {
        id: input.block_id.clone(),
        kind: input.kind,
        title: input.title.trim().to_owned(),
        body_markdown: input.body_markdown.clone(),
        research_notes_markdown: input.research_notes_markdown.clone(),
        status: input.status,
        tags: normalize_tags(input.tags.clone()),
    }
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = tags
        .into_iter()
        .map(|tag| tag.trim().to_owned())
        .filter(|tag| !tag.is_empty())
        .filter(|tag| seen.insert(tag.to_lowercase()))
        .collect::<Vec<_>>();
    output.sort_by_key(|tag| tag.to_lowercase());
    output
}

fn normalize_definition(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn replace_tags(
    transaction: &Transaction<'_>,
    block_id: &str,
    tags: &[String],
    now: &str,
) -> Result<()> {
    transaction.execute("DELETE FROM block_tags WHERE block_id=?1", [block_id])?;
    for tag in tags {
        let proposed_id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO tags(id,name,created_at) VALUES(?1,?2,?3) ON CONFLICT(name) DO NOTHING",
            params![proposed_id, tag, now],
        )?;
        let tag_id: String = transaction.query_row(
            "SELECT id FROM tags WHERE name=?1 COLLATE NOCASE",
            [tag],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO block_tags(block_id,tag_id) VALUES(?1,?2)",
            params![block_id, tag_id],
        )?;
    }
    Ok(())
}

pub(crate) fn refresh_fts(connection: &Connection, block_id: &str) -> Result<()> {
    connection.execute("DELETE FROM block_fts WHERE block_id=?1", [block_id])?;
    let active: Option<(String, String, String)> = connection
        .query_row(
            "SELECT title,body_markdown,research_notes_markdown FROM blocks WHERE id=?1 AND deleted_at IS NULL",
            [block_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((title, body, notes)) = active else {
        return Ok(());
    };
    let tags: String = connection.query_row(
        "SELECT COALESCE(group_concat(t.name,' '),'') FROM block_tags bt JOIN tags t ON t.id=bt.tag_id WHERE bt.block_id=?1",
        [block_id], |row| row.get(0)
    )?;
    let variables: String = connection.query_row(
        "SELECT COALESCE(group_concat(v.symbol || ' ' || d.definition,' '),'')
         FROM variable_definitions d JOIN variables v ON v.id=d.variable_id
         WHERE d.block_id=?1 AND d.deleted_at IS NULL AND v.deleted_at IS NULL",
        [block_id],
        |row| row.get(0),
    )?;
    let citations: String = connection.query_row(
        "SELECT COALESCE(group_concat(c.citation_key || ' ' || c.title || ' ' || c.authors,' '),'')
         FROM block_citations bc JOIN citations c ON c.id=bc.citation_id
         WHERE bc.block_id=?1 AND bc.deleted_at IS NULL AND c.deleted_at IS NULL",
        [block_id],
        |row| row.get(0),
    )?;
    let files: String = connection.query_row(
        "SELECT COALESCE(group_concat(a.display_name || ' ' || a.relation_type || ' ' || COALESCE(o.media_type,''),' '),'')
         FROM attachments a JOIN objects o ON o.sha256=a.object_hash
         WHERE a.block_id=?1 AND a.deleted_at IS NULL",
        [block_id],
        |row| row.get(0),
    )?;
    connection.execute(
        "INSERT INTO block_fts(block_id,title,body_markdown,research_notes_markdown,tags,variables,citation_metadata,file_metadata)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![block_id, title, body, notes, tags, variables, citations, files],
    )?;
    Ok(())
}

fn load_tags(connection: &Connection, block_id: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT t.name FROM block_tags bt JOIN tags t ON t.id=bt.tag_id
         WHERE bt.block_id=?1 ORDER BY lower(t.name)",
    )?;
    let rows = statement.query_map([block_id], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_block(
    connection: &Connection,
    block_id: &str,
    include_deleted: bool,
) -> Result<HypothesisBlock> {
    let sql = if include_deleted {
        "SELECT id,kind,title,body_markdown,research_notes_markdown,status,current_version_id,
                parent_block_id,row_version,created_at,updated_at,deleted_at FROM blocks WHERE id=?1"
    } else {
        "SELECT id,kind,title,body_markdown,research_notes_markdown,status,current_version_id,
                parent_block_id,row_version,created_at,updated_at,deleted_at FROM blocks WHERE id=?1 AND deleted_at IS NULL"
    };
    let row: Option<BlockDatabaseRow> = connection
        .query_row(sql, [block_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
            ))
        })
        .optional()?;
    let (
        id,
        kind,
        title,
        body,
        notes,
        status,
        current_version_id,
        parent,
        row_version,
        created,
        updated,
        deleted,
    ) = row.ok_or_else(|| SanctumError::BlockNotFound(block_id.to_owned()))?;
    Ok(HypothesisBlock {
        snapshot: BlockSnapshot {
            id,
            kind: BlockKind::try_from(kind.as_str()).map_err(SanctumError::Integrity)?,
            title,
            body_markdown: body,
            research_notes_markdown: notes,
            status: BlockStatus::try_from(status.as_str()).map_err(SanctumError::Integrity)?,
            tags: load_tags(connection, block_id)?,
        },
        current_version_id: current_version_id.ok_or_else(|| {
            SanctumError::Integrity(format!("block {block_id} has no current version"))
        })?,
        row_version,
        parent_block_id: parent,
        created_at: created,
        updated_at: updated,
        deleted_at: deleted,
    })
}

fn list_blocks_where(connection: &Connection, condition: &str) -> Result<Vec<HypothesisBlock>> {
    let sql = format!("SELECT id FROM blocks WHERE {condition} ORDER BY updated_at DESC");
    let mut statement = connection.prepare(&sql)?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.iter()
        .map(|id| load_block(connection, id, true))
        .collect()
}

fn version_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BlockVersion> {
    let snapshot_json: String = row.get(4)?;
    let snapshot = serde_json::from_str(&snapshot_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            snapshot_json.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    Ok(BlockVersion {
        id: row.get(0)?,
        block_id: row.get(1)?,
        version_index: row.get(2)?,
        version_label: row.get(3)?,
        snapshot,
        content_sha256: row.get(5)?,
        change_reason: row.get(6)?,
        created_at: row.get(7)?,
    })
}

pub(crate) fn ensure_active_block(connection: &Connection, block_id: &str) -> Result<()> {
    let exists: i64 = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM blocks WHERE id=?1 AND deleted_at IS NULL)",
        [block_id],
        |row| row.get(0),
    )?;
    if exists == 0 {
        Err(SanctumError::BlockNotFound(block_id.to_owned()))
    } else {
        Ok(())
    }
}

fn load_variable(connection: &Connection, variable_id: &str) -> Result<VariableRecord> {
    let symbol: String = connection.query_row(
        "SELECT symbol FROM variables WHERE id=?1 AND deleted_at IS NULL",
        [variable_id],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT id,block_id,definition,formula,created_at FROM variable_definitions
         WHERE variable_id=?1 AND deleted_at IS NULL ORDER BY created_at",
    )?;
    let definitions = statement
        .query_map([variable_id], |row| {
            Ok(VariableDefinitionRecord {
                id: row.get(0)?,
                block_id: row.get(1)?,
                definition: row.get(2)?,
                formula: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let unique = definitions
        .iter()
        .map(|item| normalize_definition(&item.definition))
        .collect::<HashSet<_>>();
    Ok(VariableRecord {
        id: variable_id.to_owned(),
        symbol,
        definitions,
        has_conflict: unique.len() > 1,
    })
}

fn load_block_citation(connection: &Connection, link_id: &str) -> Result<BlockCitationRecord> {
    let row: CitationJoinRow = connection.query_row(
        "SELECT bc.id,bc.block_id,c.id,c.citation_key,c.title,c.authors,c.year,c.doi,c.url,
                c.raw_csl_json,bc.quote_text,bc.locator_json,bc.created_at
         FROM block_citations bc JOIN citations c ON c.id=bc.citation_id
         WHERE bc.id=?1 AND bc.deleted_at IS NULL AND c.deleted_at IS NULL",
        [link_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
            ))
        },
    )?;
    Ok(BlockCitationRecord {
        link_id: row.0,
        block_id: row.1,
        citation: CitationRecord {
            id: row.2,
            citation_key: row.3,
            title: row.4,
            authors: row.5,
            year: row.6,
            doi: row.7,
            url: row.8,
            raw_csl_json: serde_json::from_str(&row.9)?,
        },
        quote_text: row.10,
        locator_json: serde_json::from_str(&row.11)?,
        created_at: row.12,
    })
}

pub(crate) fn object_path(root: &Path, hash: &str) -> PathBuf {
    root.join("objects/sha256")
        .join(&hash[0..2])
        .join(&hash[2..4])
        .join(hash)
}

fn fatal(code: &str, message: String, entity_id: Option<String>) -> IntegrityFinding {
    IntegrityFinding {
        severity: IntegritySeverity::Fatal,
        code: code.into(),
        message,
        entity_id,
    }
}

fn warning(code: &str, message: String, entity_id: Option<String>) -> IntegrityFinding {
    IntegrityFinding {
        severity: IntegritySeverity::Warning,
        code: code.into(),
        message,
        entity_id,
    }
}

pub(crate) fn research_history_findings(connection: &Connection) -> Result<Vec<IntegrityFinding>> {
    let mut findings = Vec::new();
    let mut versions = connection.prepare("SELECT id,block_id,snapshot_json,content_sha256 FROM block_versions ORDER BY block_id,version_index")?;
    let rows = versions.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (version_id, block_id, snapshot_json, expected_hash) = row?;
        let actual_hash = sha256_bytes(snapshot_json.as_bytes());
        if actual_hash != expected_hash {
            findings.push(fatal(
                "version_hash_mismatch",
                format!("immutable version {version_id} failed SHA-256 verification"),
                Some(block_id.clone()),
            ));
        }
        match serde_json::from_str::<BlockSnapshot>(&snapshot_json) {
            Ok(snapshot) if snapshot.id == block_id => {}
            Ok(_) => findings.push(fatal(
                "version_identity_mismatch",
                format!("version {version_id} contains another block identity"),
                Some(block_id.clone()),
            )),
            Err(error) => findings.push(fatal(
                "version_decode_failed",
                format!("version {version_id} cannot be decoded: {error}"),
                Some(block_id.clone()),
            )),
        }
    }
    drop(versions);

    let mut statement = connection.prepare("SELECT id,current_version_id FROM blocks")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let current = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    for (block_id, current_version_id) in current {
        let Some(current_version_id) = current_version_id else {
            findings.push(fatal(
                "missing_current_version",
                format!("block {block_id} has no current version"),
                Some(block_id),
            ));
            continue;
        };
        let snapshot_json: Option<String> = connection
            .query_row(
                "SELECT snapshot_json FROM block_versions WHERE id=?1 AND block_id=?2",
                params![current_version_id, block_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(snapshot_json) = snapshot_json else {
            findings.push(fatal(
                "invalid_current_version",
                format!("block {block_id} points to a missing or foreign version"),
                Some(block_id),
            ));
            continue;
        };
        let version_snapshot: BlockSnapshot = match serde_json::from_str(&snapshot_json) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let projected = load_block(connection, &block_id, true)?.snapshot;
        if projected != version_snapshot {
            findings.push(fatal(
                "current_projection_mismatch",
                format!("block {block_id} differs from its immutable current version"),
                Some(block_id),
            ));
        }
    }
    Ok(findings)
}

pub(crate) fn assert_research_history_integrity(connection: &Connection) -> Result<()> {
    if let Some(finding) = research_history_findings(connection)?.first() {
        return Err(SanctumError::Integrity(finding.message.clone()));
    }
    Ok(())
}

pub(crate) fn verify_vault_database(
    connection: &Connection,
    expected_vault_id: &str,
    expected_revision: i64,
) -> Result<()> {
    db::assert_database_integrity(connection)?;
    let (vault_id, revision): (String, i64) = connection.query_row(
        "SELECT vault_id,revision FROM vault_meta WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if vault_id != expected_vault_id || revision != expected_revision {
        return Err(SanctumError::Integrity(
            "recovery database identity or revision differs from its manifest".into(),
        ));
    }
    assert_research_history_integrity(connection)
}
