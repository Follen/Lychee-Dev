use super::*;
use crate::{input::invalid, mutation::Mutation, provider::Provider};
use lycheedev_core::storage;
use std::io::{Read, Seek, SeekFrom};

enum MediaKind {
    Image,
    Attachment,
}
pub struct WaMediaUpload {
    path: PathBuf,
    kind: MediaKind,
    install_type: Option<i64>,
    install_path: Option<String>,
}
impl WaMediaUpload {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        let value = crate::input::document(bytes)?;
        crate::contract::input_shape(Provider::Newbee, "wa-media", "upload")?.admit(&value)?;
        let path = cwd.join(value["file"].as_str().unwrap());
        if !path.is_file() {
            return Err(invalid("$.file", "media must be an existing regular file"));
        }
        let kind = if value["kind"] == "image" {
            MediaKind::Image
        } else {
            MediaKind::Attachment
        };
        if matches!(kind, MediaKind::Image)
            && (value.contains_key("install_type") || value.contains_key("install_path"))
        {
            return Err(invalid(
                "$.kind",
                "install_type and install_path apply to attachment uploads",
            ));
        }
        let install_type = value
            .get("install_type")
            .map(|value| {
                value.as_i64().ok_or_else(|| {
                    invalid(
                        "$.install_type",
                        "install_type must fit a signed 64-bit integer",
                    )
                })
            })
            .transpose()?;
        Ok(Self {
            path,
            kind,
            install_type,
            install_path: value
                .get("install_path")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }
    pub fn schema() -> Value {
        let mut schema = crate::contract::input_shape(Provider::Newbee, "wa-media", "upload")
            .unwrap()
            .schema();
        schema.as_object_mut().unwrap().remove("x-validation-scope");
        schema["allOf"] = json!([
            {"if":{"properties":{"kind":{"const":"image"}},"required":["kind"]},
             "then":{"not":{"anyOf":[{"required":["install_type"]},{"required":["install_path"]}]}}}
        ]);
        plugin::metadata_schema(&mut schema);
        schema
    }
}

pub(super) fn mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "zip" => "application/zip",
        "rar" => "application/vnd.rar",
        "7z" => "application/x-7z-compressed",
        "txt" | "lua" => "text/plain",
        "json" => "application/json",
        "xml" => "application/xml",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}
pub(super) fn request_identity(
    doc: &Value,
    cwd: &Path,
    control: &OperationControl,
) -> Result<Value> {
    let mut files = Vec::new();
    for field in [
        "logo_file",
        "thumbnail_file",
        "screenshot_files",
        "picture_files",
        "image_files",
    ] {
        let Some(value) = doc.get(field) else {
            continue;
        };
        let paths = if field.ends_with("_files") {
            value.as_array().unwrap().iter().collect::<Vec<_>>()
        } else {
            vec![value]
        };
        for (index, value) in paths.iter().enumerate() {
            let path = cwd.join(value.as_str().unwrap());
            let (file, sha256) = storage::locked_sha256(&path, u64::MAX, control)?;
            let length = file.metadata()?.len();
            storage::finish_file(file, Ok(()), control)?;
            files.push(json!({"field":field,"index":index,"sha256":sha256,"length":length}));
        }
    }
    Ok(json!({"document":doc,"local_files":files}))
}
impl Newbee {
    pub fn upload_wa_media(&mut self, request: &WaMediaUpload) -> Result<Value> {
        match request.kind {
            MediaKind::Image => self.upload_image(Resource::Wa, &request.path),
            MediaKind::Attachment => self.upload_attachment(
                &request.path,
                request.install_type,
                request.install_path.as_deref(),
            ),
        }
    }
    pub fn upload_attachment(
        &mut self,
        path: &Path,
        install_type: Option<i64>,
        install_path: Option<&str>,
    ) -> Result<Value> {
        let (mut file, digest) = storage::locked_sha256(path, u64::MAX, &self.control)?;
        let result = (|| {
            use md5::Digest as _;
            let filename = path
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or_else(|| invalid("$.file", "attachment basename must be UTF-8"))?;
            let size = file.metadata()?.len();
            let mut hash = md5::Md5::new();
            file.seek(SeekFrom::Start(0))?;
            let mut buffer = [0u8; 65536];
            loop {
                self.control.check()?;
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            let code = hex::encode(hash.finalize());
            let target = u64::from_str_radix(&digest[..16], 16).unwrap().max(1);
            let mut mutation = Mutation::open(
                &self.home,
                Provider::Newbee,
                "wa-attachment",
                target,
                "upload",
                &json!({"sha256":digest,"name":filename,"length":size}),
                &self.control,
            )?;
            let uploaded = if let Some(value) = mutation
                .evidence()
                .filter(|value| value.get("value").is_some())
            {
                value.clone()
            } else {
                let index = if mutation.pending() {
                    mutation.evidence().and_then(|value|value["index_id"].as_str()).map(str::to_owned)
                    .ok_or_else(||mutation.pending_error("attachment preparation is uncertain; inspect the original upload before another submission"))?
                } else {
                    mutation.prepare(&self.control)?;
                    let body = json!({"code":code,"indexType":2,"fileName":filename,"files":[{"fullHash":code,"totalSize":size,"chunks":[{"hash":code,"size":size}]}],"checksumAlgorithm":1});
                    let response = self
                        .http
                        .post(
                            Origin::NewbeeNext,
                            "/uploadserver/upload/v3/prepare",
                            HeaderMap::new(),
                            &body,
                            true,
                        )
                        .map_err(|error| mutation.failure(error))?;
                    let prepared = envelope(response, "/uploadserver/upload/v3/prepare", true)
                        .map_err(|error| mutation.failure(error))?;
                    let index = match prepared.get("id") {
                        Some(Value::String(value)) if !value.is_empty() => value.clone(),
                        Some(Value::Number(value)) => value.to_string(),
                        _ => {
                            return Err(mutation.pending_error(
                                "attachment preparation omitted its content index identity",
                            ));
                        }
                    };
                    mutation.remember(json!({"index_id":index}))?;
                    let item = &prepared["items"][&code];
                    if item["exists"] != true {
                        if item["exists"] != false {
                            return Err(mutation.pending_error(
                                "attachment preparation omitted the object existence state",
                            ));
                        }
                        let url = item["url"].as_str().ok_or_else(|| {
                            mutation.pending_error(
                                "attachment preparation omitted the object capability",
                            )
                        })?;
                        let url =
                            object_url(url, false).map_err(|error| mutation.failure(error))?;
                        let callback = item["callback"]
                            .as_str()
                            .filter(|value| !value.is_empty())
                            .ok_or_else(|| {
                                mutation.pending_error(
                                    "attachment preparation omitted its object callback",
                                )
                            })?;
                        let headers = crate::transport::secret_header("x-oss-callback", callback)?;
                        file.seek(SeekFrom::Start(0))?;
                        let stream = file.try_clone()?;
                        self.http
                            .put_file(url, headers, stream)
                            .map_err(|error| mutation.failure(error))?;
                    }
                    mutation.acknowledged().map_err(|_| {
                        mutation.pending_error(
                            "attachment object was accepted; finish saving its receipt",
                        )
                    })?;
                    index
                };
                let response = self
                    .http
                    .post(
                        Origin::NewbeeNext,
                        "/uploadserver/upload/v3/index/get",
                        HeaderMap::new(),
                        &json!({"code":index}),
                        false,
                    )
                    .and_then(|value| envelope(value, "/uploadserver/upload/v3/index/get", false))
                    .map_err(|error| {
                        mutation
                            .readback_error("attachment content-index readback is pending", error)
                    })?;
                let value = response["code"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        mutation.pending_error("attachment index has no reusable content code")
                    })?;
                if response
                    .get("totalSize")
                    .is_some_and(|value| value.as_u64() != Some(size))
                {
                    return Err(mutation.pending_error(
                        "attachment content-index length differs from the locked local file",
                    ));
                }
                let result = json!({"file_id":0,"name":response.get("fileName").and_then(Value::as_str).unwrap_or(filename),"value":value,"size":size,"type":mime(path),"timestamp":0,"sha256":digest,"operation_id":mutation.id(),"verification":"content_index"});
                if output::public(result.clone(), &[]) != result {
                    return Err(mutation.pending_error("attachment index returned a private capability rather than a reusable content reference"));
                }
                mutation.verified_with(result.clone())?;
                result
            };
            let mut attachment = json!({"name":uploaded["name"],"value":uploaded["value"],"is_compressed":true,"timestamp":0});
            if let Some(value) = install_type {
                attachment["install_type"] = json!(value);
            }
            if let Some(value) = install_path {
                attachment["install_path"] = json!(value);
            }
            Ok(json!({"upload":uploaded,"attachment":attachment}))
        })();
        storage::finish_file(file, result, &self.control)
    }
    /// Immutable uploaded images are cached by the locked local bytes. An
    /// uncertain media upload is retained rather than sent a second time.
    pub fn upload_image(&mut self, resource: Resource, path: &Path) -> Result<Value> {
        self.image(resource, path, true)
    }
    pub(super) fn image(
        &mut self,
        resource: Resource,
        path: &Path,
        allow_upload: bool,
    ) -> Result<Value> {
        let (mut file, digest) = storage::locked_sha256(path, u64::MAX, &self.control)?;
        let result = (|| {
            let filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| invalid("$.file", "upload file needs a UTF-8 basename"))?;
            let identity = json!({"sha256":digest,"name":filename,"length":file.metadata()?.len(),"author_subject":self.subject()?});
            let scope = storage::sha256(&storage::identity_json(&identity)?);
            let target = u64::from_str_radix(&scope[..16], 16).unwrap().max(1);
            let mut mutation = Mutation::open(
                &self.home,
                Provider::Newbee,
                &format!("{}-image", resource.key()),
                target,
                "upload",
                &identity,
                &self.control,
            )?;
            if let Some(value) = mutation.evidence() {
                return Ok(value.clone());
            }
            if mutation.pending() {
                return Err(mutation.pending_error("the original image upload is uncertain; inspect the author media before a new upload"));
            }
            if !allow_upload {
                return Err(ToolkitError::new(
                    "publish.media_pending",
                    "restore the original uploaded image before recovering the author write",
                    6,
                ));
            }
            let path = match resource {
                Resource::Plugin => "/creator/wow/mod/upload_media",
                Resource::Config => "/creator/wow/share_config/upload",
                Resource::Wa => "/creator/wow/wa/upload_media",
            };
            let headers = self.headers()?;
            file.seek(SeekFrom::Start(0))?;
            let stream = file.try_clone()?;
            mutation.prepare(&self.control)?;
            let response = self
                .http
                .multipart_file(
                    Origin::NewbeeCreator,
                    path,
                    headers,
                    &[],
                    filename,
                    mime(Path::new(filename)),
                    stream,
                )
                .map_err(|error| mutation.failure(error))?;
            let data = envelope(response, path, true).map_err(|error| mutation.failure(error))?;
            let value = match data {
                Value::String(value) => Value::String(value),
                Value::Array(items) => items
                    .into_iter()
                    .next()
                    .map(|item| {
                        if item.is_string() {
                            item
                        } else {
                            pick(&item, &["media_url", "url", "name"])
                        }
                    })
                    .unwrap_or(Value::Null),
                value => pick(&value, &["media_url", "url", "name"]),
            };
            let url = value.as_str().filter(|v| !v.is_empty()).ok_or_else(|| {
                mutation
                    .pending_error("image upload was accepted but did not return a reusable URL")
            })?;
            if output::public(json!({"url":url}), &[])["url"] != url {
                return Err(mutation.pending_error("image upload returned a temporary capability URL; inspect the author media for its permanent address"));
            }
            let result = json!({"url":url,"sha256":digest,"operation_id":mutation.id(),"verification":"upload_response"});
            mutation.verified_with(result.clone()).map_err(|_| {
                mutation
                    .pending_error("image upload was accepted; finish saving its original receipt")
            })?;
            Ok(result)
        })();
        storage::finish_file(file, result, &self.control)
    }
    pub(super) fn resolve_images(
        &mut self,
        resource: Resource,
        existing: &Value,
        files: Option<&Value>,
        cwd: &Path,
        allow_upload: bool,
    ) -> Result<Value> {
        let mut values = urls(existing.clone())?;
        if let Some(files) = files.and_then(Value::as_array) {
            for file in files {
                let path = cwd.join(
                    file.as_str()
                        .ok_or_else(|| invalid("$.files", "image paths must be strings"))?,
                );
                values.push(
                    self.image(resource, &path, allow_upload)?["url"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                );
            }
        }
        Ok(json!(values))
    }
}
pub(super) use crate::transport::object_url;

#[cfg(test)]
mod tests;
