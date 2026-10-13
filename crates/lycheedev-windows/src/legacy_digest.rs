//! CNG MD5 is used only for legacy CASC content/chunk identities, never new authorization.
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use windows::{Win32::Security::Cryptography::*, core::PCWSTR};
struct Algorithm(BCRYPT_ALG_HANDLE);
impl Drop for Algorithm {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptCloseAlgorithmProvider(self.0, 0);
        }
    }
}
struct Hash(BCRYPT_HASH_HANDLE);
impl Drop for Hash {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptDestroyHash(self.0);
        }
    }
}
pub fn legacy_md5(bytes: &[u8], deadline: Instant, cancel: &AtomicBool) -> crate::Result<[u8; 16]> {
    let check = || -> crate::Result<()> {
        if cancel.load(Ordering::Acquire) {
            Err("digest.cancelled".into())
        } else if Instant::now() >= deadline {
            Err("digest.deadline".into())
        } else {
            Ok(())
        }
    };
    check()?;
    unsafe {
        let mut alg = BCRYPT_ALG_HANDLE::default();
        BCryptOpenAlgorithmProvider(
            &mut alg,
            BCRYPT_MD5_ALGORITHM,
            PCWSTR::null(),
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS::default(),
        )
        .ok()
        .map_err(|e| e.to_string())?;
        let alg = Algorithm(alg);
        let mut hash = BCRYPT_HASH_HANDLE::default();
        BCryptCreateHash(alg.0, &mut hash, None, None, 0)
            .ok()
            .map_err(|e| e.to_string())?;
        let hash = Hash(hash);
        for chunk in bytes.chunks(1 << 20) {
            check()?;
            BCryptHashData(hash.0, chunk, 0)
                .ok()
                .map_err(|e| e.to_string())?;
        }
        let mut out = [0; 16];
        check()?;
        BCryptFinishHash(hash.0, &mut out, 0)
            .ok()
            .map_err(|e| e.to_string())?;
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_legacy_digest_and_cancel() {
        let cancel = AtomicBool::new(false);
        let digest = legacy_md5(
            b"abc",
            Instant::now() + std::time::Duration::from_secs(1),
            &cancel,
        )
        .unwrap();
        assert_eq!(
            digest,
            [
                0x90, 0x01, 0x50, 0x98, 0x3c, 0xd2, 0x4f, 0xb0, 0xd6, 0x96, 0x3f, 0x7d, 0x28, 0xe1,
                0x7f, 0x72
            ]
        );
        cancel.store(true, Ordering::Release);
        assert!(
            legacy_md5(
                b"abc",
                Instant::now() + std::time::Duration::from_secs(1),
                &cancel
            )
            .is_err()
        );
    }
}
