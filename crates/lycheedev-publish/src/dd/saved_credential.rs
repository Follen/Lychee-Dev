//! Read only the DD-selected typed account. Never infer a login mode from its
//! name, enumerate alternative accounts, repair the vendor DB or send a login.
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use lycheedev_windows::credentials::{self, AppData};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::{path::Path, time::Duration};

// Public application entropy from DD 100130's components/login/enc.pyc.
const ENTROPY: &[u8] = &[
    0x47, 0x78, 0x9a, 0xa3, 0x33, 0x5b, 0xbb, 0xd0, 0xc0, 0x56, 0x95, 0xbf, 0x0e, 0x87, 0x62, 0x52,
    0xca, 0xde, 0x9d, 0x94, 0x65, 0x9b, 0x23, 0x93, 0x92, 0x6f, 0x4f, 0xfd, 0xe4, 0x72, 0xa0, 0x3c,
];
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    Email,
    Mobile,
}
pub struct SavedCredential {
    pub(super) kind: CredentialKind,
    pub(super) account: String,
    pub(super) token: String,
}
struct Selected {
    method: i64,
    name: String,
    cipher: Vec<u8>,
}
impl SavedCredential {
    pub fn open(control: &OperationControl) -> Result<Self> {
        control.check()?;
        let root = credentials::app_data(AppData::Roaming)
            .map_err(|_| missing("Windows current-user Roaming AppData is unavailable"))?;
        let path = root.join("CCVoiceHub/cred.db");
        storage::guard_path(&root, &path)?;
        Self::from_database(&path, control)
    }
    fn from_database(path: &Path, control: &OperationControl) -> Result<Self> {
        control.check()?;
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| {
            missing(
                "DD saved login database is missing or unreadable; sign in with the desktop client",
            )
        })?;
        let selected: Result<Selected> = (|| {
            connection
                .busy_timeout(control.remaining()?.min(Duration::from_millis(150)))
                .map_err(|_| missing("DD saved login database could not be read"))?;
            let transaction = connection
                .transaction()
                .map_err(|_| missing("DD saved login snapshot is unavailable"))?;
            let account: Option<String> = transaction
                .query_row(
                    "SELECT value FROM config WHERE key='auto_account'",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|_| missing("DD selected account is unreadable"))?;
            let account = account.filter(|value| value.len() <= 4096).ok_or_else(|| {
                missing(
                    "DD has no valid selected auto-login account; sign in with the desktop client",
                )
            })?;
            let value = crate::input::value(account.as_bytes())
                .map_err(|_| missing("DD selected account is malformed"))?;
            let values = value
                .as_array()
                .filter(|values| values.len() == 2)
                .ok_or_else(|| missing("DD selected account is malformed"))?;
            let method = values[0]
                .as_i64()
                .filter(|method| [1, 2].contains(method))
                .ok_or_else(|| {
                    missing("DD selected account requires an unsupported native login method")
                })?;
            let name = values[1]
                .as_str()
                .filter(|name| {
                    !name.trim().is_empty() && name.len() <= 2048 && !name.contains('\0')
                })
                .ok_or_else(|| missing("DD selected account name is malformed"))?
                .to_owned();
            control.check()?;
            let row: Option<(i64, i64, Vec<u8>)> = transaction
                .query_row(
                    "SELECT flag, cred_type, cred FROM account WHERE type=?1 AND name=?2",
                    (method, &name),
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(|_| missing("DD selected credential is unreadable"))?;
            let (modifier, credential_type, cipher) =
                row.ok_or_else(|| missing("DD selected account has no saved credential"))?;
            let supported = (method == 1 && modifier == 0 && credential_type == 1)
                || (method == 2 && [0, 1, 4].contains(&modifier) && credential_type == 2);
            if !supported {
                return Err(missing(
                    "DD saved account, credential type and modifier are incompatible",
                ));
            }
            if cipher.is_empty() || cipher.len() > 131072 {
                return Err(missing("DD saved credential exceeds its supported format"));
            }
            transaction
                .commit()
                .map_err(|_| missing("DD saved login snapshot could not be released"))?;
            Ok(Selected {
                method,
                name,
                cipher,
            })
        })();
        connection
            .close()
            .map_err(|_| missing("DD saved login database could not be closed"))?;
        let selected = selected?;
        control.check()?;
        let plain =
            credentials::unprotect_current_user(&selected.cipher, Some(ENTROPY)).map_err(|_| {
                missing("DD saved credential cannot be decrypted for the current Windows user")
            })?;
        let token =
            String::from_utf8(plain).map_err(|_| missing("DD saved credential is not UTF-8"))?;
        if token.trim().is_empty() || token.len() > 65536 || token.contains('\0') {
            return Err(missing("DD saved credential is empty or malformed"));
        }
        control.check()?;
        Ok(Self {
            kind: if selected.method == 1 {
                CredentialKind::Email
            } else {
                CredentialKind::Mobile
            },
            account: selected.name,
            token,
        })
    }
    pub fn kind(&self) -> CredentialKind {
        self.kind
    }
    pub(super) fn cc_username(&self) -> String {
        match self.kind {
            CredentialKind::Email => self.account.clone(),
            CredentialKind::Mobile => format!("{}@mobile.163.com", self.account),
        }
    }
    pub(super) fn login_type(&self) -> &'static str {
        match self.kind {
            CredentialKind::Email => "ursMailToken",
            CredentialKind::Mobile => "ursPhoneTokenNew",
        }
    }
}
fn missing(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_saved_login", message, 3)
}
#[cfg(test)]
mod tests;
