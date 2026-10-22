pub fn token() -> crate::Result<String> {
    use windows::Win32::Security::Cryptography::*;
    let mut bytes = [0u8; 16];
    unsafe {
        BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
            .ok()
            .map_err(|e| e.to_string())?;
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
