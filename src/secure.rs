//! Protection of the TIDAL session tokens at rest.
//!
//! On Windows the tokens are encrypted with DPAPI (`CryptProtectData`) under
//! the current user's logon credentials, so `settings.json` copied to another
//! account or machine, or read from a backup, does not reveal them. DPAPI does
//! **not** protect against other programs running as the same user: they can
//! ask Windows to decrypt the blob just as TIDE does.
//!
//! On other platforms there is no equivalent wired up yet: `protect` returns
//! `None` and the caller keeps the plaintext fields (documented in
//! docs/SECURITY.md).

/// Domain separation for the blob (not a secret).
#[cfg(windows)]
const ENTROPY: &[u8] = b"TIDE session tokens v1";

#[cfg(windows)]
pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_ptr() as *mut u8 };
    let entropy = CRYPT_INTEGER_BLOB { cbData: ENTROPY.len() as u32, pbData: ENTROPY.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: every pointer refers to a live buffer of the stated length; the
    // output buffer is allocated by Windows and released with LocalFree below.
    let ok = unsafe {
        CryptProtectData(&input, std::ptr::null(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
    };
    take_blob(ok != 0, out)
}

#[cfg(windows)]
pub fn unprotect(blob: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: blob.len() as u32, pbData: blob.as_ptr() as *mut u8 };
    let entropy = CRYPT_INTEGER_BLOB { cbData: ENTROPY.len() as u32, pbData: ENTROPY.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: as in `protect`.
    let ok = unsafe {
        CryptUnprotectData(&input, std::ptr::null_mut(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
    };
    take_blob(ok != 0, out)
}

/// Copy a DPAPI output blob into a Vec and free the Windows allocation.
#[cfg(windows)]
fn take_blob(ok: bool, out: windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB) -> Option<Vec<u8>> {
    if out.pbData.is_null() {
        return None;
    }
    // SAFETY: on success Windows returned `cbData` readable bytes at `pbData`.
    let data = ok.then(|| unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec());
    // SAFETY: the buffer was allocated by DPAPI with LocalAlloc.
    unsafe { windows_sys::Win32::Foundation::LocalFree(out.pbData as _) };
    data
}

#[cfg(not(windows))]
pub fn protect(_plain: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(not(windows))]
pub fn unprotect(_blob: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn round_trip() {
        let blob = super::protect(b"token-123").expect("DPAPI available");
        assert!(!blob.windows(9).any(|w| w == b"token-123"), "blob must not contain the plaintext");
        assert_eq!(super::unprotect(&blob).as_deref(), Some(&b"token-123"[..]));
    }

    #[test]
    fn rejects_garbage() {
        assert!(super::unprotect(b"not a dpapi blob").is_none());
    }
}
