//! Shared ownership of CNG keys/providers used by AES-GCM and RSA.
use crate::{Result, process::finish_query};
use windows::Win32::{Foundation::NTSTATUS, Security::Cryptography::*};

#[derive(Default)]
pub(super) struct Handles {
    pub(super) algorithm: Option<BCRYPT_ALG_HANDLE>,
    pub(super) key: Option<BCRYPT_KEY_HANDLE>,
}
pub(super) fn checked(status: NTSTATUS, message: &str) -> Result<()> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(message.into())
    }
}
impl Handles {
    pub(super) fn finish(&mut self) -> Result<()> {
        let key = self.key.take().map_or(Ok(()), |key| {
            // SAFETY: this owner consumes its original CNG key exactly once.
            checked(unsafe { BCryptDestroyKey(key) }, "CNG key release failed")
        });
        let algorithm = self.algorithm.take().map_or(Ok(()), |algorithm| {
            // SAFETY: no live borrowed key remains; this is the sole close.
            checked(
                unsafe { BCryptCloseAlgorithmProvider(algorithm, 0) },
                "CNG provider release failed",
            )
        });
        finish_query(key, algorithm)
    }
}
impl Drop for Handles {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
