use super::*;
use lycheedev_windows::credentials::{self, AppData};
use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Client {
    Creator,
    Main,
}
impl Client {
    pub fn id(self) -> &'static str {
        match self {
            Self::Creator => "creator",
            Self::Main => "main",
        }
    }
}
pub(super) struct Session {
    pub(super) client: Client,
    pub(super) token: String,
    pub(super) device: Option<String>,
    pub(super) build: Option<u8>,
}
fn auth_error(client: Client) -> ToolkitError {
    ToolkitError::new(
        "publish.modus_login_required",
        format!(
            "log in to ModUs {} as the current Windows user, then run publish modus session doctor --client {}",
            client.id(),
            client.id()
        ),
        3,
    )
}
impl Session {
    pub(super) fn load(client: Client, control: &OperationControl) -> Result<Self> {
        control.check()?;
        let kind = match client {
            Client::Creator => AppData::Local,
            Client::Main => AppData::Roaming,
        };
        let path = credentials::app_data(kind).map_err(|_| auth_error(client))?;
        match client {
            Client::Creator => Self::creator(&path.join("ModUs.Creator/auth"), control),
            Client::Main => Self::main(&path.join("modus/Local Storage/leveldb"), control),
        }
    }
    fn creator(root: &Path, control: &OperationControl) -> Result<Self> {
        let primary = root.join("token.dat");
        // token.json is the official Creator store on older client versions,
        // never a Fuploader session or a substitute for a failed DPAPI token.
        let token = if primary.exists() {
            let cipher = limited(&primary)?;
            String::from_utf8(
                credentials::unprotect_current_user(&cipher, Some(b"ModUs.Creator.TokenStore.v1"))
                    .map_err(|_| auth_error(Client::Creator))?,
            )
            .map_err(|_| auth_error(Client::Creator))?
        } else {
            let document = crate::input::value(&limited(&root.join("token.json"))?)
                .map_err(|_| auth_error(Client::Creator))?;
            document
                .get("token")
                .or_else(|| document.get("accessToken"))
                .and_then(Value::as_str)
                .ok_or_else(|| auth_error(Client::Creator))?
                .to_owned()
        };
        control.check()?;
        let token = token.trim().to_owned();
        if token.is_empty() {
            return Err(auth_error(Client::Creator));
        }
        Ok(Self {
            client: Client::Creator,
            token,
            device: None,
            build: None,
        })
    }
    fn main(root: &Path, control: &OperationControl) -> Result<Self> {
        let values = super::leveldb::read(root, control)?;
        let user = parse_key(values.get("user"))?;
        let app = parse_key(values.get("app"))?;
        let token = user["token"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| auth_error(Client::Main))?
            .to_owned();
        let device = app["deviceId"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        Ok(Self {
            client: Client::Main,
            token,
            device,
            build: current(values.get("wow")),
        })
    }
    pub(super) fn authorization(&self) -> String {
        if self.client == Client::Creator
            && !self
                .token
                .get(..7)
                .is_some_and(|p| p.eq_ignore_ascii_case("Bearer "))
        {
            format!("Bearer {}", self.token)
        } else {
            self.token.clone()
        }
    }
    pub(super) fn build(&self, explicit: Option<u8>) -> Result<u8> {
        let build = explicit.or(self.build).unwrap_or(0);
        if build > 4 {
            return Err(ToolkitError::arguments(
                "--server-type must be a ModUs build ID 0..4; use publish modus builds",
            ));
        }
        Ok(build)
    }
}
fn limited(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| auth_error(Client::Creator))?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| auth_error(Client::Creator))?;
    if bytes.len() > 1024 * 1024 {
        return Err(auth_error(Client::Creator));
    }
    Ok(bytes)
}
fn parse_key(value: Option<&String>) -> Result<Value> {
    match value {
        Some(value) => crate::input::value(value.as_bytes())
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| auth_error(Client::Main)),
        None => Ok(json!({})),
    }
}
fn current(value: Option<&String>) -> Option<u8> {
    let value = parse_key(value).ok()?;
    value
        .pointer("/currentGameWow/id")
        .and_then(Value::as_u64)
        .filter(|n| *n <= 4)
        .map(|n| n as u8)
}
pub(super) const BUILD_CODES: [&str; 5] = [
    "retail",
    "classic_era",
    "classic",
    "classic_titan",
    "anniversary",
];
pub(super) fn builds(control: &OperationControl) -> Result<Value> {
    let root = credentials::app_data(AppData::Roaming)
        .map_err(|_| auth_error(Client::Main))?
        .join("modus/Local Storage/leveldb");
    let current = if root.exists() {
        current(super::leveldb::read(&root, control)?.get("wow"))
    } else {
        None
    };
    Ok(
        json!({"platform":"modus","current":current,"default_if_unset":0,"builds":[
        {"id":0,"code":BUILD_CODES[0],"name":"至暗之夜"},{"id":1,"code":BUILD_CODES[1],"name":"经典旧世"},
        {"id":2,"code":BUILD_CODES[2],"name":"熊猫人之谜"},{"id":3,"code":BUILD_CODES[3],"name":"泰坦重铸"},
        {"id":4,"code":BUILD_CODES[4],"name":"燃烧的远征"}]}),
    )
}

#[cfg(test)]
mod tests;
