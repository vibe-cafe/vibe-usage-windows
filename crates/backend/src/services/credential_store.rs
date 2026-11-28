//! The current Windows user's Credential Manager, for credentials this app
//! keeps itself: the ZCode keys and the Cursor sign-in. Only targets under
//! `ai.vibecafe.vibe-usage.windows/` are ever read or written here; nothing
//! another program stored (Cursor's own login included) is touched.

/// `CRED_MAX_CREDENTIAL_BLOB_SIZE`: the largest blob one entry holds.
pub const MAX_BLOB: usize = 5 * 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// No Credential Manager on this platform (development builds off Windows).
    #[cfg(not(windows))]
    Unavailable,
    /// The system refused the read, write or delete.
    Failed,
}

pub fn load(target: &str) -> Result<Option<Vec<u8>>, StoreError> {
    platform::load(target)
}

pub fn store(target: &str, blob: &[u8]) -> Result<(), StoreError> {
    if blob.len() > MAX_BLOB {
        return Err(StoreError::Failed);
    }
    platform::store(target, blob)
}

/// Removing an entry that does not exist succeeds.
pub fn delete(target: &str) -> Result<(), StoreError> {
    platform::delete(target)
}

#[cfg(windows)]
mod platform {
    use super::StoreError;
    use std::ffi::c_void;
    use std::ptr;
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    const ERROR_NOT_FOUND: i32 = 1168;

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn load(target: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let target = wide(target);
        let mut raw: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: target is a terminated UTF-16 buffer and `raw` is initialized
        // by CredReadW. Its allocation is released exactly once with CredFree.
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) };
        if ok == 0 {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_NOT_FOUND) {
                Ok(None)
            } else {
                Err(StoreError::Failed)
            };
        }
        if raw.is_null() {
            return Ok(None);
        }
        // SAFETY: CredReadW returned a valid CREDENTIALW whose blob remains
        // alive until the matching CredFree below.
        let bytes = unsafe {
            let credential = &*raw;
            if credential.CredentialBlobSize == 0 || credential.CredentialBlob.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
                .to_vec()
            }
        };
        // SAFETY: `raw` came from CredReadW and has not been freed yet.
        unsafe { CredFree(raw.cast::<c_void>()) };
        Ok(Some(bytes))
    }

    pub fn store(target: &str, blob: &[u8]) -> Result<(), StoreError> {
        let mut target = wide(target);
        let mut username = wide("Vibe Usage");
        let mut blob = blob.to_vec();
        // SAFETY: every pointer references a live buffer for the duration of
        // CredWriteW. Generic credential blobs may contain arbitrary bytes.
        let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = target.as_mut_ptr();
        credential.CredentialBlobSize = blob.len() as u32;
        credential.CredentialBlob = blob.as_mut_ptr();
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        credential.UserName = username.as_mut_ptr();
        let ok = unsafe { CredWriteW(&credential, 0) };
        if ok == 0 {
            Err(StoreError::Failed)
        } else {
            Ok(())
        }
    }

    pub fn delete(target: &str) -> Result<(), StoreError> {
        let target = wide(target);
        // SAFETY: target is a terminated UTF-16 buffer.
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok != 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_NOT_FOUND) {
            Ok(())
        } else {
            Err(StoreError::Failed)
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::StoreError;

    pub fn load(_target: &str) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(None)
    }

    pub fn store(_target: &str, _blob: &[u8]) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    pub fn delete(_target: &str) -> Result<(), StoreError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    /// Opt-in native acceptance with disposable dummy values, never the
    /// production credential targets this app uses.
    #[cfg(windows)]
    #[test]
    #[ignore = "explicit Windows Credential Manager acceptance"]
    fn credential_manager_roundtrip_isolated() {
        let namespace = format!(
            "ai.vibecafe.vibe-usage.acceptance/{}/{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        struct Cleanup([String; 2]);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for key in &self.0 {
                    let _ = delete(key);
                }
            }
        }
        let cleanup = Cleanup([format!("{namespace}/bigmodel"), format!("{namespace}/zai")]);
        let [domestic, overseas] = &cleanup.0;
        assert_eq!(load(domestic).unwrap(), None);
        assert_eq!(load(overseas).unwrap(), None);
        store(domestic, b"acceptance-domestic-fixture").unwrap();
        store(overseas, b"acceptance-overseas-fixture").unwrap();
        store(domestic, b"acceptance-updated-fixture").unwrap();
        assert_eq!(load(domestic).unwrap().as_deref(), Some(&b"acceptance-updated-fixture"[..]));
        assert_eq!(load(overseas).unwrap().as_deref(), Some(&b"acceptance-overseas-fixture"[..]));
        delete(domestic).unwrap();
        delete(overseas).unwrap();
        assert_eq!(load(domestic).unwrap(), None);
        assert_eq!(load(overseas).unwrap(), None);
    }

    #[test]
    fn a_blob_over_the_system_limit_is_refused_before_the_call() {
        assert_eq!(store("ai.vibecafe.vibe-usage.acceptance/too-big", &vec![b'x'; MAX_BLOB + 1]), Err(StoreError::Failed));
    }
}
