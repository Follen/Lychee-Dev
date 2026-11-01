use super::{
    bundle::{BackupGenerate, GROUPS, Validated},
    *,
};
use crate::{contract, input, mutation::Mutation, provider::Provider};
use lycheedev_core::storage;
use reqwest::{
    Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::dd) struct UploadRequest {
    pub path: PathBuf,
    pub sha256: String,
    pub recover_sn: Option<String>,
}
pub struct BackupUpload {
    request: UploadRequest,
    name: Option<String>,
}
impl BackupUpload {
    pub fn parse(bytes: &[u8], cwd: &Path, control: &OperationControl) -> Result<Self> {
        let v = input::document(bytes)?;
        contract::input_shape(Provider::Dd, "backup", "upload")?.admit(&v)?;
        Self::select(
            &cwd.join(v["bundle_file"].as_str().unwrap()),
            v.get("name").and_then(Value::as_str),
            control,
        )
    }
    fn select(path: &Path, name: Option<&str>, control: &OperationControl) -> Result<Self> {
        if name.is_some_and(|n| n.trim().is_empty() || n.chars().count() > 14) {
            return Err(invalid(
                "$.name",
                "backup name must contain 1..14 characters",
            ));
        }
        let bundle = Validated::open(path, control)?;
        let request = UploadRequest {
            path: path.canonicalize()?,
            sha256: bundle.sha256.clone(),
            recover_sn: None,
        };
        bundle.finish(
            Ok(Self {
                request,
                name: name.map(str::to_owned),
            }),
            control,
        )
    }
    pub fn schema() -> Value {
        let mut s = contract::input_shape(Provider::Dd, "backup", "upload")
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        s["description"] = json!(
            "Upload the original generated DD bundle through an explicit retained task. Bundle hashes, ZIP members and selected roles are checked locally and again in the worker. The same author and bundle reuse the original cloud backup. Optional name is a separate verified rename. Unknown creation can be read back with --recover-sn; uncertain PUT or finish is never replayed."
        );
        s
    }
    pub fn plan(&self) -> Value {
        json!({"platform":"dd","action":"upload","bundle_file":self.request.path,"bundle_sha256":self.request.sha256,"name":self.name,"local_validation":"archives_and_selection","remote_write":false,"credential_reads":false})
    }
    pub fn execute(
        &self,
        home: &Path,
        client: &super::super::task::Client,
        recover_sn: Option<&str>,
        control: &OperationControl,
    ) -> Result<Value> {
        let mut request = self.request.clone();
        if let Some(sn) = recover_sn {
            Reference::parse(sn)?;
            request.recover_sn = Some(sn.into());
        }
        let mut result = client.backup_upload(&request, control)?;
        if let Some(name) = &self.name {
            let sn = result["sn"]
                .as_str()
                .ok_or_else(|| data("DD backup result omitted its original SN"))?;
            let rename = BackupRename::parse(&serde_json::to_vec(
                &json!({"schema":"lycheedev.publish.v1.dd.backup.rename","sn":sn,"name":name}),
            )?)?;
            match rename.execute(home, client, control) {
                Ok(value) => result["rename"] = value,
                Err(mut error) => {
                    error.details["created_backup"] = result;
                    return Err(error);
                }
            }
        }
        Ok(result)
    }
}
pub enum BackupCreate {
    Existing(BackupUpload),
    Generate {
        request: BackupGenerate,
        name: Option<String>,
    },
}
impl BackupCreate {
    pub fn parse(bytes: &[u8], cwd: &Path, control: &OperationControl) -> Result<Self> {
        let mut v = input::document(bytes)?;
        contract::input_shape(Provider::Dd, "backup", "create")?.admit(&v)?;
        let name = v.remove("name").map(|v| v.as_str().unwrap().to_owned());
        if name.as_ref().is_some_and(|n| n.trim().is_empty()) {
            return Err(invalid(
                "$.name",
                "backup name must contain 1..14 characters",
            ));
        }
        if let Some(file) = v.remove("bundle_file") {
            if v.keys().any(|k| k != "schema") {
                return Err(invalid(
                    "$",
                    "choose either bundle_file or generation fields",
                ));
            }
            Ok(Self::Existing(BackupUpload::select(
                &cwd.join(file.as_str().unwrap()),
                name.as_deref(),
                control,
            )?))
        } else {
            v.insert(
                "schema".into(),
                json!("lycheedev.publish.v1.dd.backup.generate"),
            );
            Ok(Self::Generate {
                request: BackupGenerate::parse(&serde_json::to_vec(&v)?, cwd)?,
                name,
            })
        }
    }
    pub fn schema() -> Value {
        let generate = BackupGenerate::schema();
        let mut s = contract::input_shape(Provider::Dd, "backup", "create")
            .unwrap()
            .schema();
        for (k, v) in generate["properties"].as_object().unwrap() {
            if k != "schema" {
                s["properties"][k] = v.clone();
            }
        }
        let generation_fields = generate["properties"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| k.as_str() != "schema")
            .map(|k| json!({"required":[k]}))
            .collect::<Vec<_>>();
        s["oneOf"] = json!([{"required":["bundle_file"],"not":{"anyOf":generation_fields}},{"required":["game_dir","game_type","output_dir"],"not":{"required":["bundle_file"]},"anyOf":generate["anyOf"]}]);
        s.as_object_mut().unwrap().remove("x-validation-scope");
        s["description"] = json!(
            "Use one existing bundle or explicit generation fields. A matching existing generated bundle is reused, never rebuilt on recovery. Optional name is applied after cloud creation is verified."
        );
        s
    }
    pub fn plan(&self) -> Value {
        match self {
            Self::Existing(r) => r.plan(),
            Self::Generate { request, name } => {
                let mut v = request.plan();
                v["action"] = json!("create");
                v["name"] = json!(name);
                v
            }
        }
    }
    pub fn execute(
        &self,
        home: &Path,
        client: &super::super::task::Client,
        recover_sn: Option<&str>,
        control: &OperationControl,
    ) -> Result<Value> {
        match self {
            Self::Existing(r) => r.execute(home, client, recover_sn, control),
            Self::Generate { request, name } => {
                let generated = request.generate(control)?;
                let upload = BackupUpload::select(
                    Path::new(generated["bundle_file"].as_str().unwrap()),
                    name.as_deref(),
                    control,
                )?;
                upload.execute(home, client, recover_sn, control)
            }
        }
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    before: Vec<String>,
    sn: Option<String>,
    stage: String,
    uploaded: Vec<String>,
    reused: Vec<String>,
}
fn subset(expected: &Value, actual: &Value) -> bool {
    match expected {
        Value::Object(values) => actual.as_object().is_some_and(|a| {
            values
                .iter()
                .all(|(k, v)| a.get(k).is_some_and(|x| subset(v, x)))
        }),
        Value::Array(values) => actual.as_array().is_some_and(|a| {
            a.len() == values.len() && values.iter().zip(a).all(|(e, a)| subset(e, a))
        }),
        _ => expected == actual,
    }
}
fn verify(api: &mut super::super::DdTransport, sn: &str, manifest: &Value) -> Result<Value> {
    let raw = super::selected(api, sn)?;
    if forms::scalar(&raw["game_type"]) != forms::scalar(&manifest["game_type"]) {
        return Err(data("DD backup game type differs from the original bundle"));
    }
    for group in GROUPS {
        let field = if group == "wtf" { "accounts" } else { "items" };
        let expected = &manifest[group][field];
        let empty = json!([]);
        let actual = if raw[group].is_null() && expected.as_array().is_some_and(Vec::is_empty) {
            &empty
        } else {
            &raw[group][field]
        };
        if !subset(expected, actual) {
            return Err(data(
                "DD backup selection readback differs from the original bundle",
            ));
        }
    }
    read::backup_view(&raw, sn)
}
fn response(context: &Context, m: &Mutation, readback: Value, recovered: bool) -> Value {
    forms::public(
        json!({"sn":context.sn,"uploaded_components":context.uploaded,"reused_components":context.reused,"operation_id":m.id(),"recovered":recovered,"verification_scope":"cloud_selection_metadata","archive_bytes_verified":false,"readback":readback,"automatic_retry":false}),
    )
}
pub(in crate::dd) fn send(
    author: &mut super::super::DdTransport,
    home: &Path,
    actor: &str,
    request: &UploadRequest,
    control: &OperationControl,
) -> Result<Value> {
    let mut bundle = Validated::open(&request.path, control)?;
    let result = (|| {
        if bundle.sha256 != request.sha256 {
            return Err(invalid(
                "$.bundle_file",
                "DD bundle changed after original selection",
            ));
        }
        let input = json!({"author":actor,"bundle_sha256":bundle.sha256});
        let scope = storage::sha256(&storage::identity_json(&input)?);
        let target = u64::from_str_radix(&scope[..16], 16).unwrap().max(1);
        let mut m = Mutation::open(
            home,
            Provider::Dd,
            "backup-create",
            target,
            "create",
            &input,
            control,
        )?;
        let mut context = m
            .evidence()
            .map(|v| {
                serde_json::from_value::<Context>(v.clone()).map_err(|_| {
                    ToolkitError::invalid("invalid original DD backup creation record")
                })
            })
            .transpose()?
            .unwrap_or_default();
        if let Some(sn) = &context.sn {
            if request.recover_sn.as_ref().is_some_and(|r| r != sn) {
                return Err(invalid(
                    "--recover-sn",
                    "recovery must use the original DD backup SN",
                ));
            }
            if !matches!(context.stage.as_str(), "finish_pending" | "complete") {
                let mut e=m.pending_error("the original DD backup upload is incomplete; inspect the retained SN and components without repeating pre_create or PUT");
                e.details["sn"] = json!(sn);
                e.details["uploaded_components"] = json!(context.uploaded);
                e.details["stage"] = json!(context.stage);
                return Err(e);
            }
            let readback = verify(author, sn, &bundle.spec.manifest)
                .map_err(|e| m.readback_error("original DD backup readback is pending", e))?;
            context.stage = "complete".into();
            m.verified_with(serde_json::to_value(&context)?)?;
            return Ok(response(&context, &m, readback, true));
        }
        if m.pending() {
            let sn=request.recover_sn.as_deref().ok_or_else(||m.pending_error("DD pre_create result is unknown; inspect the original backup list and supply --recover-sn for read-only verification"))?;
            Reference::parse(sn)?;
            if context.before.iter().any(|old| old == sn) {
                return Err(invalid(
                    "--recover-sn",
                    "selected backup existed before the original creation",
                ));
            }
            let readback = verify(author, sn, &bundle.spec.manifest)
                .map_err(|e| m.readback_error("selected recovery backup is not verified", e))?;
            // No original finish acknowledgement exists. Preserve the discovered
            // identity, but do not promote pre_create metadata to completed upload.
            context.sn = Some(sn.into());
            context.stage = "creation_unconfirmed".into();
            m.remember(serde_json::to_value(&context)?)?;
            let mut e=m.pending_error("original backup identity was found; upload completion has no acknowledgement and no writes were replayed");
            e.details["sn"] = json!(sn);
            e.details["readback"] = readback;
            return Err(e);
        }
        if request.recover_sn.is_some() {
            return Err(invalid(
                "--recover-sn",
                "there is no pending original backup creation to recover",
            ));
        }
        let games = author.get("/game_type/list", &[])?;
        if !read::items(&games)?
            .iter()
            .any(|v| v["game_type"] == bundle.spec.manifest["game_type"])
        {
            return Err(invalid(
                "$.game_type",
                "game type is unavailable in the retained author session",
            ));
        }
        context = Context {
            before: super::list(author)?
                .iter()
                .map(super::reference)
                .collect::<Result<_>>()?,
            stage: "pre_create_pending".into(),
            ..Context::default()
        };
        m.prepare_with_evidence(serde_json::to_value(&context)?, control)?;
        let prepared = author
            .post("/backup/pre_create", &[], &bundle.spec.manifest, true)
            .map_err(|e| m.failure(e))?;
        let info = &prepared["result"];
        let sn = info["sn"]
            .as_str()
            .filter(|s| Reference::parse(s).is_ok())
            .ok_or_else(|| {
                m.pending_error(
                    "DD pre_create omitted the original backup SN; do not repeat creation",
                )
            })?;
        if context.before.iter().any(|old| old == sn) {
            return Err(m.pending_error("DD pre_create returned an existing backup identity"));
        }
        context.sn = Some(sn.into());
        context.stage = "prepared".into();
        m.remember(serde_json::to_value(&context)?)?;
        m.acknowledged()?;
        let uploaded = (|| {
            for (artifact, file) in bundle.spec.artifacts.iter().zip(&mut bundle.files) {
                let grant = info
                    .get(&artifact.group)
                    .and_then(Value::as_object)
                    .ok_or_else(|| data("DD preparation omitted a selected upload component"))?;
                match grant.get("u_url") {
                    None | Some(Value::Null) => context.reused.push(artifact.group.clone()),
                    Some(Value::String(url)) if url.is_empty() => {
                        context.reused.push(artifact.group.clone())
                    }
                    Some(Value::String(url)) => {
                        let (url, headers) = upload_target(url)?;
                        file.seek(SeekFrom::Start(0))?;
                        context.stage = format!("put_pending:{}", artifact.group);
                        m.remember(serde_json::to_value(&context)?)?;
                        let reply = author.http.put_file(url, headers, file.try_clone()?)?;
                        if !matches!(reply["http_status"].as_u64(), Some(200 | 201)) {
                            return Err(data(
                                "DD backup object PUT did not acknowledge its original bytes",
                            ));
                        }
                        context.uploaded.push(artifact.group.clone());
                    }
                    _ => return Err(data("DD backup upload target has an invalid shape")),
                }
                context.stage = "components_in_progress".into();
                m.remember(serde_json::to_value(&context)?)?;
            }
            context.stage = "finish_pending".into();
            m.remember(serde_json::to_value(&context)?)?;
            if let Err(error) = author.post("/backup/finish_create", &[], &json!({"sn":sn}), true) {
                if matches!(
                    error.details["submission"].as_str(),
                    Some("not_sent" | "rejected")
                ) {
                    context.stage =
                        format!("finish_{}", error.details["submission"].as_str().unwrap());
                    m.remember(serde_json::to_value(&context)?)?;
                }
                return Err(error);
            }
            let readback = verify(author, sn, &bundle.spec.manifest)?;
            context.stage = "complete".into();
            m.verified_with(serde_json::to_value(&context)?)?;
            Ok(response(&context, &m, readback, false))
        })();
        uploaded.map_err(|e|{let mut e=m.readback_error("DD backup creation needs recovery; reuse the original bundle and retained task, never repeat submitted uploads",e);e.details["sn"]=json!(context.sn);e.details["stage"]=json!(context.stage);e.details["uploaded_components"]=json!(context.uploaded);e.details["reused_components"]=json!(context.reused);e})
    })();
    bundle.finish(result, control)
}
fn upload_target(value: &str) -> Result<(Url, HeaderMap)> {
    let mut url = Url::parse(value).map_err(|_| data("DD backup upload URL is malformed"))?;
    if url.scheme() == "http"
        && url.host_str() == Some("a19-ui-res.s3v2.nie.netease.com")
        && url.port_or_known_default() == Some(80)
        && url.username().is_empty()
        && url.password().is_none()
    {
        url.set_scheme("https")
            .map_err(|_| data("invalid DD backup upload scheme"))?;
        url.set_port(None)
            .map_err(|_| data("invalid DD backup upload port"))?;
    }
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(data(
            "DD backup upload requires a credential-free HTTPS authority",
        ));
    }
    let mut headers = HeaderMap::new();
    for (key, value) in url.query_pairs() {
        let key = key.to_ascii_lowercase();
        if !matches!(key.as_str(), "content-type" | "x-amz-acl") {
            continue;
        }
        let key =
            HeaderName::from_bytes(key.as_bytes()).map_err(|_| data("invalid signed DD header"))?;
        if headers.contains_key(&key) || value.is_empty() {
            return Err(data("DD backup upload contains ambiguous signed headers"));
        }
        headers.insert(
            key,
            HeaderValue::from_str(&value).map_err(|_| data("invalid signed DD header value"))?,
        );
    }
    if !headers.contains_key("content-type") {
        headers.insert(
            "content-type",
            HeaderValue::from_static("application/x-zip-compressed"),
        );
    }
    if !headers.contains_key("x-amz-acl") {
        headers.insert("x-amz-acl", HeaderValue::from_static("public-read"));
    }
    Ok((url, headers))
}

#[cfg(test)]
mod tests;
