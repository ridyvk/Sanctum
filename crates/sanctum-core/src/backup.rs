use crate::db;
use crate::domain::{BackupRecord, VaultManifest};
use crate::error::{Result, SanctumError};
use crate::journal;
use crate::snapshot::backup_database;
use crate::vault::{assert_research_history_integrity, object_path, verify_vault_database};
use crate::{
    atomic_write, rename_noreplace, sha256_file, sync_directory, sync_file, sync_tree_directories,
    utc_now, Vault,
};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::rngs::OsRng;
use rand::RngCore;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"SANCTM01";
const CHUNK_SIZE: usize = 1024 * 1024;
const MAX_HEADER_SIZE: u32 = 1024 * 1024;
const MAX_MANIFEST_SIZE: u64 = 4 * 1024 * 1024;
const MAX_ARCHIVE_EXPANSION: u64 = 512 * 1024 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncryptionHeader {
    format_version: u32,
    kdf: String,
    salt_hex: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    cipher: String,
    base_nonce_hex: String,
    chunk_size: u32,
    plaintext_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchivedFile {
    path: String,
    sha256: String,
    byte_size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    format_version: u32,
    backup_id: String,
    vault_id: String,
    schema_version: i64,
    revision: i64,
    created_at: String,
    vault_manifest: ArchivedFile,
    database: ArchivedFile,
    objects: Vec<ArchivedFile>,
    journal: Vec<ArchivedFile>,
}

impl Vault {
    pub fn create_encrypted_backup(
        &self,
        destination: impl AsRef<Path>,
        password: &str,
    ) -> Result<BackupRecord> {
        if password.chars().count() < 12 {
            return Err(SanctumError::InvalidInput(
                "backup password must contain at least 12 characters".into(),
            ));
        }
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(SanctumError::RefuseOverwrite(destination.to_path_buf()));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| SanctumError::InvalidInput("backup path has no parent".into()))?;
        fs::create_dir_all(parent)?;
        let canonical_parent = parent.canonicalize()?;
        let canonical_root = self.root.canonicalize()?;
        if canonical_parent.starts_with(&canonical_root) {
            return Err(SanctumError::InvalidInput(
                "external backup must be stored outside the live Vault".into(),
            ));
        }

        let backup_id = Uuid::new_v4().to_string();
        let created_at = utc_now();
        let temporary = tempfile::tempdir_in(parent)?;
        let package_root = temporary.path().join("package");
        for directory in ["db", "objects/sha256", "journal"] {
            fs::create_dir_all(package_root.join(directory))?;
        }
        let compressed_archive = temporary.path().join("archive.tar.zst");

        let mut connection = self.connection.lock().expect("vault mutex poisoned");
        journal::drain_outbox(&mut connection, &self.root)?;
        db::assert_database_integrity(&connection)?;
        assert_research_history_integrity(&connection)?;
        if let Some(first) = journal::verify_journal(&connection, &self.root)?.first() {
            return Err(SanctumError::Integrity(first.clone()));
        }
        let (vault_id, revision): (String, i64) = connection.query_row(
            "SELECT vault_id,revision FROM vault_meta WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let database = package_root.join("db/sanctum.sqlite");
        backup_database(&connection, &database, &vault_id, revision)?;

        let live_manifest = self.root.join("manifest.json");
        let archived_manifest = package_root.join("manifest.json");
        fs::copy(&live_manifest, &archived_manifest)?;
        sync_file(&archived_manifest)?;

        let mut object_files = Vec::new();
        let mut statement =
            connection.prepare("SELECT sha256,byte_size FROM objects ORDER BY sha256")?;
        let objects = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for (hash, expected_size) in objects {
            let source = object_path(&self.root, &hash);
            if !source.is_file()
                || source.metadata()?.len() != expected_size as u64
                || sha256_file(&source)? != hash
            {
                return Err(SanctumError::Integrity(format!(
                    "cannot back up missing or corrupt object {hash}"
                )));
            }
            let relative = format!("objects/sha256/{}/{}/{}", &hash[0..2], &hash[2..4], hash);
            let target = package_root.join(&relative);
            fs::create_dir_all(target.parent().expect("object parent"))?;
            fs::copy(&source, &target)?;
            sync_file(&target)?;
            object_files.push(ArchivedFile {
                path: relative,
                sha256: hash,
                byte_size: expected_size as u64,
            });
        }

        let mut journal_files = Vec::new();
        for number in 1..=revision {
            let relative = format!("journal/{number:020}.json");
            let source = self.root.join(&relative);
            if !source.is_file() {
                return Err(SanctumError::Integrity(format!(
                    "journal entry {number} is missing"
                )));
            }
            let target = package_root.join(&relative);
            fs::copy(&source, &target)?;
            sync_file(&target)?;
            journal_files.push(archived_file(&target, relative)?);
        }
        let manifest_file = archived_file(&archived_manifest, "manifest.json".into())?;
        let database_file = archived_file(&database, "db/sanctum.sqlite".into())?;
        let backup_manifest = BackupManifest {
            format_version: 1,
            backup_id: backup_id.clone(),
            vault_id: vault_id.clone(),
            schema_version: db::SCHEMA_VERSION,
            revision,
            created_at: created_at.clone(),
            vault_manifest: manifest_file,
            database: database_file,
            objects: object_files,
            journal: journal_files,
        };
        atomic_write(
            &package_root.join("backup-manifest.json"),
            serde_json::to_string_pretty(&backup_manifest)?.as_bytes(),
        )?;
        build_archive(&package_root, &backup_manifest, &compressed_archive)?;

        let partial = canonical_parent.join(format!(
            ".{}.{}.partial",
            destination
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("sanctum-backup"),
            Uuid::new_v4()
        ));
        encrypt_file(&compressed_archive, &partial, password)?;
        let verify_dir = temporary.path().join("readback");
        fs::create_dir(&verify_dir)?;
        extract_and_verify(&partial, password, &verify_dir)?;
        let archive_sha256 = sha256_file(&partial)?;
        let byte_size = partial.metadata()?.len() as i64;
        rename_noreplace(&partial, destination)?;
        sync_file(destination)?;
        sync_directory(&canonical_parent)?;
        let verified_at = utc_now();
        let destination_string = destination.to_string_lossy().into_owned();
        let file_name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("backup.sanctum-backup")
            .to_owned();
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO backups(id,revision,file_name,destination_path,archive_sha256,byte_size,created_at,verified_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![backup_id, revision, file_name, destination_string, archive_sha256, byte_size, created_at, verified_at],
        )?;
        journal::append_event(
            &transaction,
            "BackupCreated",
            "Backup",
            &backup_id,
            json!({
                "capturedRevision": revision, "archiveSha256": archive_sha256, "byteSize": byte_size
            }),
        )?;
        transaction.commit()?;
        journal::drain_outbox(&mut connection, &self.root)?;
        Ok(BackupRecord {
            id: backup_id,
            revision,
            file_name,
            destination_path: destination_string,
            archive_sha256,
            byte_size,
            created_at,
            verified_at,
        })
    }

    pub fn backups(&self) -> Result<Vec<BackupRecord>> {
        let connection = self.connection.lock().expect("vault mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id,revision,file_name,destination_path,archive_sha256,byte_size,created_at,verified_at
             FROM backups ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(BackupRecord {
                id: row.get(0)?,
                revision: row.get(1)?,
                file_name: row.get(2)?,
                destination_path: row.get(3)?,
                archive_sha256: row.get(4)?,
                byte_size: row.get(5)?,
                created_at: row.get(6)?,
                verified_at: row.get(7)?,
            })
        })?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub fn verify_encrypted_backup(archive: impl AsRef<Path>, password: &str) -> Result<()> {
        let temporary = tempfile::tempdir()?;
        extract_and_verify(archive.as_ref(), password, temporary.path()).map(|_| ())
    }

    pub fn restore_encrypted_backup_to(
        archive: impl AsRef<Path>,
        password: &str,
        destination: impl AsRef<Path>,
    ) -> Result<PathBuf> {
        let archive = archive.as_ref();
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(SanctumError::RefuseOverwrite(destination.to_path_buf()));
        }
        let parent = destination.parent().ok_or_else(|| {
            SanctumError::InvalidInput("restore destination has no parent".into())
        })?;
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
            extract_and_verify(archive, password, &stage)?;
            for directory in ["snapshots", "staging", "quarantine"] {
                fs::create_dir_all(stage.join(directory))?;
            }
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
}

fn archived_file(path: &Path, archive_path: String) -> Result<ArchivedFile> {
    Ok(ArchivedFile {
        path: archive_path,
        sha256: sha256_file(path)?,
        byte_size: path.metadata()?.len(),
    })
}

fn build_archive(package_root: &Path, manifest: &BackupManifest, destination: &Path) -> Result<()> {
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let encoder = zstd::Encoder::new(output, 9)?;
    let mut builder = tar::Builder::new(encoder);
    builder.mode(tar::HeaderMode::Deterministic);
    builder.append_path_with_name(
        package_root.join("backup-manifest.json"),
        "backup-manifest.json",
    )?;
    builder.append_path_with_name(
        package_root.join(&manifest.vault_manifest.path),
        &manifest.vault_manifest.path,
    )?;
    builder.append_path_with_name(
        package_root.join(&manifest.database.path),
        &manifest.database.path,
    )?;
    for file in manifest.objects.iter().chain(manifest.journal.iter()) {
        builder.append_path_with_name(package_root.join(&file.path), &file.path)?;
    }
    let encoder = builder.into_inner()?;
    let output = encoder.finish()?;
    output.sync_all()?;
    Ok(())
}

fn encrypt_file(source: &Path, destination: &Path, password: &str) -> Result<()> {
    let plaintext_size = source.metadata()?.len();
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce[..16]);
    nonce[16..].fill(0);
    let header = EncryptionHeader {
        format_version: 1,
        kdf: "Argon2id".into(),
        salt_hex: hex::encode(salt),
        memory_kib: 65_536,
        iterations: 3,
        parallelism: 1,
        cipher: "XChaCha20-Poly1305".into(),
        base_nonce_hex: hex::encode(nonce),
        chunk_size: CHUNK_SIZE as u32,
        plaintext_size,
    };
    let header_bytes = serde_json::to_vec(&header)?;
    if header_bytes.len() > MAX_HEADER_SIZE as usize {
        return Err(SanctumError::InvalidInput(
            "encryption header is too large".into(),
        ));
    }
    let key = derive_key(password, &header, &salt)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    output.write_all(MAGIC)?;
    output.write_all(&(header_bytes.len() as u32).to_le_bytes())?;
    output.write_all(&header_bytes)?;
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut index = 0u64;
    loop {
        let mut count = 0usize;
        while count < buffer.len() {
            let read = input.read(&mut buffer[count..])?;
            if read == 0 {
                break;
            }
            count += read;
        }
        if count == 0 {
            break;
        }
        let current_nonce = nonce_for(nonce, index);
        let aad = associated_data(&header_bytes, index);
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&current_nonce),
                Payload {
                    msg: &buffer[..count],
                    aad: &aad,
                },
            )
            .map_err(|_| SanctumError::Authentication)?;
        output.write_all(&(ciphertext.len() as u32).to_le_bytes())?;
        output.write_all(&ciphertext)?;
        index = index
            .checked_add(1)
            .ok_or_else(|| SanctumError::InvalidInput("backup has too many chunks".into()))?;
    }
    output.sync_all()?;
    Ok(())
}

fn decrypt_file(source: &Path, destination: &Path, password: &str) -> Result<()> {
    let mut input = File::open(source)?;
    let mut magic = [0u8; 8];
    input
        .read_exact(&mut magic)
        .map_err(|_| SanctumError::Authentication)?;
    if &magic != MAGIC {
        return Err(SanctumError::UnsupportedFormat(
            "not a Sanctum backup".into(),
        ));
    }
    let mut length_bytes = [0u8; 4];
    input
        .read_exact(&mut length_bytes)
        .map_err(|_| SanctumError::Authentication)?;
    let header_length = u32::from_le_bytes(length_bytes);
    if header_length == 0 || header_length > MAX_HEADER_SIZE {
        return Err(SanctumError::UnsupportedFormat(
            "invalid backup header length".into(),
        ));
    }
    let mut header_bytes = vec![0u8; header_length as usize];
    input
        .read_exact(&mut header_bytes)
        .map_err(|_| SanctumError::Authentication)?;
    let header: EncryptionHeader =
        serde_json::from_slice(&header_bytes).map_err(|_| SanctumError::Authentication)?;
    validate_encryption_header(&header)?;
    let salt = decode_fixed::<16>(&header.salt_hex)?;
    let base_nonce = decode_fixed::<24>(&header.base_nonce_hex)?;
    let parent = destination
        .parent()
        .ok_or_else(|| SanctumError::InvalidInput("decrypt destination has no parent".into()))?;
    if fs2::available_space(parent)? < header.plaintext_size.saturating_add(64 * 1024 * 1024) {
        return Err(SanctumError::Io(std::io::Error::new(
            std::io::ErrorKind::StorageFull,
            "not enough free space to decrypt backup",
        )));
    }
    let key = derive_key(password, &header, &salt)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut written = 0u64;
    let mut index = 0u64;
    while written < header.plaintext_size {
        input
            .read_exact(&mut length_bytes)
            .map_err(|_| SanctumError::Authentication)?;
        let encrypted_length = u32::from_le_bytes(length_bytes) as usize;
        if encrypted_length < 16 || encrypted_length > header.chunk_size as usize + 16 {
            return Err(SanctumError::Authentication);
        }
        let mut ciphertext = vec![0u8; encrypted_length];
        input
            .read_exact(&mut ciphertext)
            .map_err(|_| SanctumError::Authentication)?;
        let nonce = nonce_for(base_nonce, index);
        let aad = associated_data(&header_bytes, index);
        let plaintext = cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| SanctumError::Authentication)?;
        if written.saturating_add(plaintext.len() as u64) > header.plaintext_size {
            return Err(SanctumError::Authentication);
        }
        output.write_all(&plaintext)?;
        written += plaintext.len() as u64;
        index += 1;
    }
    let mut extra = [0u8; 1];
    if input.read(&mut extra)? != 0 {
        return Err(SanctumError::Authentication);
    }
    output.sync_all()?;
    Ok(())
}

fn derive_key(
    password: &str,
    header: &EncryptionHeader,
    salt: &[u8; 16],
) -> Result<Zeroizing<[u8; 32]>> {
    let parameters = Params::new(
        header.memory_kib,
        header.iterations,
        header.parallelism,
        Some(32),
    )
    .map_err(|error| SanctumError::InvalidInput(format!("invalid Argon2 parameters: {error}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, parameters);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|_| SanctumError::Authentication)?;
    Ok(key)
}

fn validate_encryption_header(header: &EncryptionHeader) -> Result<()> {
    if header.format_version != 1
        || header.kdf != "Argon2id"
        || header.cipher != "XChaCha20-Poly1305"
        || header.memory_kib < 16_384
        || header.memory_kib > 1_048_576
        || header.iterations == 0
        || header.iterations > 16
        || header.parallelism == 0
        || header.parallelism > 16
        || header.chunk_size == 0
        || header.chunk_size as usize > 16 * 1024 * 1024
        || header.plaintext_size > MAX_ARCHIVE_EXPANSION
    {
        return Err(SanctumError::UnsupportedFormat(
            "unsafe or unsupported backup parameters".into(),
        ));
    }
    Ok(())
}

fn decode_fixed<const N: usize>(value: &str) -> Result<[u8; N]> {
    let decoded = hex::decode(value).map_err(|_| SanctumError::Authentication)?;
    decoded.try_into().map_err(|_| SanctumError::Authentication)
}

fn nonce_for(mut base: [u8; 24], index: u64) -> [u8; 24] {
    base[16..24].copy_from_slice(&index.to_le_bytes());
    base
}

fn associated_data(header: &[u8], index: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(header.len() + 8);
    data.extend_from_slice(header);
    data.extend_from_slice(&index.to_le_bytes());
    data
}

fn extract_and_verify(
    archive: &Path,
    password: &str,
    destination: &Path,
) -> Result<BackupManifest> {
    if !archive.is_file() {
        return Err(SanctumError::InvalidInput("backup file is missing".into()));
    }
    if !destination.is_dir() || destination.read_dir()?.next().is_some() {
        return Err(SanctumError::InvalidInput(
            "backup extraction destination must be an empty directory".into(),
        ));
    }
    let temporary = tempfile::tempdir_in(destination.parent().unwrap_or(destination))?;
    let compressed = temporary.path().join("decrypted.tar.zst");
    decrypt_file(archive, &compressed, password)?;
    let decoder = zstd::Decoder::new(File::open(&compressed)?)?;
    let mut tar = tar::Archive::new(decoder);
    let entries = tar.entries()?;
    let mut manifest: Option<BackupManifest> = None;
    let mut expected: HashMap<String, ArchivedFile> = HashMap::new();
    let mut seen = HashSet::new();
    for entry in entries {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            return Err(SanctumError::Integrity(
                "backup archive contains a non-file entry".into(),
            ));
        }
        let path = entry.path()?.into_owned();
        validate_archive_path(&path)?;
        let path_string = path.to_string_lossy().replace('\\', "/");
        if !seen.insert(path_string.clone()) {
            return Err(SanctumError::Integrity(format!(
                "duplicate archive path: {path_string}"
            )));
        }
        if manifest.is_none() {
            if path_string != "backup-manifest.json" || entry.size() > MAX_MANIFEST_SIZE {
                return Err(SanctumError::Integrity(
                    "backup manifest must be the first archive entry".into(),
                ));
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            let parsed: BackupManifest = serde_json::from_slice(&bytes)?;
            validate_backup_manifest(&parsed)?;
            let total = parsed
                .database
                .byte_size
                .saturating_add(parsed.vault_manifest.byte_size)
                .saturating_add(
                    parsed
                        .objects
                        .iter()
                        .map(|file| file.byte_size)
                        .sum::<u64>(),
                )
                .saturating_add(
                    parsed
                        .journal
                        .iter()
                        .map(|file| file.byte_size)
                        .sum::<u64>(),
                );
            if total > MAX_ARCHIVE_EXPANSION
                || fs2::available_space(destination)? < total.saturating_add(64 * 1024 * 1024)
            {
                return Err(SanctumError::Io(std::io::Error::new(
                    std::io::ErrorKind::StorageFull,
                    "insufficient space for verified backup extraction",
                )));
            }
            for file in std::iter::once(&parsed.vault_manifest)
                .chain(std::iter::once(&parsed.database))
                .chain(parsed.objects.iter())
                .chain(parsed.journal.iter())
            {
                if expected.insert(file.path.clone(), file.clone()).is_some() {
                    return Err(SanctumError::Integrity(format!(
                        "backup manifest repeats path {}",
                        file.path
                    )));
                }
            }
            manifest = Some(parsed);
            continue;
        }
        let declaration = expected.remove(&path_string).ok_or_else(|| {
            SanctumError::Integrity(format!("archive path is not declared: {path_string}"))
        })?;
        if entry.size() != declaration.byte_size {
            return Err(SanctumError::Integrity(format!(
                "archive size mismatch: {path_string}"
            )));
        }
        let target = destination.join(&path);
        fs::create_dir_all(target.parent().expect("archive file parent"))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        let written = std::io::copy(&mut entry, &mut output)?;
        output.sync_all()?;
        if written != declaration.byte_size || sha256_file(&target)? != declaration.sha256 {
            return Err(SanctumError::Integrity(format!(
                "archive hash mismatch: {path_string}"
            )));
        }
    }
    let manifest =
        manifest.ok_or_else(|| SanctumError::Integrity("backup manifest is missing".into()))?;
    if !expected.is_empty() {
        return Err(SanctumError::Integrity(format!(
            "backup is missing {} declared files",
            expected.len()
        )));
    }
    let vault_manifest: VaultManifest =
        serde_json::from_slice(&fs::read(destination.join("manifest.json"))?)?;
    if vault_manifest.vault_id != manifest.vault_id || vault_manifest.format_version != 1 {
        return Err(SanctumError::Integrity(
            "backup Vault manifest identity mismatch".into(),
        ));
    }
    let connection = Connection::open(destination.join("db/sanctum.sqlite"))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    verify_vault_database(&connection, &manifest.vault_id, manifest.revision)?;
    let mut statement =
        connection.prepare("SELECT sha256,byte_size FROM objects ORDER BY sha256")?;
    let database_objects = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut archived_objects = manifest
        .objects
        .iter()
        .map(|file| (file.sha256.clone(), file.byte_size))
        .collect::<Vec<_>>();
    archived_objects.sort();
    archived_objects.dedup();
    if database_objects != archived_objects || archived_objects.len() != manifest.objects.len() {
        return Err(SanctumError::Integrity(
            "backup object manifest does not match the database".into(),
        ));
    }
    drop(statement);
    if let Some(first) = journal::verify_journal(&connection, destination)?.first() {
        return Err(SanctumError::Integrity(first.clone()));
    }
    drop(connection);
    sync_directory(destination)?;
    Ok(manifest)
}

fn validate_backup_manifest(manifest: &BackupManifest) -> Result<()> {
    if manifest.format_version != 1 || manifest.schema_version != db::SCHEMA_VERSION {
        return Err(SanctumError::UnsupportedFormat(
            "backup format or schema is not supported".into(),
        ));
    }
    if manifest.database.path != "db/sanctum.sqlite"
        || manifest.vault_manifest.path != "manifest.json"
    {
        return Err(SanctumError::Integrity(
            "backup manifest has invalid core paths".into(),
        ));
    }
    if manifest.database.byte_size > MAX_ARCHIVE_EXPANSION {
        return Err(SanctumError::Integrity(
            "backup database is unreasonably large".into(),
        ));
    }
    for file in std::iter::once(&manifest.vault_manifest)
        .chain(std::iter::once(&manifest.database))
        .chain(manifest.objects.iter())
        .chain(manifest.journal.iter())
    {
        validate_archive_path(Path::new(&file.path))?;
        if file.sha256.len() != 64 || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(SanctumError::Integrity(format!(
                "invalid archived hash for {}",
                file.path
            )));
        }
    }
    for object in &manifest.objects {
        let hash = object.path.rsplit('/').next().unwrap_or_default();
        if hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || hash != object.sha256
            || object.path != format!("objects/sha256/{}/{}/{}", &hash[0..2], &hash[2..4], hash)
        {
            return Err(SanctumError::Integrity(
                "invalid content-addressed object path".into(),
            ));
        }
    }
    if manifest.journal.len() as i64 != manifest.revision {
        return Err(SanctumError::Integrity(
            "backup journal count does not equal Vault revision".into(),
        ));
    }
    Ok(())
}

fn validate_archive_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(SanctumError::Integrity(
            "archive contains an absolute or empty path".into(),
        ));
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(SanctumError::Integrity(
            "archive path traversal was rejected".into(),
        ));
    }
    let value = path.to_string_lossy().replace('\\', "/");
    let allowed = value == "backup-manifest.json"
        || value == "manifest.json"
        || value == "db/sanctum.sqlite"
        || (value.starts_with("objects/sha256/") && value.split('/').count() == 5)
        || (value.starts_with("journal/")
            && value.ends_with(".json")
            && value.split('/').count() == 2);
    if !allowed {
        return Err(SanctumError::Integrity(format!(
            "archive path is outside the allowlist: {value}"
        )));
    }
    Ok(())
}
