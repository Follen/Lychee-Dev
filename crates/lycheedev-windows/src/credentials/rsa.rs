//! RSAES-PKCS1-v1_5 through CNG. The caller owns the public key and protocol;
//! this boundary only imports the public integers and encrypts one message.
use super::cng::{Handles, checked};
use crate::{Result, process::finish_query};
use windows::{Win32::Security::Cryptography::*, core::PCWSTR};

pub fn encrypt_rsa_pkcs1(modulus: &[u8], exponent: &[u8], plain: &[u8]) -> Result<Vec<u8>> {
    if !(128..=1024).contains(&modulus.len())
        || exponent.is_empty()
        || exponent.len() > 8
        || plain.len() > modulus.len() - 11
    {
        return Err("invalid RSA public key or message length".into());
    }
    let mut blob = Vec::new();
    for field in [
        0x31415352u32,
        (modulus.len() * 8) as u32,
        exponent.len() as u32,
        modulus.len() as u32,
        0,
        0,
    ] {
        blob.extend_from_slice(&field.to_le_bytes());
    }
    blob.extend_from_slice(exponent);
    blob.extend_from_slice(modulus);
    let mut handles = Handles::default();
    let result = (|| {
        let mut algorithm = BCRYPT_ALG_HANDLE::default();
        // SAFETY: fixed algorithm name and live output storage; any returned
        // handle is recorded before checking the status.
        let opened = unsafe {
            BCryptOpenAlgorithmProvider(
                &mut algorithm,
                BCRYPT_RSA_ALGORITHM,
                PCWSTR::null(),
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
        };
        if !algorithm.0.is_null() {
            handles.algorithm = Some(algorithm);
        }
        checked(opened, "CNG RSA provider is unavailable")?;
        let mut key = BCRYPT_KEY_HANDLE::default();
        // SAFETY: blob contains the complete RSA public header and integers.
        let imported = unsafe {
            BCryptImportKeyPair(algorithm, None, BCRYPT_RSAPUBLIC_BLOB, &mut key, &blob, 0)
        };
        if !key.0.is_null() {
            handles.key = Some(key);
        }
        checked(imported, "CNG RSA public key import failed")?;
        let mut output = vec![0; modulus.len()];
        let mut written = 0;
        // SAFETY: input and output remain live; RSA PKCS1 encryption takes no
        // padding-info structure or IV. CNG supplies the randomized padding.
        checked(
            unsafe {
                BCryptEncrypt(
                    key,
                    Some(plain),
                    None,
                    None,
                    Some(&mut output),
                    &mut written,
                    BCRYPT_PAD_PKCS1,
                )
            },
            "CNG RSA encryption failed",
        )?;
        if written as usize != output.len() {
            return Err("CNG RSA returned an invalid size".into());
        }
        Ok(output)
    })();
    finish_query(result, handles.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    // Generated with an independent OpenSSL-backed key generator for this
    // test only. This is a synthetic key, never a platform credential.
    fn bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|s| u8::from_str_radix(std::str::from_utf8(s).unwrap(), 16).unwrap())
            .collect()
    }
    #[test]
    fn rsa_encryption_uses_random_pkcs1_padding_and_preserves_plaintext() {
        let modulus = bytes(
            "d4f7fe4109ae51f9b8963759f828c981d13c4e499e24701501833698b35face2f02441555e1284b0620d76b73eb09e8d6afcbcacddda2041c0c7abb2ba0823e364636965dd9d34dde456bd18701fe821525af688f225a139061c342faca11350ba911a507d3a16c1d48f2fde5831afa5ad03dbf4a5dfca12196ad88e8827864d",
        );
        let private = bytes(
            "525341320004000003000000800000004000000040000000010001d4f7fe4109ae51f9b8963759f828c981d13c4e499e24701501833698b35face2f02441555e1284b0620d76b73eb09e8d6afcbcacddda2041c0c7abb2ba0823e364636965dd9d34dde456bd18701fe821525af688f225a139061c342faca11350ba911a507d3a16c1d48f2fde5831afa5ad03dbf4a5dfca12196ad88e8827864dfdf79f69f5ebcf909dd19a12098baf0dd8d818b1dc3c3d34bb20f63800b355c1fe90df326febcbab3791549ba2f24b73f78afb7f05d803be7caa790160db9b29d6ac5d5d23535edf0b4d419ee8f57f3b4bab78ddde5b571e466587ecb956231e2dfe2caa0f457c74b6dd26193e9e3630832b06ade686ce66c280b95ed157da85",
        );
        let plain = b"+8613812345678";
        let first = encrypt_rsa_pkcs1(&modulus, &[1, 0, 1], plain).unwrap();
        let second = encrypt_rsa_pkcs1(&modulus, &[1, 0, 1], plain).unwrap();
        assert_ne!(first, second);
        let mut handles = Handles::default();
        let mut algorithm = BCRYPT_ALG_HANDLE::default();
        let mut key = BCRYPT_KEY_HANDLE::default();
        // SAFETY: fixed synthetic private key, live buffers, and owned handles;
        // this tests the native padding mode rather than ciphertext equality.
        unsafe {
            checked(
                BCryptOpenAlgorithmProvider(
                    &mut algorithm,
                    BCRYPT_RSA_ALGORITHM,
                    PCWSTR::null(),
                    BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
                ),
                "test provider",
            )
            .unwrap();
            handles.algorithm = Some(algorithm);
            checked(
                BCryptImportKeyPair(
                    algorithm,
                    None,
                    BCRYPT_RSAPRIVATE_BLOB,
                    &mut key,
                    &private,
                    0,
                ),
                "test private key",
            )
            .unwrap();
            handles.key = Some(key);
            for cipher in [&first, &second] {
                let mut recovered = vec![0; 128];
                let mut written = 0;
                checked(
                    BCryptDecrypt(
                        key,
                        Some(cipher),
                        None,
                        None,
                        Some(&mut recovered),
                        &mut written,
                        BCRYPT_PAD_PKCS1,
                    ),
                    "test decrypt",
                )
                .unwrap();
                assert_eq!(&recovered[..written as usize], plain);
            }
        }
        handles.finish().unwrap();
        assert!(encrypt_rsa_pkcs1(&modulus, &[1, 0, 1], &[0; 118]).is_err());
    }
}
