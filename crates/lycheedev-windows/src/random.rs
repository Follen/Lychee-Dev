pub fn token() -> crate::Result<String> {
    Ok(bytes::<16>()?.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn bytes<const N: usize>() -> crate::Result<[u8; N]> {
    use windows::Win32::Security::Cryptography::*;
    let mut bytes = [0u8; N];
    // SAFETY: BCrypt receives the complete mutable byte buffer and uses the
    // operating system RNG, with no caller-managed algorithm handle.
    unsafe {
        BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
            .ok()
            .map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}
