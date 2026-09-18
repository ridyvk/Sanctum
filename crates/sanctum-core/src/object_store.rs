use crate::domain::{Attachment, AttachmentRelation};
use crate::error::{Result, SanctumError};
use crate::journal;
use crate::vault::{ensure_active_block, object_path, refresh_fts};
use crate::{rename_noreplace, sha256_file, sync_directory, sync_file, utc_now, Vault};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use uuid::Uuid;

const COPY_BUFFER: usize = 1024 * 1024;

impl Vault {
    pub fn attach_file(
        &self,
        block_id: &str,
        source: impl AsRef<Path>,
        relation: AttachmentRelation,
        locator: Value,
    ) -> Result<Attachment> {
        let source = source.as_ref();
        if !source.is_file() {
            return Err(SanctumError::InvalidInput(format!(
                "attachment source is not a regular file: {}",
                source.display()
            )));
        }
        let metadata = source.metadata()?;
        let available = fs2::available_space(self.root.join("staging"))?;
        if available
            < metadata
                .len()
                .saturating_mul(2)
                .saturating_add(16 * 1024 * 1024)
        {
            return Err(SanctumError::Io(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "not enough free space to stage and verify attachment",
            )));
        }
        let stage_path = self
            .root
            .join("staging")
            .join(format!("attachment-{}.partial", Uuid::new_v4()));
        let copy_result = (|| -> Result<String> {
            let mut input = File::open(source)?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&stage_path)?;
            let mut buffer = vec![0u8; COPY_BUFFER];
            loop {
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count])?;
            }
            output.sync_all()?;
            drop(output);
            let staged_size = stage_path.metadata()?.len();
            if staged_size != metadata.len() {
                return Err(SanctumError::Integrity(
                    "attachment size changed while copying".into(),
                ));
            }
            sha256_file(&stage_path)
        })();
        let hash = match copy_result {
            Ok(hash) => hash,
            Err(error) => {
                let _ = fs::rename(
                    &stage_path,
                    self.root
                        .join("quarantine")
                        .join(format!("failed-attachment-{}", Uuid::new_v4())),
                );
                return Err(error);
            }
        };
        let target = object_path(&self.root, &hash);
        if target.exists() {
            if !target.is_file() || sha256_file(&target)? != hash {
                let quarantine = self
                    .root
                    .join("quarantine")
                    .join(format!("cas-conflict-{hash}-{}", Uuid::new_v4()));
                fs::rename(&stage_path, quarantine)?;
                return Err(SanctumError::Integrity(format!(
                    "existing CAS object {hash} is invalid"
                )));
            }
            fs::remove_file(&stage_path)?;
        } else {
            let parent = target.parent().expect("object parent");
            fs::create_dir_all(parent)?;
            if let Err(error) = rename_noreplace(&stage_path, &target) {
                if matches!(error, SanctumError::RefuseOverwrite(_))
                    && target.is_file()
                    && sha256_file(&target)? == hash
                {
                    fs::remove_file(&stage_path)?;
                } else {
                    return Err(error);
                }
            }
            sync_file(&target)?;
            sync_directory(parent)?;
        }

        let id = Uuid::new_v4().to_string();
        let now = utc_now();
        let display_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("attachment")
            .to_owned();
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_lowercase);
        let media_type = media_type_for(extension.as_deref()).map(str::to_owned);
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        ensure_active_block(&transaction, block_id)?;
        transaction.execute(
            "INSERT INTO objects(sha256,byte_size,media_type,original_extension,created_at,last_verified_at)
             VALUES(?1,?2,?3,?4,?5,?5) ON CONFLICT(sha256) DO UPDATE SET last_verified_at=excluded.last_verified_at",
            params![hash, metadata.len() as i64, media_type, extension, now],
        )?;
        transaction.execute(
            "INSERT INTO attachments(id,block_id,object_hash,relation_type,display_name,locator_json,created_at,deleted_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,NULL)",
            params![id, block_id, hash, relation.as_str(), display_name, serde_json::to_string(&locator)?, now],
        )?;
        refresh_fts(&transaction, block_id)?;
        journal::append_event(
            &transaction,
            "FileAttached",
            "Attachment",
            &id,
            json!({
                "blockId": block_id, "objectHash": hash, "relationType": relation.as_str()
            }),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        Ok(Attachment {
            id,
            block_id: block_id.to_owned(),
            object_hash: hash,
            relation_type: relation,
            display_name,
            media_type,
            byte_size: metadata.len() as i64,
            locator_json: locator,
            created_at: now,
            deleted_at: None,
        })
    }

    pub fn attachments_for_block(&self, block_id: &str) -> Result<Vec<Attachment>> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT a.id,a.block_id,a.object_hash,a.relation_type,a.display_name,o.media_type,o.byte_size,
                    a.locator_json,a.created_at,a.deleted_at
             FROM attachments a JOIN objects o ON o.sha256=a.object_hash
             WHERE a.block_id=?1 AND a.deleted_at IS NULL ORDER BY a.created_at DESC",
        )?;
        let rows = statement.query_map([block_id], |row| {
            let relation: String = row.get(3)?;
            let locator: String = row.get(7)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                relation,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
                locator,
                row.get::<_, String>(8)?,
                row.get::<_, Option<String>>(9)?,
            ))
        })?;
        let mut attachments = Vec::new();
        for row in rows {
            let (id, block_id, hash, relation, name, media, size, locator, created, deleted) = row?;
            attachments.push(Attachment {
                id,
                block_id,
                object_hash: hash,
                relation_type: AttachmentRelation::try_from(relation.as_str())
                    .map_err(SanctumError::Integrity)?,
                display_name: name,
                media_type: media,
                byte_size: size,
                locator_json: serde_json::from_str(&locator)?,
                created_at: created,
                deleted_at: deleted,
            });
        }
        Ok(attachments)
    }

    pub fn attachment(&self, attachment_id: &str) -> Result<Attachment> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let row = connection
            .query_row(
                "SELECT a.id,a.block_id,a.object_hash,a.relation_type,a.display_name,o.media_type,o.byte_size,
                        a.locator_json,a.created_at,a.deleted_at
                 FROM attachments a JOIN objects o ON o.sha256=a.object_hash
                 WHERE a.id=?1 AND a.deleted_at IS NULL",
                [attachment_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, Option<String>>(9)?,
                    ))
                },
            )
            .optional()?;
        let (id, block_id, hash, relation, name, media, size, locator, created, deleted) = row
            .ok_or_else(|| SanctumError::InvalidInput("active attachment not found".into()))?;
        Ok(Attachment {
            id,
            block_id,
            object_hash: hash,
            relation_type: AttachmentRelation::try_from(relation.as_str())
                .map_err(SanctumError::Integrity)?,
            display_name: name,
            media_type: media,
            byte_size: size,
            locator_json: serde_json::from_str(&locator)?,
            created_at: created,
            deleted_at: deleted,
        })
    }

    pub fn attachment_object_path(&self, attachment_id: &str) -> Result<std::path::PathBuf> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let hash: Option<String> = connection
            .query_row(
                "SELECT object_hash FROM attachments WHERE id=?1 AND deleted_at IS NULL",
                [attachment_id],
                |row| row.get(0),
            )
            .optional()?;
        let hash =
            hash.ok_or_else(|| SanctumError::InvalidInput("active attachment not found".into()))?;
        let path = object_path(&self.root, &hash);
        if !path.is_file() || sha256_file(&path)? != hash {
            return Err(SanctumError::Integrity(format!(
                "attachment object {hash} is missing or corrupt"
            )));
        }
        Ok(path)
    }

    pub fn soft_delete_attachment(&self, attachment_id: &str) -> Result<()> {
        let now = utc_now();
        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        let transaction = connection.transaction()?;
        let block_id: Option<String> = transaction.query_row(
            "UPDATE attachments SET deleted_at=?1 WHERE id=?2 AND deleted_at IS NULL RETURNING block_id",
            params![now, attachment_id], |row| row.get(0)
        ).optional()?;
        let block_id = block_id
            .ok_or_else(|| SanctumError::InvalidInput("active attachment not found".into()))?;
        refresh_fts(&transaction, &block_id)?;
        journal::append_event(
            &transaction,
            "AttachmentSoftDeleted",
            "Attachment",
            attachment_id,
            json!({"blockId": block_id}),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        Ok(())
    }
}

fn media_type_for(extension: Option<&str>) -> Option<&'static str> {
    match extension {
        Some("pdf") => Some("application/pdf"),
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("webp") => Some("image/webp"),
        Some("csv") => Some("text/csv"),
        Some("json") => Some("application/json"),
        Some("txt") => Some("text/plain"),
        Some("md" | "markdown") => Some("text/markdown"),
        Some("py") => Some("text/x-python"),
        Some("r") => Some("text/x-r-source"),
        _ => None,
    }
}
