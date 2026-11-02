//! Current-user OS credential mechanisms; no provider paths, tokens or policy.
use crate::Result;
use std::{ffi::c_void, path::PathBuf};
use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    },
    core::{GUID, HRESULT, PWSTR},
};
mod aes_gcm;
mod cng;
mod rsa;
pub use aes_gcm::decrypt_aes_gcm;
pub use rsa::encrypt_rsa_pkcs1;

#[derive(Clone, Copy)]
pub enum AppData {
    Roaming,
    Local,
}

// Retain the out pointer on failure too: the Shell API requires the caller to
// free any returned buffer regardless of HRESULT. These two system APIs are the
// only raw bindings here; using their ABI avoids losing that ownership fact.
#[link(name = "shell32")]
unsafe extern "system" {
    #[link_name = "SHGetKnownFolderPath"]
    fn known_folder(id: *const GUID, flags: u32, token: *mut c_void, path: *mut PWSTR) -> HRESULT;
}
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(memory: *const c_void);
}

pub fn app_data(kind: AppData) -> Result<PathBuf> {
    let id = match kind {
        AppData::Roaming => GUID::from_u128(0x3eb685db_65f9_4cf6_a03a_e3ef65729f3d),
        AppData::Local => GUID::from_u128(0xf1b32785_6fba_4fcf_9d55_7b8e7f157091),
    };
    let mut raw = PWSTR::null();
    // SAFETY: GUID/out storage remain live; null token selects the current user.
    let status = unsafe { known_folder(&id, 0, std::ptr::null_mut(), &mut raw) };
    let result = if status.is_ok() && !raw.is_null() {
        // Preserve Windows paths with unpaired UTF-16 rather than converting
        // a path through a token/log string.
        use std::os::windows::ffi::OsStringExt;
        let wide = unsafe { raw.as_wide() };
        Ok(PathBuf::from(std::ffi::OsString::from_wide(wide)))
    } else {
        Err("Windows current-user AppData lookup failed".into())
    };
    if !raw.is_null() {
        unsafe { CoTaskMemFree(raw.0.cast()) };
    }
    result
}

fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len())
            .map_err(|_| "DPAPI input exceeds the Windows length limit")?,
        pbData: bytes.as_ptr().cast_mut(),
    })
}
fn take_output(output: CRYPT_INTEGER_BLOB) -> Result<Vec<u8>> {
    if output.pbData.is_null() {
        return if output.cbData == 0 {
            Ok(Vec::new())
        } else {
            Err("DPAPI returned invalid output storage".into())
        };
    }
    // SAFETY: DPAPI allocated exactly cbData output bytes. Copy while owned,
    // clear the native plaintext allocation, then release it with LocalFree.
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    for index in 0..output.cbData as usize {
        unsafe { output.pbData.add(index).write_volatile(0) };
    }
    let remaining = unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
    if !remaining.0.is_null() {
        return Err("DPAPI native output release failed".into());
    }
    Ok(bytes)
}
pub fn unprotect_current_user(cipher: &[u8], entropy: Option<&[u8]>) -> Result<Vec<u8>> {
    let input = blob(cipher)?;
    let entropy = entropy.map(blob).transpose()?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: all borrowed input buffers and output storage remain live; no UI
    // or impersonation is permitted and native output is owned on success.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            entropy.as_ref().map(|v| v as *const _),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|_| "DPAPI decryption failed for the current Windows user".to_owned())?;
    take_output(output)
}
pub fn protect_current_user(plain: &[u8], entropy: Option<&[u8]>) -> Result<Vec<u8>> {
    let input = blob(plain)?;
    let entropy = entropy.map(blob).transpose()?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            None,
            entropy.as_ref().map(|v| v as *const _),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|_| "DPAPI protection failed for the current Windows user".to_owned())?;
    take_output(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_user_dpapi_round_trip_honors_provider_entropy() {
        let plain = b"synthetic-credential-for-native-test-only";
        let entropy = b"lychee.native.test.v1";
        let cipher = protect_current_user(plain, Some(entropy)).unwrap();
        assert_ne!(cipher, plain);
        assert_eq!(
            unprotect_current_user(&cipher, Some(entropy)).unwrap(),
            plain
        );
        assert!(unprotect_current_user(&cipher, Some(b"other-provider")).is_err());
        assert_eq!(
            unprotect_current_user(&protect_current_user(plain, None).unwrap(), None).unwrap(),
            plain
        );
    }
    #[test]
    fn appdata_comes_from_the_current_user_known_folders() {
        for kind in [AppData::Roaming, AppData::Local] {
            let path = app_data(kind).unwrap();
            assert!(path.is_absolute());
            assert!(path.is_dir());
        }
    }
}
