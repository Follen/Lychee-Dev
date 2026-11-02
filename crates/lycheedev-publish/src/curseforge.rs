//! CurseForge's public Core API and author Upload API have separate credentials.
//! Public author search never restricts the user's explicit upload project ID.
use crate::{
    contract::input_shape,
    input::{document, invalid},
    output,
    provider::Provider,
    transport::{Http, Origin, secret_header},
};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha1::{Digest, Sha1};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const CONFIG_KEYS: [&str; 3] = [
    "CURSEFORGE_AUTHOR_ID",
    "CURSEFORGE_API_KEY",
    "CURSEFORGE_UPLOAD_TOKEN",
];

/// Secret values have no Debug/Serialize implementation and are only used in
/// sensitive HTTP headers. This loader never imports the old Fuploader profile.
pub struct Config {
    path: PathBuf,
    author_id: Option<u64>,
    api_key: Option<String>,
    upload_token: Option<String>,
}
impl Config {
    pub fn path(home: &Path) -> PathBuf {
        home.join("publish").join("curseforge.env")
    }
    pub fn load(home: &Path) -> Result<Self> {
        let path = Self::path(home);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => {
                return Err(ToolkitError::new(
                    "publish.credentials_read",
                    "cannot read the CurseForge profile",
                    3,
                ));
            }
        };
        Self::parse(path, &bytes, |key| std::env::var(key).ok())
    }
    fn parse(
        path: PathBuf,
        bytes: &[u8],
        environment: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| invalid("$profile", "CurseForge profile must be UTF-8"))?
            .trim_start_matches('\u{feff}');
        let mut fields = Map::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let (name, value) = line.split_once('=').ok_or_else(|| {
                invalid("$profile", "CurseForge profile requires NAME=VALUE lines")
            })?;
            let name = name.trim();
            if !CONFIG_KEYS.contains(&name) {
                return Err(invalid("$profile", "unknown CurseForge profile field"));
            }
            if fields
                .insert(name.to_owned(), json!(value.trim()))
                .is_some()
            {
                return Err(invalid("$profile", "duplicate CurseForge profile field"));
            }
        }
        for key in CONFIG_KEYS {
            if let Some(value) = environment(key).filter(|v| !v.trim().is_empty()) {
                fields.insert(key.to_owned(), json!(value.trim()));
            }
        }
        let get = |key: &str| {
            fields
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let author_id = get(CONFIG_KEYS[0])
            .map(|value| {
                value
                    .parse::<u64>()
                    .ok()
                    .filter(|id| *id > 0)
                    .ok_or_else(|| {
                        invalid(
                            "$profile.CURSEFORGE_AUTHOR_ID",
                            "author ID must be a positive integer",
                        )
                    })
            })
            .transpose()?;
        Ok(Self {
            path,
            author_id,
            api_key: get(CONFIG_KEYS[1]),
            upload_token: get(CONFIG_KEYS[2]),
        })
    }
    fn required<'a>(&'a self, value: &'a Option<String>, key: &str) -> Result<&'a str> {
        value.as_deref().ok_or_else(|| {
            ToolkitError::new(
                "publish.credentials_missing",
                "a required CurseForge credential is missing",
                3,
            )
            .with_details(json!({"config_path":self.path,"missing":[key]}))
        })
    }
    pub fn doctor(&self) -> Value {
        let present = [
            self.author_id.is_some(),
            self.api_key.is_some(),
            self.upload_token.is_some(),
        ];
        json!({"config_path":self.path,"fields":CONFIG_KEYS.iter().zip(present).map(|(name,present)|json!({"name":name,"present":present})).collect::<Vec<_>>(),
            "ready":present.into_iter().all(|value|value),"project_listing_scope":"public_wow_projects_with_author_membership",
            "upload_target":"explicit_project_id_including_private_projects"})
    }
}

pub struct Curseforge {
    config: Config,
    http: Http,
    control: OperationControl,
    home: PathBuf,
}
impl Curseforge {
    pub fn open(home: &Path, control: &OperationControl) -> Result<Self> {
        Ok(Self {
            config: Config::load(home)?,
            http: Http::new(control)?,
            control: control.clone(),
            home: home.to_owned(),
        })
    }
    pub fn doctor(&self) -> Value {
        self.config.doctor()
    }

    pub fn projects(&self, author_id: Option<u64>) -> Result<Value> {
        let key = self.config.required(&self.config.api_key, CONFIG_KEYS[1])?;
        let author = author_id
            .or(self.config.author_id)
            .filter(|id| *id > 0)
            .ok_or_else(|| ToolkitError::arguments("select a positive CurseForge author ID"))?;
        let mut projects = Vec::new();
        let mut seen = BTreeSet::new();
        let mut total = None;
        let mut pages = 0usize;
        let mut index = 0usize;
        loop {
            self.control.check()?;
            if index >= 10000 {
                return Err(platform_data(
                    "public project enumeration reached the 10000-result API limit",
                ));
            }
            let size = 50usize.min(10000 - index);
            let payload = self.http.get(
                Origin::CurseforgeCore,
                "/v1/mods/search",
                &[
                    ("gameId", "1".into()),
                    ("authorId", author.to_string()),
                    ("index", index.to_string()),
                    ("pageSize", size.to_string()),
                ],
                secret_header("x-api-key", key)?,
            )?;
            let rows = payload
                .get("data")
                .and_then(Value::as_array)
                .ok_or_else(|| platform_data("public project response has no data array"))?;
            let pagination = payload
                .get("pagination")
                .filter(|v| !v.is_null())
                .map(|v| {
                    v.as_object()
                        .ok_or_else(|| platform_data("invalid public project pagination"))
                })
                .transpose()?;
            if rows.len() > size {
                return Err(platform_data(
                    "public project page exceeds the requested size",
                ));
            }
            if let Some(pagination) = pagination {
                for (key, expected) in [("index", index), ("resultCount", rows.len())] {
                    if let Some(value) = pagination.get(key) {
                        if value.as_u64() != Some(expected as u64) {
                            return Err(platform_data(
                                "public project pagination does not match the requested page",
                            ));
                        }
                    }
                }
                if let Some(value) = pagination.get("pageSize") {
                    let size = value
                        .as_u64()
                        .filter(|n| *n <= 50 && (*n > 0 || rows.is_empty()))
                        .ok_or_else(|| platform_data("invalid public project page size"))?;
                    if size < rows.len() as u64 {
                        return Err(platform_data(
                            "public project page exceeds its declared size",
                        ));
                    }
                }
                if let Some(value) = pagination.get("totalCount") {
                    let current = value
                        .as_u64()
                        .filter(|n| *n <= 10000)
                        .ok_or_else(|| platform_data("invalid public project total"))?
                        as usize;
                    if total.is_some_and(|expected| expected != current) {
                        return Err(platform_data(
                            "public project total changed during enumeration",
                        ));
                    }
                    total = Some(current);
                }
            }
            pages += 1;
            for row in rows {
                let id = row
                    .get("id")
                    .and_then(Value::as_u64)
                    .filter(|id| *id > 0)
                    .ok_or_else(|| platform_data("public project has no positive ID"))?;
                if !seen.insert(id) {
                    return Err(platform_data(
                        "public project enumeration repeated a project ID",
                    ));
                }
                let mut project = Map::new();
                project.insert("id".to_owned(), json!(id));
                for key in ["name", "slug", "dateCreated", "dateModified"] {
                    let value = row
                        .get(key)
                        .filter(|value| value.is_string() || value.is_null())
                        .cloned()
                        .unwrap_or(Value::Null);
                    project.insert(key.to_owned(), value);
                }
                project.insert(
                    "status".to_owned(),
                    row.get("status")
                        .filter(|v| v.is_i64() || v.is_null())
                        .cloned()
                        .unwrap_or(Value::Null),
                );
                projects.push(Value::Object(project));
            }
            if let Some(total) = total {
                if projects.len() > total || rows.is_empty() && projects.len() < total {
                    return Err(platform_data("public project enumeration is incomplete"));
                }
                if projects.len() == total {
                    break;
                }
            } else if rows.is_empty() {
                total = Some(projects.len());
                break;
            }
            index += rows.len();
        }
        Ok(
            json!({"author_id":author,"game_id":1,"total_count":total,"projects":projects,"pages":pages,"complete":true,"author_scope":"member",
            "pagination":{"totalCount":total,"resultCount":seen.len()},"visibility":"public_only"}),
        )
    }

    pub fn game_versions(&self) -> Result<Value> {
        let token = self
            .config
            .required(&self.config.upload_token, CONFIG_KEYS[2])?;
        let payload = self.http.get(
            Origin::CurseforgeUpload,
            "/api/game/versions",
            &[],
            secret_header("x-api-token", token)?,
        )?;
        let rows = payload
            .as_array()
            .ok_or_else(|| platform_data("game-version response must be an array"))?;
        let mut versions = Vec::new();
        let mut seen = BTreeSet::new();
        for row in rows {
            let id = row
                .get("id")
                .and_then(Value::as_u64)
                .filter(|n| *n > 0)
                .ok_or_else(|| platform_data("game-version response has an invalid ID"))?;
            let name = row
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| platform_data("game-version response has no name"))?;
            let kind = row
                .get("gameVersionTypeID")
                .and_then(Value::as_u64)
                .filter(|n| *n > 0)
                .ok_or_else(|| platform_data("game-version response has no type ID"))?;
            if !seen.insert(id) {
                return Err(platform_data("game-version response repeated an ID"));
            }
            versions.push(json!({"id":id,"name":name,"gameVersionTypeID":kind,"slug":row.get("slug").and_then(Value::as_str)}));
        }
        Ok(json!(versions))
    }

    pub fn upload(&self, upload: &Upload) -> Result<Value> {
        let token = self
            .config
            .required(&self.config.upload_token, CONFIG_KEYS[2])?;
        self.control.check()?;
        // The same locked file is hashed, validated and streamed; replacing a
        // path after admission cannot change the uploaded archive on Windows.
        let (mut file, digest) = storage::locked_sha256(&upload.file, u64::MAX, &self.control)?;
        let outcome = (|| {
            {
                let archive = zip::ZipArchive::new(&mut file)
                    .map_err(|_| invalid("$.file", "file must be a valid ZIP archive"))?;
                if archive.is_empty() {
                    return Err(invalid("$.file", "plugin ZIP must contain files"));
                }
            }
            file.seek(SeekFrom::Start(0))?;
            let mut sha1 = Sha1::new();
            let mut buffer = [0u8; 65536];
            loop {
                self.control.check()?;
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                sha1.update(&buffer[..n]);
            }
            let sha1 = hex::encode(sha1.finalize());
            let file_length = file.metadata()?.len();
            file.seek(SeekFrom::Start(0))?;
            let filename = upload
                .file
                .file_name()
                .and_then(|s| s.to_str())
                .filter(|s| !s.contains(['\r', '\n', '"', '\\']))
                .ok_or_else(|| {
                    invalid(
                        "$.file",
                        "upload basename cannot be represented in multipart form-data",
                    )
                })?;
            let metadata = upload.metadata();
            let request = storage::sha256(&storage::identity_json(
                &json!({"project_id":upload.project_id,"file_sha256":digest,"metadata":metadata}),
            )?);
            let journal = UploadJournal::lock(&self.home, upload.project_id, &self.control)?;
            if let Some(value) = journal.previous(&request)? {
                return Ok(value);
            }
            // Validate only explicitly selected game versions. A child file has
            // no selectors; no public-project lookup is required for upload.
            if upload.game_versions.is_some() || upload.game_version_names.is_some() {
                let versions = self.game_versions()?;
                let rows = versions.as_array().expect("validated game version array");
                if upload.game_versions.as_ref().is_some_and(|ids| {
                    ids.iter()
                        .any(|id| !rows.iter().any(|v| v["id"].as_u64() == Some(*id)))
                }) {
                    return Err(invalid(
                        "$.game_versions",
                        "select IDs from the current CurseForge game-version options",
                    ));
                }
                if upload.game_version_names.as_ref().is_some_and(|names| {
                    names
                        .iter()
                        .any(|name| !rows.iter().any(|v| v["name"].as_str() == Some(name)))
                }) {
                    return Err(invalid(
                        "$.game_version_names",
                        "select names from the current CurseForge game-version options",
                    ));
                }
            }
            let mut receipt = UploadReceipt {
                schema: "lycheedev.publish.curseforge-upload.v1".into(),
                project_id: upload.project_id,
                operation_id: request.clone(),
                file_sha256: digest.clone(),
                file_sha1: sha1,
                file_length,
                archive: filename.to_owned(),
                created_at: storage::utc_now(),
                file_id: None,
                rejection_status: None,
                verification: None,
                submission: "intent_recorded".into(),
            };
            let stream = file.try_clone()?;
            let headers = secret_header("x-api-token", token)?;
            journal.intent(&receipt)?;
            let response = self
                .http
                .multipart(
                    Origin::CurseforgeUpload,
                    &format!("/api/projects/{}/upload-file", upload.project_id),
                    headers,
                    &metadata,
                    filename,
                    stream,
                )
                .map_err(|mut error| {
                    let rejected = error.code == "publish.http_status"
                        && error.details["http_status"].as_u64().is_some_and(|status| {
                            matches!(status, 400 | 401 | 403 | 404 | 413 | 415 | 422 | 429)
                        });
                    let mut receipt_pending = false;
                    let not_sent = error.details["submission"] == "not_sent";
                    if rejected || not_sent {
                        receipt.submission = if not_sent { "not_sent" } else { "rejected" }.into();
                        receipt.rejection_status =
                            error.details["http_status"].as_u64().map(|n| n as u16);
                        receipt_pending = journal.intent(&receipt).is_err();
                        error.details["submission"] = json!(receipt.submission);
                    }
                    error.details = json!({"operation_id":request,"project_id":upload.project_id,
                    "submission":if not_sent {"not_sent"}else if rejected {"rejected"}else{"uncertain"},"replay_allowed":false,
                    "receipt_pending":receipt_pending,"cause":error.details});
                    error
                })?;
            let id=response.get("id").and_then(Value::as_u64).filter(|n|*n>0).ok_or_else(||platform_data("upload response did not contain a positive integer file ID")
                .with_details(json!({"operation_id":request,"project_id":upload.project_id,"submission":"uncertain","replay_allowed":false})))?;
            receipt.file_id = Some(id);
            receipt.verification = Some("upload_response".into());
            receipt.submission = "acknowledged".into();
            journal.acknowledge(&receipt).map_err(|_|ToolkitError::new("publish.receipt_pending","upload was acknowledged; finish saving its original receipt",6)
                .with_details(json!({"operation_id":request,"project_id":upload.project_id,"file_id":id,"submission":"acknowledged","replay_allowed":false})))?;
            Ok(receipt.outcome(false))
        })();
        storage::finish_file(file, outcome, &self.control)
    }

    /// Read an original receipt; this operation sends no upload and needs no
    /// credentials. An acknowledged file can still be awaiting platform review.
    pub fn upload_status(
        home: &Path,
        project: u64,
        operation: &str,
        control: &OperationControl,
    ) -> Result<Value> {
        control.check()?;
        let root = UploadJournal::root(home, project);
        let record = UploadJournal::read_record(&root, project, operation)?;
        Ok(
            json!({"operation_id":record.operation_id,"project_id":record.project_id,"file_id":record.file_id,
            "submission":record.submission,"verification":record.verification,"rejection_status":record.rejection_status,"archive":record.archive,"created_at":record.created_at,"replay_allowed":false}),
        )
    }

    /// Resolve a lost upload response using an explicitly selected remote file.
    /// Compare the Core API's project, length and SHA-1 to the ORIGINAL locked
    /// upload, never to a newly supplied file or a changed local pathname.
    pub fn verify_upload(&self, project: u64, operation: &str, file_id: u64) -> Result<Value> {
        if project == 0 || file_id == 0 {
            return Err(ToolkitError::arguments(
                "project and file IDs must be positive",
            ));
        }
        let key = self.config.required(&self.config.api_key, CONFIG_KEYS[1])?;
        let journal = UploadJournal::lock(&self.home, project, &self.control)?;
        let mut record = journal.record(project, operation)?;
        if matches!(record.submission.as_str(), "rejected" | "not_sent") {
            return Err(ToolkitError::arguments(
                "an upload that was rejected or not sent needs a new explicit submission",
            ));
        }
        if record.file_id.is_some_and(|id| id != file_id) {
            return Err(invalid(
                "$.file_id",
                "file ID differs from the original upload acknowledgement",
            ));
        }
        let payload = self.http.get(
            Origin::CurseforgeCore,
            &format!("/v1/mods/{project}/files/{file_id}"),
            &[],
            secret_header("x-api-key", key)?,
        )?;
        let file = payload
            .get("data")
            .and_then(Value::as_object)
            .ok_or_else(|| platform_data("file verification response has no file object"))?;
        let matches = file.get("id").and_then(Value::as_u64) == Some(file_id)
            && file.get("modId").and_then(Value::as_u64) == Some(project)
            && file.get("fileLength").and_then(Value::as_u64) == Some(record.file_length)
            && file
                .get("hashes")
                .and_then(Value::as_array)
                .is_some_and(|hashes| {
                    hashes.iter().any(|hash| {
                        hash["algo"].as_u64() == Some(1)
                            && hash["value"]
                                .as_str()
                                .is_some_and(|value| value.eq_ignore_ascii_case(&record.file_sha1))
                    })
                });
        if !matches {
            return Err(ToolkitError::new("publish.upload_mismatch","remote file does not match the original upload identity",5)
            .with_details(json!({"operation_id":operation,"project_id":project,"file_id":file_id,"replay_allowed":false})));
        }
        record.file_id = Some(file_id);
        record.submission = "acknowledged".into();
        record.verification = Some("remote_file_identity".into());
        journal.verify(&record)?;
        Ok(record.outcome(true))
    }
}

#[cfg(test)]
mod tests;

fn platform_data(message: &str) -> ToolkitError {
    ToolkitError::new("publish.curseforge_data", message, 5)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upload {
    #[serde(rename = "schema")]
    _schema: String,
    project_id: u64,
    file: PathBuf,
    changelog: String,
    release_type: String,
    changelog_type: Option<String>,
    display_name: Option<String>,
    game_versions: Option<Vec<u64>>,
    game_version_names: Option<Vec<String>>,
    parent_file_id: Option<u64>,
    relations: Option<Relations>,
    is_marked_for_manual_release: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Relations {
    projects: Vec<Relation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Relation {
    slug: String,
    #[serde(rename = "type")]
    kind: String,
    project_id: Option<u64>,
}
impl Upload {
    pub fn schema() -> Value {
        let mut schema = input_shape(Provider::Curseforge, "plugin", "upload")
            .expect("CurseForge upload contract")
            .schema();
        schema.as_object_mut().unwrap().remove("x-validation-scope");
        schema["properties"]["game_versions"]["items"] = json!({"type":"integer","minimum":1});
        schema["properties"]["game_version_names"]["items"] =
            json!({"type":"string","minLength":1,"pattern":"\\S"});
        schema["properties"]["relations"] = json!({"type":"object","additionalProperties":false,"required":["projects"],
            "properties":{"projects":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["slug","type"],
                "properties":{"slug":{"type":"string","minLength":1,"pattern":"\\S"},"type":{"enum":["embeddedLibrary","incompatible","optionalDependency","requiredDependency","tool"]},"project_id":{"type":"integer","minimum":1}}}}}});
        schema["allOf"] = json!([{"if":{"required":["parent_file_id"]},"then":{"not":{"anyOf":[{"required":["game_versions"]},{"required":["game_version_names"]}]}}}]);
        schema
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let (mut file, digest) = storage::locked_sha256(&self.file, u64::MAX, control)?;
        let result = (|| {
            let length = file.metadata()?.len();
            let archive = zip::ZipArchive::new(&mut file)
                .map_err(|_| invalid("$.file", "file must be a valid ZIP archive"))?;
            if archive.is_empty() {
                return Err(invalid("$.file", "plugin ZIP must contain files"));
            }
            let filename = self
                .file
                .file_name()
                .and_then(|s| s.to_str())
                .filter(|s| !s.contains(['\r', '\n', '"', '\\']))
                .ok_or_else(|| {
                    invalid(
                        "$.file",
                        "upload basename cannot be represented in multipart form-data",
                    )
                })?;
            Ok(output::public(
                json!({"provider":"curseforge","project_id":self.project_id,"archive":filename,
                "file_bytes":length,"file_sha256":digest,"metadata":self.metadata(),
                "planned":true,"remote_writes":0,"options_validation":"performed_by_upload_for_explicit_version_selectors"}),
                &[],
            ))
        })();
        storage::finish_file(file, result, control)
    }
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        let value = document(bytes)?;
        input_shape(Provider::Curseforge, "plugin", "upload")?.admit(&value)?;
        if value
            .get("relations")
            .and_then(|r| r.get("projects"))
            .and_then(Value::as_array)
            .is_some_and(|projects| {
                projects.iter().any(|r| {
                    r.get("project_id")
                        .is_some_and(|id| id.as_u64().is_none_or(|id| id == 0))
                })
            })
        {
            return Err(invalid(
                "$.relations.projects",
                "relation project IDs must be positive integers when present",
            ));
        }
        let mut upload: Self = serde_json::from_value(Value::Object(value))
            .map_err(|_| invalid("$", "invalid CurseForge nested upload fields"))?;
        if upload
            .game_versions
            .as_ref()
            .is_some_and(|ids| ids.iter().any(|id| *id == 0))
        {
            return Err(invalid(
                "$.game_versions",
                "version IDs must be positive integers",
            ));
        }
        if upload
            .game_version_names
            .as_ref()
            .is_some_and(|names| names.iter().any(|s| s.trim().is_empty()))
        {
            return Err(invalid(
                "$.game_version_names",
                "version names must be nonempty strings",
            ));
        }
        if upload.parent_file_id.is_some()
            && (upload.game_versions.is_some() || upload.game_version_names.is_some())
        {
            return Err(invalid(
                "$.parent_file_id",
                "child uploads must omit both game-version selectors",
            ));
        }
        if let Some(relations) = &upload.relations {
            for relation in &relations.projects {
                if relation.slug.trim().is_empty()
                    || ![
                        "embeddedLibrary",
                        "incompatible",
                        "optionalDependency",
                        "requiredDependency",
                        "tool",
                    ]
                    .contains(&relation.kind.as_str())
                    || relation.project_id == Some(0)
                {
                    return Err(invalid(
                        "$.relations.projects",
                        "invalid CurseForge project relation",
                    ));
                }
            }
        }
        if upload.file.is_relative() {
            upload.file = cwd.join(&upload.file);
        }
        Ok(upload)
    }
    pub fn project_id(&self) -> u64 {
        self.project_id
    }
    fn metadata(&self) -> Value {
        let mut metadata = Map::new();
        metadata.insert("changelog".into(), json!(self.changelog));
        metadata.insert("releaseType".into(), json!(self.release_type));
        for (key, value) in [
            (
                "changelogType",
                self.changelog_type.as_ref().map(|v| json!(v)),
            ),
            ("displayName", self.display_name.as_ref().map(|v| json!(v))),
            (
                "gameVersions",
                self.game_versions.as_ref().map(|v| json!(v)),
            ),
            (
                "gameVersionNames",
                self.game_version_names.as_ref().map(|v| json!(v)),
            ),
            ("parentFileID", self.parent_file_id.map(|v| json!(v))),
            (
                "isMarkedForManualRelease",
                self.is_marked_for_manual_release.map(|v| json!(v)),
            ),
        ] {
            if let Some(value) = value {
                metadata.insert(key.into(), value);
            }
        }
        if let Some(relations) = &self.relations {
            metadata.insert("relations".into(),json!({"projects":relations.projects.iter().map(|r|{
                let mut item=json!({"slug":r.slug,"type":r.kind});if let Some(id)=r.project_id {item["projectID"]=json!(id);}item
            }).collect::<Vec<_>>()}));
        }
        Value::Object(metadata)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadReceipt {
    schema: String,
    project_id: u64,
    operation_id: String,
    file_sha256: String,
    file_sha1: String,
    file_length: u64,
    archive: String,
    created_at: String,
    file_id: Option<u64>,
    rejection_status: Option<u16>,
    verification: Option<String>,
    submission: String,
}
impl UploadReceipt {
    fn outcome(&self, reused: bool) -> Value {
        json!({"operation_id":self.operation_id,"project_id":self.project_id,"file_id":self.file_id,
        "archive":self.archive,"status":"uploaded","verification":self.verification.as_deref().unwrap_or("unknown"),
        "metadata_verification":if self.verification.as_deref()==Some("upload_response") {"accepted_by_upload_api"}else{"not_checked"},"reused":reused,"review_state":"not_checked"})
    }
}
struct UploadJournal {
    root: PathBuf,
    project: u64,
    _lock: storage::ExclusiveLock,
}
impl UploadJournal {
    fn root(home: &Path, project: u64) -> PathBuf {
        home.join("publish")
            .join("curseforge")
            .join(project.to_string())
    }
    fn lock(home: &Path, project: u64, control: &OperationControl) -> Result<Self> {
        let root = Self::root(home, project);
        let lock = storage::ExclusiveLock::resource_controlled(&root, "upload", control)?;
        Ok(Self {
            root,
            project,
            _lock: lock,
        })
    }
    fn previous(&self, request: &str) -> Result<Option<Value>> {
        let active = self.root.join("active-upload.json");
        let latest: UploadReceipt = match fs::read(&active) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| ToolkitError::invalid("invalid original CurseForge upload record"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if latest.schema != "lycheedev.publish.curseforge-upload.v1"
            || latest.project_id != self.project
        {
            return Err(ToolkitError::invalid(
                "CurseForge active upload record differs from its original target",
            ));
        }
        let historical = self.root.join(format!("{request}.json"));
        if historical.exists() {
            let record = self.record(latest.project_id, request)?;
            if record.submission == "acknowledged" && record.file_id.is_some() {
                if latest.operation_id == request && latest.submission != "acknowledged" {
                    self.acknowledge(&record)?;
                }
                return Ok(Some(record.outcome(true)));
            }
            if !matches!(record.submission.as_str(), "rejected" | "not_sent") {
                return Err(ToolkitError::new("publish.upload_pending","the original matching upload needs verification",6)
                .with_details(json!({"operation_id":record.operation_id,"project_id":record.project_id,"replay_allowed":false})));
            }
        }
        if (latest.submission == "acknowledged" && latest.file_id.is_some())
            || matches!(latest.submission.as_str(), "rejected" | "not_sent")
        {
            return Ok(None);
        }
        Err(ToolkitError::new("publish.upload_pending","resolve the original CurseForge upload before starting another upload to this project",6)
            .with_details(json!({"operation_id":latest.operation_id,"project_id":latest.project_id,"submission":latest.submission,"replay_allowed":false})))
    }
    fn intent(&self, receipt: &UploadReceipt) -> Result<()> {
        storage::atomic_json(
            &self.root.join(format!("{}.json", receipt.operation_id)),
            receipt,
        )?;
        storage::atomic_json(&self.root.join("active-upload.json"), receipt)
    }
    fn acknowledge(&self, receipt: &UploadReceipt) -> Result<()> {
        self.intent(receipt)
    }
    fn record(&self, project: u64, operation: &str) -> Result<UploadReceipt> {
        Self::read_record(&self.root, project, operation)
    }
    fn read_record(root: &Path, project: u64, operation: &str) -> Result<UploadReceipt> {
        if project == 0 {
            return Err(ToolkitError::arguments("project ID must be positive"));
        }
        if operation.len() != 64
            || !operation
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(ToolkitError::arguments(
                "operation ID must be 64 lowercase hexadecimal characters",
            ));
        }
        let bytes = fs::read(root.join(format!("{operation}.json")))?;
        let record: UploadReceipt = serde_json::from_slice(&bytes)
            .map_err(|_| ToolkitError::invalid("invalid original CurseForge upload record"))?;
        if record.schema != "lycheedev.publish.curseforge-upload.v1"
            || record.project_id != project
            || record.operation_id != operation
        {
            return Err(ToolkitError::invalid(
                "CurseForge upload record does not match its original target",
            ));
        }
        Ok(record)
    }
    fn verify(&self, receipt: &UploadReceipt) -> Result<()> {
        storage::atomic_json(
            &self.root.join(format!("{}.json", receipt.operation_id)),
            receipt,
        )?;
        let active: UploadReceipt = storage::read_json(&self.root.join("active-upload.json"))?;
        if active.operation_id == receipt.operation_id {
            storage::atomic_json(&self.root.join("active-upload.json"), receipt)?;
        }
        Ok(())
    }
}
