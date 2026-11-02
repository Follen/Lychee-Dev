//! Original object bytes through one existing DD author session. The upload
//! grant remains in this worker; only a permanent artifact receipt leaves it.
use super::{DdTransport, task::Client};
use crate::{input::invalid, mutation::Mutation, output, provider::Provider};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use reqwest::{
    Url,
    header::{CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadKind {
    PluginArchive,
    WaMaterials,
    PluginImage,
    ConfigImage,
    WaImage,
}
impl UploadKind {
    fn image(self) -> bool {
        matches!(self, Self::PluginImage | Self::ConfigImage | Self::WaImage)
    }
    fn maximum(self) -> u64 {
        if self.image() {
            10 << 20
        } else if self == Self::WaMaterials {
            50 << 20
        } else {
            u64::MAX
        }
    }
    fn mime(self, path: &Path) -> Result<&'static str> {
        let suffix = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match (self.image(), suffix.as_str()) {
            (true, "png") => Ok("image/png"),
            (true, "jpg" | "jpeg") => Ok("image/jpeg"),
            (true, "gif") => Ok("image/gif"),
            (false, "zip") => Ok("application/x-zip-compressed"),
            _ => Err(invalid(
                "$.file",
                if self.image() {
                    "DD media requires .png, .jpg, .jpeg or .gif"
                } else {
                    "DD resource upload requires .zip"
                },
            )),
        }
    }
    fn query(self, mime: &str) -> Vec<(&'static str, String)> {
        let mut result = vec![
            (
                "file_type",
                if self.image() {
                    "a19-ui-media"
                } else {
                    "a19-ui-res"
                }
                .into(),
            ),
            (
                "business_id",
                if self.image() {
                    "img"
                } else if self == Self::PluginArchive {
                    "addon"
                } else {
                    "wa"
                }
                .into(),
            ),
            ("mime_type", mime.into()),
        ];
        match self {
            Self::PluginArchive => result.push(("file_name", "addon.zip".into())),
            Self::WaMaterials => result.push(("file_name", "wa_materials.zip".into())),
            Self::PluginImage => result.push(("file_name", String::new())),
            Self::ConfigImage | Self::WaImage => {}
        }
        result
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    path: PathBuf,
    kind: UploadKind,
    sha256: String,
    size: u64,
}
pub struct ObjectUpload {
    pub(super) request: Request,
}
impl ObjectUpload {
    /// Pure local selection: plans never open a task, authenticate or write home.
    pub fn select(path: &Path, kind: UploadKind, control: &OperationControl) -> Result<Self> {
        Self::select_local(path, kind, false, control)
    }
    pub(super) fn plugin_archive(path: &Path, control: &OperationControl) -> Result<Self> {
        Self::select_local(path, UploadKind::PluginArchive, true, control)
    }
    fn select_local(
        path: &Path,
        kind: UploadKind,
        check_archive: bool,
        control: &OperationControl,
    ) -> Result<Self> {
        control.check()?;
        kind.mime(path)?;
        let (mut file, sha256) = storage::locked_sha256(path, kind.maximum(), control)?;
        let result = (|| {
            if check_archive {
                plugin_zip(&mut file, control)?;
            }
            let size = file.metadata()?.len();
            if size == 0 {
                return Err(invalid("$.file", "DD upload file must not be empty"));
            }
            Ok(Self {
                request: Request {
                    path: path.canonicalize()?,
                    kind,
                    sha256,
                    size,
                },
            })
        })();
        storage::finish_file(file, result, control)
    }
    pub fn plan(&self) -> Value {
        json!({"file":self.request.path,"kind":self.request.kind,"sha256":self.request.sha256,"bytes":self.request.size,
            "remote_write":false,"authentication":false,"original_bytes_rechecked_at_upload":true})
    }
    /// Explicitly selected retained task. The worker opens and hashes the file
    /// again before authorization, then keeps its deny-write/delete handle.
    pub fn execute(&self, client: &Client, control: &OperationControl) -> Result<Value> {
        client.upload(&self.request, control)
    }
}
fn plugin_zip(file: &mut std::fs::File, control: &OperationControl) -> Result<()> {
    use std::io::Read;
    let invalid_zip = || {
        invalid(
            "$.file",
            "DD plugin file must be a valid ZIP archive with forward-slash entry names",
        )
    };
    file.seek(SeekFrom::Start(0))?;
    let offsets = {
        let mut archive = zip::ZipArchive::new(&mut *file).map_err(|_| invalid_zip())?;
        let mut offsets = Vec::new();
        for index in 0..archive.len() {
            control.check()?;
            let member = archive.by_index_raw(index).map_err(|_| invalid_zip())?;
            if member.name_raw().contains(&b'\\') {
                return Err(invalid_zip());
            }
            offsets.push(member.header_start());
        }
        offsets
    };
    for offset in offsets {
        control.check()?;
        file.seek(SeekFrom::Start(offset))?;
        let mut header = [0; 30];
        file.read_exact(&mut header).map_err(|_| invalid_zip())?;
        if &header[..4] != b"PK\x03\x04" {
            return Err(invalid_zip());
        }
        let size = u16::from_le_bytes(header[26..28].try_into().unwrap()) as usize;
        let mut name = vec![0; size];
        file.read_exact(&mut name).map_err(|_| invalid_zip())?;
        if name.contains(&b'\\') {
            return Err(invalid_zip());
        }
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    url: String,
    sha256: String,
    bytes: u64,
    kind: UploadKind,
    operation_id: String,
    verification: String,
}
fn data(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_object", message, 5)
}
fn artifact(value: &str) -> Result<Url> {
    let url = Url::parse(value)
        .map_err(|_| data("DD object authorization omitted a usable HTTPS target"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(data(
            "DD object authorization returned an unsupported target",
        ));
    }
    Ok(url)
}
pub(super) fn permanent(value: &str) -> Result<()> {
    artifact(value)?;
    if output::public(json!({"url":value}), &[])["url"] != value {
        return Err(data(
            "DD upload needs a permanent artifact address; its temporary capability is not stored",
        ));
    }
    Ok(())
}
pub(super) fn send(
    author: &mut DdTransport,
    home: &Path,
    author_id: &str,
    request: &Request,
    control: &OperationControl,
) -> Result<Value> {
    let mut put_started = false;
    send_inner(author, home, author_id, request, control, &mut put_started).map_err(|error| {
        if matches!(
            error.details.get("submission").and_then(Value::as_str),
            Some("not_sent" | "rejected" | "uncertain" | "pending")
        ) {
            error
        } else {
            super::submission(
                error,
                true,
                if put_started { "uncertain" } else { "not_sent" },
            )
        }
    })
}
fn send_inner(
    author: &mut DdTransport,
    home: &Path,
    author_id: &str,
    request: &Request,
    control: &OperationControl,
    put_started: &mut bool,
) -> Result<Value> {
    control.check()?;
    if !request.path.is_absolute()
        || request.sha256.len() != 64
        || !request
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || author_id.trim().is_empty()
        || author_id.len() > 1024
        || author_id.chars().any(char::is_control)
    {
        return Err(ToolkitError::invalid("invalid private DD upload selection"));
    }
    let mime = request.kind.mime(&request.path)?;
    let (mut file, digest) =
        storage::locked_sha256(&request.path, request.kind.maximum(), control)?;
    let result = (|| {
        if digest != request.sha256 || file.metadata()?.len() != request.size || request.size == 0 {
            return Err(invalid(
                "$.file",
                "DD upload bytes changed after the original selection",
            ));
        }
        let identity = json!({"author":author_id,"sha256":digest,"bytes":request.size,"kind":request.kind,"mime_type":mime});
        let scope = storage::sha256(&storage::identity_json(&identity)?);
        let target = u64::from_str_radix(&scope[..16], 16).unwrap().max(1);
        let mut mutation = Mutation::open(
            home,
            Provider::Dd,
            "object",
            target,
            "upload",
            &identity,
            control,
        )?;
        if let Some(value) = mutation.evidence() {
            let receipt: Receipt = serde_json::from_value(value.clone())
                .map_err(|_| ToolkitError::invalid("invalid original DD object receipt"))?;
            permanent(&receipt.url)?;
            if receipt.sha256 != digest
                || receipt.bytes != request.size
                || receipt.kind != request.kind
                || Some(receipt.operation_id.as_str()) != mutation.id()
                || receipt.verification != "object_put_200"
            {
                return Err(ToolkitError::invalid(
                    "DD object receipt differs from its original file or purpose",
                ));
            }
            let mut value = value.clone();
            value["reused"] = json!(true);
            return Ok(value);
        }
        if mutation.pending() {
            return Err(mutation.pending_error("the original DD object PUT is uncertain; inspect the original upload before a new submission"));
        }
        let authorization = author.get("/file/upload", &request.kind.query(mime))?;
        let info = authorization
            .get("result")
            .and_then(Value::as_object)
            .ok_or_else(|| data("DD upload authorization omitted its result"))?;
        let put = artifact(
            info.get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| data("DD upload authorization omitted its object target"))?,
        )?;
        let download = info.get("d_url").and_then(Value::as_str).ok_or_else(|| {
            data("DD upload authorization omitted its permanent artifact address")
        })?;
        permanent(download)?;
        let maximum = match info.get("maxSize") {
            None | Some(Value::Null) => 0,
            Some(Value::Number(v)) => v
                .as_u64()
                .ok_or_else(|| data("DD upload limit is not a nonnegative integer"))?,
            Some(Value::String(v)) => v
                .parse::<u64>()
                .map_err(|_| data("DD upload limit is not a nonnegative integer"))?,
            _ => return Err(data("DD upload limit has an invalid type")),
        };
        if maximum != 0 && request.size > maximum {
            return Err(invalid(
                "$.file",
                "selected file exceeds the DD server upload limit",
            ));
        }
        file.seek(SeekFrom::Start(0))?;
        let stream = file.try_clone()?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(mime));
        headers.insert("x-amz-acl", HeaderValue::from_static("public-read"));
        mutation.prepare(control)?;
        *put_started = true;
        let response = author
            .http
            .put_file(put, headers, stream)
            .map_err(|e| mutation.failure(e))?;
        if response.get("http_status").and_then(Value::as_u64) != Some(200) {
            return Err(mutation.failure(data(
                "DD object PUT did not return its required HTTP 200 acknowledgement",
            )));
        }
        let receipt = Receipt {
            url: download.into(),
            sha256: digest,
            bytes: request.size,
            kind: request.kind,
            operation_id: mutation.id().unwrap().into(),
            verification: "object_put_200".into(),
        };
        let value = serde_json::to_value(receipt)?;
        mutation.verified_with(value.clone()).map_err(|_| {
            mutation.pending_error(
                "DD object PUT completed; finish saving its original receipt before another upload",
            )
        })?;
        let mut value = value;
        value["reused"] = json!(false);
        Ok(value)
    })();
    storage::finish_file(file, result, control)
}

#[cfg(test)]
mod tests;
