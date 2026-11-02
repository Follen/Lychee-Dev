//! Authenticated decryption through the system CNG provider. No provider files,
//! account discovery or credentials leave this byte-only mechanism.
use crate::{Result, process::finish_query};
use windows::{Win32::Security::Cryptography::*, core::PCWSTR};

use super::cng::{Handles, checked};

/// Decrypt one AES-GCM message with a 12-byte nonce and a trailing 16-byte tag.
/// The provider allocates its key object and DestroyKey releases that storage:
/// https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcryptgeneratesymmetrickey
pub fn decrypt_aes_gcm(key: &[u8], nonce: &[u8], encrypted: &[u8]) -> Result<Vec<u8>> {
    if ![16, 24, 32].contains(&key.len())
        || nonce.len() != 12
        || !(16..=16 << 20).contains(&encrypted.len())
    {
        return Err("invalid AES-GCM parameters".into());
    }
    let mut handles = Handles::default();
    let result = (|| {
        let mut algorithm = BCRYPT_ALG_HANDLE::default();
        // SAFETY: the fixed algorithm name and output storage remain live.
        let opened = unsafe {
            BCryptOpenAlgorithmProvider(
                &mut algorithm,
                BCRYPT_AES_ALGORITHM,
                PCWSTR::null(),
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
        };
        if !algorithm.0.is_null() {
            handles.algorithm = Some(algorithm);
        }
        checked(opened, "CNG AES provider is unavailable")?;
        if handles.algorithm.is_none() {
            return Err("CNG AES provider returned no handle".into());
        }
        let mode = "ChainingModeGCM\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        // SAFETY: mode is a complete NUL-terminated UTF-16 property value.
        checked(
            unsafe {
                BCryptSetProperty(BCRYPT_HANDLE(algorithm.0), BCRYPT_CHAINING_MODE, &mode, 0)
            },
            "CNG GCM mode is unavailable",
        )?;
        let mut native_key = BCRYPT_KEY_HANDLE::default();
        // SAFETY: CNG owns its key-object allocation; borrowed key bytes remain
        // live for this synchronous call and are not retained by our wrapper.
        let generated =
            unsafe { BCryptGenerateSymmetricKey(algorithm, &mut native_key, None, key, 0) };
        if !native_key.0.is_null() {
            handles.key = Some(native_key);
        }
        checked(generated, "CNG key creation failed")?;
        if handles.key.is_none() {
            return Err("CNG key creation returned no handle".into());
        }
        let size = encrypted.len() - 16;
        let mut nonce = nonce.to_vec();
        let mut tag = encrypted[size..].to_vec();
        let info = BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO {
            cbSize: std::mem::size_of::<BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO>() as u32,
            dwInfoVersion: BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO_VERSION,
            pbNonce: nonce.as_mut_ptr(),
            cbNonce: nonce.len() as u32,
            pbTag: tag.as_mut_ptr(),
            cbTag: tag.len() as u32,
            ..Default::default()
        };
        let mut output = vec![0; size];
        let mut written = 0;
        // SAFETY: all nonce/tag/input/output buffers and mode info remain live
        // for the call. Only a successful tag check can return plaintext.
        checked(
            unsafe {
                BCryptDecrypt(
                    native_key,
                    Some(&encrypted[..size]),
                    Some((&info as *const BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO).cast()),
                    None,
                    Some(&mut output),
                    &mut written,
                    BCRYPT_FLAGS(0),
                )
            },
            "CNG authenticated decryption failed",
        )?;
        if written as usize != size {
            return Err("CNG decryption returned an invalid length".into());
        }
        Ok(output)
    })();
    finish_query(result, handles.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_decryption_matches_a_known_vector_and_rejects_modified_input() {
        let encrypted = [
            0x03, 0x88, 0xda, 0xce, 0x60, 0xb6, 0xa3, 0x92, 0xf3, 0x28, 0xc2, 0xb9, 0x71, 0xb2,
            0xfe, 0x78, 0xab, 0x6e, 0x47, 0xd4, 0x2c, 0xec, 0x13, 0xbd, 0xf5, 0x3a, 0x67, 0xb2,
            0x12, 0x57, 0xbd, 0xdf,
        ];
        assert_eq!(
            decrypt_aes_gcm(&[0; 16], &[0; 12], &encrypted).unwrap(),
            [0; 16]
        );
        let mut changed = encrypted;
        changed[31] ^= 1;
        assert!(decrypt_aes_gcm(&[0; 16], &[0; 12], &changed).is_err());
        assert!(decrypt_aes_gcm(&[1; 16], &[0; 12], &encrypted).is_err());
        assert!(decrypt_aes_gcm(&[0; 15], &[0; 12], &encrypted).is_err());
        assert!(decrypt_aes_gcm(&[0; 16], &[0; 11], &encrypted).is_err());
        assert!(decrypt_aes_gcm(&[0; 16], &[0; 12], &[0; 15]).is_err());
    }
}
