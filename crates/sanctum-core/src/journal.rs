use crate::error::{Result, SanctumError};
use crate::{atomic_write, sha256_bytes, sync_directory, utc_now};
use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JournalEvent {
    pub revision: i64,
    pub event_id: String,
    pub event_type: String,
    pub entity_type: String,
    pub entity_id: String,
    pub payload: Value,
    pub previous_event_hash: Option<String>,
    pub event_hash: String,
    pub created_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventHashMaterial<'a> {
    revision: i64,
    event_id: &'a str,
    event_type: &'a str,
    entity_type: &'a str,
    entity_id: &'a str,
    payload: &'a Value,
    previous_event_hash: &'a Option<String>,
    created_at: &'a str,
}

pub(crate) fn append_event(
    transaction: &Transaction<'_>,
    event_type: &str,
    entity_type: &str,
    entity_id: &str,
    payload: Value,
) -> Result<i64> {
    let revision: i64 = transaction.query_row(
        "UPDATE vault_meta SET revision = revision + 1 WHERE singleton = 1 RETURNING revision",
        [],
        |row| row.get(0),
    )?;
    let previous_event_hash: Option<String> = if revision == 1 {
        None
    } else {
        transaction.query_row(
            "SELECT event_hash FROM change_events WHERE revision = ?1",
            [revision - 1],
            |row| row.get(0),
        )?
    };
    let event_id = Uuid::new_v4().to_string();
    let created_at = utc_now();
    let material = EventHashMaterial {
        revision,
        event_id: &event_id,
        event_type,
        entity_type,
        entity_id,
        payload: &payload,
        previous_event_hash: &previous_event_hash,
        created_at: &created_at,
    };
    let event_hash = sha256_bytes(&serde_json::to_vec(&material)?);
    let event = JournalEvent {
        revision,
        event_id: event_id.clone(),
        event_type: event_type.to_owned(),
        entity_type: entity_type.to_owned(),
        entity_id: entity_id.to_owned(),
        payload: payload.clone(),
        previous_event_hash: previous_event_hash.clone(),
        event_hash: event_hash.clone(),
        created_at: created_at.clone(),
    };
    let event_json = serde_json::to_string_pretty(&event)?;
    transaction.execute(
        "INSERT INTO change_events(
            revision,event_id,event_type,entity_type,entity_id,payload_json,
            previous_event_hash,event_hash,created_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            revision,
            event_id,
            event_type,
            entity_type,
            entity_id,
            serde_json::to_string(&payload)?,
            previous_event_hash,
            event_hash,
            created_at
        ],
    )?;
    transaction.execute(
        "INSERT INTO journal_outbox(revision,event_json,dispatched_at) VALUES(?1,?2,NULL)",
        params![revision, event_json],
    )?;
    Ok(revision)
}

pub(crate) fn drain_outbox(connection: &mut Connection, root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("journal"))?;
    loop {
        let next: Option<(i64, String)> = match connection.query_row(
            "SELECT revision,event_json FROM journal_outbox
             WHERE dispatched_at IS NULL ORDER BY revision LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ) {
            Ok(value) => Some(value),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(error.into()),
        };
        let Some((revision, event_json)) = next else {
            break;
        };
        let path = journal_path(root, revision);
        if path.exists() {
            let existing = fs::read_to_string(&path)?;
            let expected: JournalEvent = serde_json::from_str(&event_json)?;
            let actual: JournalEvent = serde_json::from_str(&existing)?;
            if actual != expected {
                return Err(SanctumError::Integrity(format!(
                    "journal file {} disagrees with database outbox",
                    path.display()
                )));
            }
        } else {
            atomic_write(&path, event_json.as_bytes())?;
            sync_directory(path.parent().expect("journal parent"))?;
        }
        connection.execute(
            "UPDATE journal_outbox SET dispatched_at = ?1 WHERE revision = ?2 AND dispatched_at IS NULL",
            params![utc_now(), revision],
        )?;
    }
    Ok(())
}

pub(crate) fn verify_journal(connection: &Connection, root: &Path) -> Result<Vec<String>> {
    let mut findings = Vec::new();
    let revision: i64 = connection.query_row(
        "SELECT revision FROM vault_meta WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    let mut previous: Option<String> = None;
    for number in 1..=revision {
        let path = journal_path(root, number);
        if !path.is_file() {
            findings.push(format!("journal entry {number} is missing"));
            continue;
        }
        let event: JournalEvent = match serde_json::from_slice(&fs::read(&path)?) {
            Ok(event) => event,
            Err(error) => {
                findings.push(format!("journal entry {number} cannot be decoded: {error}"));
                continue;
            }
        };
        if event.revision != number {
            findings.push(format!(
                "journal entry {number} has revision {}",
                event.revision
            ));
        }
        if event.previous_event_hash != previous {
            findings.push(format!("journal hash chain breaks at revision {number}"));
        }
        let material = EventHashMaterial {
            revision: event.revision,
            event_id: &event.event_id,
            event_type: &event.event_type,
            entity_type: &event.entity_type,
            entity_id: &event.entity_id,
            payload: &event.payload,
            previous_event_hash: &event.previous_event_hash,
            created_at: &event.created_at,
        };
        let calculated = sha256_bytes(&serde_json::to_vec(&material)?);
        if event.event_hash != calculated {
            findings.push(format!("journal hash mismatch at revision {number}"));
        }
        let db_hash: Option<String> = match connection.query_row(
            "SELECT event_hash FROM change_events WHERE revision = ?1",
            [number],
            |row| row.get(0),
        ) {
            Ok(value) => Some(value),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(error.into()),
        };
        if db_hash.as_deref() != Some(event.event_hash.as_str()) {
            findings.push(format!("journal/database mismatch at revision {number}"));
        }
        previous = Some(event.event_hash);
    }
    Ok(findings)
}

pub(crate) fn rebuild_journal(connection: &Connection, root: &Path) -> Result<()> {
    let journal_dir = root.join("journal");
    fs::create_dir_all(&journal_dir)?;
    let mut statement = connection.prepare(
        "SELECT revision,event_id,event_type,entity_type,entity_id,payload_json,
                previous_event_hash,event_hash,created_at
         FROM change_events ORDER BY revision",
    )?;
    let rows = statement.query_map([], |row| {
        let payload_json: String = row.get(5)?;
        Ok(JournalEvent {
            revision: row.get(0)?,
            event_id: row.get(1)?,
            event_type: row.get(2)?,
            entity_type: row.get(3)?,
            entity_id: row.get(4)?,
            payload: serde_json::from_str(&payload_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    payload_json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
            previous_event_hash: row.get(6)?,
            event_hash: row.get(7)?,
            created_at: row.get(8)?,
        })
    })?;
    for row in rows {
        let event = row?;
        atomic_write(
            &journal_path(root, event.revision),
            serde_json::to_string_pretty(&event)?.as_bytes(),
        )?;
    }
    sync_directory(&journal_dir)?;
    Ok(())
}

fn journal_path(root: &Path, revision: i64) -> std::path::PathBuf {
    root.join("journal").join(format!("{revision:020}.json"))
}
