mod backup;
mod db;
pub mod domain;
mod error;
mod journal;
mod object_store;
mod snapshot;
mod vault;

pub use domain::*;
pub use error::{Result, SanctumError};
pub use vault::Vault;

use chrono::{SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

pub(crate) fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher_writer(&mut hasher))?;
    Ok(hex::encode(hasher.finalize()))
}

/// Flush an existing regular file through a handle that has write access.
///
/// Windows requires `GENERIC_WRITE` for `FlushFileBuffers`; reopening a file
/// with `File::open` creates a read-only handle and fails with OS error 5.
/// Keeping this in one helper prevents platform-specific durability regressions
/// in vault creation, snapshots, object publishing, backups, and restores.
pub(crate) fn sync_file(path: &Path) -> Result<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()?;
    Ok(())
}

fn hasher_writer<'a>(hasher: &'a mut Sha256) -> impl Write + 'a {
    struct Writer<'a>(&'a mut Sha256);
    impl Write for Writer<'_> {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.update(buffer);
            Ok(buffer.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    Writer(hasher)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| SanctumError::InvalidInput("path has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.partial",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("write"),
        Uuid::new_v4()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = rename_noreplace(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    sync_directory(parent)?;
    Ok(())
}

pub(crate) fn rename_noreplace(source: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        return Err(SanctumError::RefuseOverwrite(destination.to_path_buf()));
    }

    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let source = CString::new(source.as_os_str().as_bytes())
            .map_err(|_| SanctumError::InvalidInput("source path contains a NUL byte".into()))?;
        let destination_c = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
            SanctumError::InvalidInput("destination path contains a NUL byte".into())
        })?;
        let status = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                destination_c.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if status == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EEXIST) {
            return Err(SanctumError::RefuseOverwrite(destination.to_path_buf()));
        }
        Err(error.into())
    }

    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let source = CString::new(source.as_os_str().as_bytes())
            .map_err(|_| SanctumError::InvalidInput("source path contains a NUL byte".into()))?;
        let destination_c = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
            SanctumError::InvalidInput("destination path contains a NUL byte".into())
        })?;
        let status =
            unsafe { libc::renamex_np(source.as_ptr(), destination_c.as_ptr(), libc::RENAME_EXCL) };
        if status == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EEXIST) {
            return Err(SanctumError::RefuseOverwrite(destination.to_path_buf()));
        }
        Err(error.into())
    }

    #[cfg(target_os = "windows")]
    {
        // MoveFileExW without MOVEFILE_REPLACE_EXISTING, used by std::fs::rename,
        // refuses an existing destination on Windows.
        fs::rename(source, destination).map_err(|error| {
            if destination.exists() {
                SanctumError::RefuseOverwrite(destination.to_path_buf())
            } else {
                error.into()
            }
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = source;
        Err(SanctumError::UnsupportedFormat(
            "atomic no-replace publish is not implemented for this platform".into(),
        ))
    }
}

pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

pub(crate) fn sync_tree_directories(root: &Path) -> Result<()> {
    let mut directories = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_dir())
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        sync_directory(&directory)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::sync_file;

    #[test]
    fn sync_file_reopens_existing_file_with_write_access() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("durable.bin");
        std::fs::write(&path, b"sanctum").expect("write test file");

        sync_file(&path).expect("flush existing file through writable handle");
        assert_eq!(std::fs::read(&path).expect("read test file"), b"sanctum");
    }
}
