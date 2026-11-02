//! Decode one selected WebView2 cookie snapshot entirely in memory. This module
//! performs no discovery, file reads, process access, login or credential writes.
use base64::{Engine, engine::general_purpose::STANDARD};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, metadata::owned_write::finish_owned,
};
use lycheedev_windows::credentials;
use rusqlite::{Connection, MAIN_DB};
use sha2::{Digest, Sha256};

mod image;
const MAX_IMAGE: usize = 16 << 20;
const MAX_COOKIE: usize = 16384;
const HOST: &str = ".163.com";

// Deliberately no Debug/Serialize: only the selected private HTTP boundary
// consumes this capability; public task state contains identity projections.
pub(super) struct Cookie(String);
impl Cookie {
    pub(super) fn into_value(self) -> String {
        self.0
    }
}
fn fault(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_desktop_cookie", message, 3)
}
pub(super) fn decode(
    database: &[u8],
    wal: &[u8],
    local_state: &[u8],
    now: i64,
    control: &OperationControl,
) -> Result<Cookie> {
    control.check()?;
    let image = image::compose(database, wal, control)?;
    if local_state.len() > 1 << 20 {
        return Err(fault("DD browser Local State exceeds its budget"));
    }
    let state = crate::input::value(local_state)
        .map_err(|_| fault("DD browser Local State is malformed"))?;
    let encrypted = state
        .pointer("/os_crypt/encrypted_key")
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.len() <= 65536)
        .ok_or_else(|| fault("DD browser encryption key is unavailable"))?;
    let encrypted = STANDARD
        .decode(encrypted)
        .map_err(|_| fault("DD browser encryption key is malformed"))?;
    let encrypted = encrypted
        .strip_prefix(b"DPAPI")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| fault("DD browser encryption scheme is unsupported"))?;
    control.check()?;
    let key = credentials::unprotect_current_user(encrypted, None)
        .map_err(|_| fault("DD browser key cannot be decrypted by the current Windows user"))?;
    if ![16, 24, 32].contains(&key.len()) {
        return Err(fault("DD browser encryption key has an invalid length"));
    }
    let mut connection = Connection::open_in_memory()
        .map_err(|_| fault("DD browser snapshot could not be opened"))?;
    let result = (|| {
        control.check()?;
        // This exact borrowed slice always supplies the admitted byte count.
        // rusqlite owns the SQLite allocation and frees it on connection close.
        connection
            .deserialize_read_exact(MAIN_DB, &image[..], image.len(), true)
            .map_err(|_| fault("DD browser private database image could not be opened"))?;
        connection
            .pragma_update(None, "trusted_schema", false)
            .map_err(|_| fault("DD browser database policy could not be applied"))?;
        let version: String = connection
            .query_row("SELECT value FROM meta WHERE key='version'", [], |r| {
                r.get(0)
            })
            .map_err(|_| fault("DD browser database version is unavailable"))?;
        let version = version
            .parse::<u32>()
            .map_err(|_| fault("DD browser database version is invalid"))?;
        let mut query = connection.prepare("SELECT value,encrypted_value,expires_utc FROM cookies WHERE host_key=?1 AND name='NTES_YD_SESS'")
            .map_err(|_| fault("DD browser session table is unavailable"))?;
        let mut rows = query
            .query([HOST])
            .map_err(|_| fault("DD browser session query failed"))?;
        let mut selected = None;
        while let Some(row) = rows
            .next()
            .map_err(|_| fault("DD browser session snapshot is invalid"))?
        {
            control.check()?;
            let plain: String = row
                .get(0)
                .map_err(|_| fault("DD browser cookie text is invalid"))?;
            let encrypted: Vec<u8> = row
                .get(1)
                .map_err(|_| fault("DD browser encrypted cookie is invalid"))?;
            let expiry: i64 = row
                .get(2)
                .map_err(|_| fault("DD browser cookie expiry is invalid"))?;
            if expiry != 0 && expiry <= now {
                continue;
            }
            if encrypted.len() > MAX_COOKIE + 64 {
                return Err(fault("DD browser encrypted cookie exceeds its budget"));
            }
            let value = if encrypted.starts_with(b"v10") {
                if encrypted.len() < 31 {
                    return Err(fault("DD browser encrypted cookie is incomplete"));
                }
                let mut value =
                    credentials::decrypt_aes_gcm(&key, &encrypted[3..15], &encrypted[15..])
                        .map_err(|_| fault("DD browser cookie authentication failed"))?;
                if version >= 24 {
                    if value.len() < 32 || value[..32] != Sha256::digest(HOST.as_bytes())[..] {
                        return Err(fault("DD browser cookie domain binding failed"));
                    }
                    value.drain(..32);
                }
                String::from_utf8(value)
                    .map_err(|_| fault("DD browser cookie encoding is invalid"))?
            } else if encrypted.starts_with(b"v20") {
                return Err(fault(
                    "DD browser uses unsupported application-bound cookie encryption",
                ));
            } else if !encrypted.is_empty() {
                String::from_utf8(
                    credentials::unprotect_current_user(&encrypted, None).map_err(|_| {
                        fault("DD browser cookie cannot be decrypted by the current Windows user")
                    })?,
                )
                .map_err(|_| fault("DD browser cookie encoding is invalid"))?
            } else {
                plain
            };
            if !value.is_empty()
                && value.len() <= MAX_COOKIE
                && !value.chars().any(|c| c.is_control() || c == ';')
            {
                if selected.is_some() {
                    return Err(fault(
                        "DD browser has no unique live session cookie; select an account in DD first",
                    ));
                }
                selected = Some(Cookie(value));
            }
        }
        control.check()?;
        selected
            .ok_or_else(|| fault("DD browser has no live session cookie; sign in with DD first"))
    })();
    let result = finish_owned(
        result,
        connection
            .close()
            .map_err(|_| fault("DD browser private database close failed")),
    );
    result.and_then(|cookie| {
        control.check()?;
        Ok(cookie)
    })
}

#[cfg(test)]
mod tests;
