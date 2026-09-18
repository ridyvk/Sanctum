use zeroize::Zeroizing;

#[cfg(windows)]
pub fn write_password(target: &str, password: &str) -> std::io::Result<()> {
    use std::mem;
    use windows_sys::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    let mut target = wide(target);
    let mut blob = Zeroizing::new(password.as_bytes().to_vec());
    let blob_size = u32::try_from(blob.len()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "credential is too large")
    })?;
    let mut credential: CREDENTIALW = unsafe { mem::zeroed() };
    credential.Type = CRED_TYPE_GENERIC;
    credential.TargetName = target.as_mut_ptr();
    credential.CredentialBlobSize = blob_size;
    credential.CredentialBlob = blob.as_mut_ptr();
    credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
    let status = unsafe { CredWriteW(&credential, 0) };
    if status == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn write_password(_target: &str, _password: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "automatic backup credentials require Windows Credential Manager",
    ))
}

#[cfg(windows)]
pub fn read_password(target: &str) -> std::io::Result<Zeroizing<String>> {
    use std::ffi::c_void;
    use std::ptr;
    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target = wide(target);
    let mut credential: *mut CREDENTIALW = ptr::null_mut();
    let status = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
    if status == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = unsafe {
        let credential_ref = &*credential;
        let bytes = std::slice::from_raw_parts(
            credential_ref.CredentialBlob,
            credential_ref.CredentialBlobSize as usize,
        );
        String::from_utf8(bytes.to_vec()).map(Zeroizing::new).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stored automatic backup password is not valid UTF-8",
            )
        })
    };
    unsafe { CredFree(credential.cast::<c_void>()) };
    result
}

#[cfg(not(windows))]
pub fn read_password(_target: &str) -> std::io::Result<Zeroizing<String>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "automatic backup credentials require Windows Credential Manager",
    ))
}

#[cfg(windows)]
pub fn delete_password(target: &str) -> std::io::Result<()> {
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target = wide(target);
    let status = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if status != 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(1168) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(not(windows))]
pub fn delete_password(_target: &str) -> std::io::Result<()> {
    Ok(())
}

pub fn has_password(target: &str) -> bool {
    read_password(target).is_ok()
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
