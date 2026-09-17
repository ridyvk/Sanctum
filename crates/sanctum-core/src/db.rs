use crate::error::{Result, SanctumError};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 1;
pub const APPLICATION_ID: i64 = 1_396_788_803;
const MIGRATION_1: &str = include_str!("../migrations/0001_initial.sql");

pub(crate) fn open_existing(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        return Err(SanctumError::VaultMissing(path.to_path_buf()));
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let application_id: i64 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if application_id != APPLICATION_ID {
        return Err(SanctumError::Integrity(
            "database is not a recognized Sanctum database; it was left untouched".into(),
        ));
    }
    configure(&connection)?;
    Ok(connection)
}

pub(crate) fn create_new(path: &Path) -> Result<Connection> {
    if path.exists() {
        return Err(SanctumError::VaultExists(path.to_path_buf()));
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure(&connection)?;
    Ok(connection)
}

pub(crate) fn configure(connection: &Connection) -> Result<()> {
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    let mode: String = connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(SanctumError::Integrity(format!(
            "SQLite refused WAL mode and returned {mode}"
        )));
    }
    Ok(())
}

pub(crate) fn initialize_or_migrate(connection: &mut Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            checksum TEXT NOT NULL,
            applied_at TEXT NOT NULL
        ) STRICT;",
    )?;

    let checksum = hex::encode(Sha256::digest(MIGRATION_1.as_bytes()));
    let installed: Option<String> = connection
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE version = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(installed) = installed {
        if installed != checksum {
            return Err(SanctumError::Integrity(
                "migration 1 checksum differs from the installed schema".into(),
            ));
        }
    } else {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_1)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES(1, ?1, ?2)",
            rusqlite::params![checksum, crate::utc_now()],
        )?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
    }

    let current: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current != SCHEMA_VERSION {
        return Err(SanctumError::UnsupportedFormat(format!(
            "database schema {current}; this build supports {SCHEMA_VERSION}"
        )));
    }
    Ok(())
}

pub(crate) fn database_findings(connection: &Connection) -> Result<Vec<String>> {
    let mut findings = Vec::new();
    let quick: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if quick != "ok" {
        findings.push(format!("SQLite quick_check: {quick}"));
    }
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    let rows = statement.query_map([], |row| {
        Ok(format!(
            "foreign key violation in {} row {} referencing {}",
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?
        ))
    })?;
    for row in rows {
        findings.push(row?);
    }
    Ok(findings)
}

pub(crate) fn assert_database_integrity(connection: &Connection) -> Result<()> {
    let findings = database_findings(connection)?;
    if let Some(first) = findings.first() {
        return Err(SanctumError::Integrity(first.clone()));
    }
    Ok(())
}

trait OptionalRow<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}

impl<T> OptionalRow<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }
}
