use super::{
    bundle::{self, Validated},
    *,
};
use base64::Engine;
use std::path::PathBuf;

pub struct BackupUpload {
    path: PathBuf,
    sha256: String,
    summary: Value,
}
impl BackupUpload {
    pub fn parse(bytes: &[u8], cwd: &Path, control: &OperationControl) -> Result<Self> {
        let doc = document(bytes)?;
        input_shape(Provider::Blackbox, "backup", "upload")?.admit(&doc)?;
        Self::select(&cwd.join(doc["bundle_file"].as_str().unwrap()), control)
    }
    fn select(path: &Path, control: &OperationControl) -> Result<Self> {
        let b = Validated::open(path, control)?;
        let result = Ok(Self {
            path: path.canonicalize()?,
            sha256: b.sha256.clone(),
            summary: json!({"archive_count":b.spec.archives.len(),"source_bytes":b.spec.source_bytes,"strappid":b.spec.configuration["Strappid"]}),
        });
        b.finish(result, control)
    }
    pub fn schema() -> Value {
        let mut s = input_shape(Provider::Blackbox, "backup", "upload")
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        s["description"] = json!(
            "Use the original backup-bundle.json from generate. Local admission checks hashes, native ZIP members and explicit characters. Upload keeps the same account, allocated backup ID, original object locations and completed components. Repeat the input to recover without replaying unknown PUT/save. --recover-backup-id explicitly selects an original allocation whose response was lost; it only verifies completed metadata, never allocates/uploads again."
        );
        s
    }
    pub fn plan(&self) -> Value {
        json!({"platform":"blackbox","planned":true,"bundle_file":self.path,"bundle_sha256":self.sha256,"bundle":self.summary,"credential_reads":false,"remote_read":false,"remote_write":false,"local_write":false})
    }
    pub fn execute(
        &self,
        home: &Path,
        recover_id: Option<&str>,
        control: &OperationControl,
    ) -> Result<Value> {
        let mut bundle = Validated::open(&self.path, control)?;
        let result = (|| {
            if bundle.sha256 != self.sha256 {
                return Err(invalid("$.bundle_file", "bundle changed after admission"));
            }
            if let Some(id) = recover_id {
                read::reference(id)?;
            }
            auth::Auth::open(home, control)?.finish(|auth| {
                let mut session = auth.renew(false)?;
                run(
                    home,
                    &auth.api,
                    &mut session,
                    &mut bundle,
                    recover_id,
                    control,
                )
            })
        })();
        bundle.finish(result, control)
    }
}
pub enum BackupCreate {
    Existing(BackupUpload),
    Generate(BackupGenerate),
}
impl BackupCreate {
    pub fn parse(bytes: &[u8], cwd: &Path, control: &OperationControl) -> Result<Self> {
        let mut doc = document(bytes)?;
        input_shape(Provider::Blackbox, "backup", "create")?.admit(&doc)?;
        if let Some(file) = doc.remove("bundle_file") {
            if doc.keys().any(|k| k != "schema") {
                return Err(invalid(
                    "$",
                    "choose bundle_file or explicit generation fields",
                ));
            }
            Ok(Self::Existing(BackupUpload::select(
                &cwd.join(file.as_str().unwrap()),
                control,
            )?))
        } else {
            doc.insert(
                "schema".into(),
                json!("lycheedev.publish.v1.blackbox.backup.generate"),
            );
            Ok(Self::Generate(BackupGenerate::parse(
                &serde_json::to_vec(&doc)?,
                cwd,
            )?))
        }
    }
    pub fn schema() -> Value {
        let mut s = input_shape(Provider::Blackbox, "backup", "create")
            .unwrap()
            .schema();
        let generate = BackupGenerate::schema();
        for (k, v) in generate["properties"].as_object().unwrap() {
            if k != "schema" {
                s["properties"][k] = v.clone();
            }
        }
        let fields = generate["properties"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|s| s.as_str() != "schema")
            .map(|s| json!({"required":[s]}))
            .collect::<Vec<_>>();
        s["oneOf"] = json!([{"required":["bundle_file"],"not":{"anyOf":fields}},{"required":["game_dir","output_dir","strappid","unknown_addons","materials","fonts","wtf_accounts"],"not":{"required":["bundle_file"]},"anyOf":generate["anyOf"]}]);
        s.as_object_mut().unwrap().remove("x-validation-scope");
        s["description"] = json!(
            "One original bundle or explicit generation fields. Matching generated artifacts are reused. Unknown upload/save recovers the original allocation; no reallocation, PUT or save replay."
        );
        s
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        match self {
            Self::Existing(v) => Ok(v.plan()),
            Self::Generate(v) => v.plan(control),
        }
    }
    pub fn execute(
        &self,
        home: &Path,
        recover_id: Option<&str>,
        control: &OperationControl,
    ) -> Result<Value> {
        match self {
            Self::Existing(v) => v.execute(home, recover_id, control),
            Self::Generate(v) => {
                let generated = v.generate(control)?;
                let path = Path::new(generated["bundle_file"].as_str().unwrap());
                let result =
                    BackupUpload::select(path, control)?.execute(home, recover_id, control);
                match result {
                    Ok(mut result) => {
                        result["bundle_file"] = json!(path);
                        Ok(result)
                    }
                    Err(mut error) => {
                        error.details["bundle_file"] = json!(path);
                        Err(error)
                    }
                }
            }
        }
    }
}
fn backup_expected(mut value: Value) -> Value {
    for key in ["HeyboxID", "Name", "SaveTime"] {
        value.as_object_mut().unwrap().remove(key);
    }
    value["ID"] = json!(identifier(&value["ID"]));
    value
}
fn checkpoint_error(
    m: &crate::mutation::Mutation,
    mut error: ToolkitError,
    id: Option<&str>,
) -> ToolkitError {
    error.details["operation_id"] = json!(m.id());
    error.details["backup_id"] = json!(id);
    error.details["replay_allowed"] = json!(false);
    error
}
pub(super) fn run(
    home: &Path,
    api: &Api,
    session: &mut Session,
    bundle: &mut Validated,
    recover_id: Option<&str>,
    control: &OperationControl,
) -> Result<Value> {
    let identity = json!({"bundle_sha256":bundle.sha256});
    let mut m = write::open(
        home,
        session,
        "backup-upload",
        "new",
        "upload",
        &identity,
        control,
    )?;
    if let Some(result) = write::completed(&m) {
        if recover_id.is_some_and(|id| result["backup_id"] != id) {
            return Err(invalid(
                "--recover-backup-id",
                "ID differs from the original completed allocation",
            ));
        }
        return Ok(result);
    }
    let recovered = m.pending();
    if !recovered && recover_id.is_some() {
        return Err(ToolkitError::arguments(
            "--recover-backup-id only inspects an original pending allocation",
        ));
    }
    if !recovered {
        let previous = write::ids(&read::backups(api, session)?, "ID")?;
        m.prepare_with_evidence(json!({"stage":"allocating","previous":previous}), control)?;
        let response = api
            .post(session, "/wow/create_backup/", &[], &json!({}))
            .map_err(|e| m.failure(e))?;
        let id=identifier(&response["ID"]).ok_or_else(||m.pending_error("allocation response omitted its ID; inspect backup list and use --recover-backup-id for read-only verification"))?;
        let mut configuration = bundle.spec.configuration.clone();
        configuration["ID"] = response["ID"].clone();
        configuration["SaveTime"] = json!(now());
        m.remember(json!({"stage":"uploading","id":id,"maximum":response["MaxSize"],"configuration":configuration,"objects":bundle.spec.archives.iter().map(|_|json!({"state":"queued"})).collect::<Vec<_>>()}))?;
    }
    let mut evidence = m
        .evidence()
        .cloned()
        .ok_or_else(|| m.pending_error("original backup upload record is missing"))?;
    if evidence["stage"] == "allocating" {
        let id=recover_id.ok_or_else(||m.pending_error("allocation outcome is pending; read backup list, then use the exact original --recover-backup-id to inspect completed metadata"))?;
        if evidence["previous"]
            .as_array()
            .is_none_or(|v| v.contains(&json!(id)))
        {
            return Err(invalid(
                "--recover-backup-id",
                "recovery ID was already present before this allocation",
            ));
        }
        let current = read::execute(api, session, Query::Backup(id.into())).map_err(|e| {
            m.readback_error("original allocated backup requires metadata readback", e)
        })?["backup"]
            .clone();
        let mut expected = bundle.spec.configuration.clone();
        expected["ID"] = current["ID"].clone();
        for artifact in &bundle.spec.archives {
            let actual = bundle::descriptor(&current, artifact)?;
            let host = forms::text(&actual["Host"], "$.original_backup.Host", true)?;
            let path = forms::text(&actual["Path"], "$.original_backup.Path", true)?;
            object_url(host, path)?;
            bundle::set_location(&mut expected, artifact, host, path)?;
        }
        let fingerprints = write::fingerprints(&backup_expected(expected))?;
        if !write::matches_fingerprints(&fingerprints, &backup_expected(current))? {
            return Err(m.pending_error("selected allocation does not contain the complete original backup metadata; allocation and upload were not replayed"));
        }
        let result = json!({"platform":"blackbox","backup_id":id,"verified":true,"metadata_verified":true,"archive_bytes_verified":false,"recovered":true,"operation_id":m.id(),"verification_scope":"explicit_original_backup_metadata"});
        m.verified_with(json!({"result":result}))?;
        return Ok(result);
    }
    let id = evidence["id"]
        .as_str()
        .ok_or_else(|| m.pending_error("original allocation ID is missing"))?
        .to_owned();
    if recover_id.is_some_and(|value| value != id) {
        return Err(invalid(
            "--recover-backup-id",
            "ID differs from the original allocation",
        ));
    }
    let maximum = evidence["maximum"]
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or_else(|| {
            m.pending_error("original allocation omitted its maximum size; no archive was uploaded")
        })?;
    if bundle.spec.source_bytes as f64 > maximum {
        return Err(checkpoint_error(
            &m,
            invalid(
                "$.bundle_file",
                "selected source bytes exceed the original allocation's MaxSize",
            ),
            Some(&id),
        ));
    }
    if evidence["stage"] == "uploading" {
        for (index, artifact) in bundle.spec.archives.iter().enumerate() {
            let state = evidence["objects"][index]["state"]
                .as_str()
                .ok_or_else(|| m.pending_error("original object state is incomplete"))?;
            if state == "uploaded" {
                continue;
            }
            if state == "uploading" {
                let descriptor = bundle::descriptor(&evidence["configuration"], artifact)?;
                let url = object_url(
                    descriptor["Host"].as_str().unwrap_or(""),
                    descriptor["Path"].as_str().unwrap_or(""),
                )?;
                let valid = api
                    .http
                    .verify_download(
                        home,
                        crate::provider::Provider::Blackbox,
                        url,
                        artifact.bytes,
                        Some(&artifact.sha256),
                        None,
                    )
                    .map_err(|e| {
                        m.readback_error(
                            "original COS PUT needs byte readback; no object was resent",
                            e,
                        )
                    })?;
                if !valid {
                    return Err(m.pending_error("original COS object does not match the selected archive; no PUT was replayed"));
                }
                evidence["objects"][index]["state"] = json!("uploaded");
                evidence["objects"][index]["bytes_verified"] = json!(true);
                m.remember(evidence.clone())?;
                continue;
            }
            let raw = api.request(
                session,
                "/wow/get_user_backup_credential/",
                &[
                    ("backup_id", id.clone()),
                    ("file_type", artifact.file_type.clone()),
                ],
                Some(&json!({})),
                false,
                None,
            )?;
            let response = protocol::checked_result(&raw)?;
            let cos_url = forms::url(&response["cos"], "$.allocation.cos")?;
            let host = cos_url.host_str().unwrap();
            if ![
                ".cos.ap-shanghai.myqcloud.com",
                ".cos.ap-beijing.myqcloud.com",
            ]
            .iter()
            .any(|s| host.ends_with(s))
                || cos_url.path() != "/"
                || cos_url.query().is_some()
            {
                return Err(protocol_error(
                    "backup credential returned an unsupported COS origin",
                ));
            }
            let path = forms::text(&response["path"], "$.allocation.path", true)?;
            let public_host = forms::text(&response["host"], "$.allocation.host", true)?;
            object_url(public_host, path)?;
            let object = cos::Object::new(host, path, &response["Credentials"], public_host)?;
            bundle::set_location(&mut evidence["configuration"], artifact, public_host, path)?;
            evidence["objects"][index]["state"] = json!("uploading");
            m.remember(evidence.clone())?;
            if let Err(error) = object.put(&api.http, &mut bundle.files[index], &artifact.md5, true)
            {
                if error.details["submission"] == "not_sent" {
                    evidence["objects"][index]["state"] = json!("queued");
                    m.remember(evidence.clone())?;
                }
                return Err(checkpoint_error(&m, error, Some(&id)));
            }
            evidence["objects"][index]["state"] = json!("uploaded");
            m.remember(evidence.clone())?;
        }
        evidence["stage"] = json!("ready");
        m.remember(evidence.clone())?;
    }
    if evidence["stage"] == "ready" {
        let bytes = serde_json::to_vec(&evidence["configuration"])?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        evidence["stage"] = json!("saving");
        m.remember(evidence.clone())?;
        if let Err(error) = api.post(
            session,
            "/wow/save_user_configuration/",
            &[("backup_id", id.clone())],
            &json!({"configuration":encoded}),
        ) {
            if error.details["submission"] == "not_sent" {
                evidence["stage"] = json!("ready");
                m.remember(evidence.clone())?;
            }
            return Err(checkpoint_error(&m, error, Some(&id)));
        }
        m.acknowledged()?;
    }
    let expected = write::fingerprints(&backup_expected(evidence["configuration"].clone()))?;
    let current = read::execute(api, session, Query::Backup(id.clone()))
        .map_err(|e| m.readback_error("backup registration needs original metadata readback", e))?
        ["backup"]
        .clone();
    if !write::matches_fingerprints(&expected, &backup_expected(current))? {
        return Err(m.pending_error("backup metadata has not converged; repeat the original bundle input to read without repeating save"));
    }
    let result = json!({"platform":"blackbox","backup_id":id,"verified":true,"metadata_verified":true,"archive_bytes_verified":false,"verification_scope":"remote_backup_metadata","archive_count":bundle.spec.archives.len(),"source_bytes":bundle.spec.source_bytes,"operation_id":m.id(),"recovered":recovered});
    m.verified_with(json!({"result":result}))?;
    Ok(result)
}
fn object_url(host: &str, path: &str) -> Result<reqwest::Url> {
    if host.is_empty()
        || path.is_empty()
        || path.starts_with('/')
        || path
            .split('/')
            .any(|p| p.is_empty() || matches!(p, "." | ".."))
        || path.chars().any(|c| c.is_control() || c == '\\')
    {
        return Err(protocol_error("original backup object location is invalid"));
    }
    let mut url = forms::url(&json!(format!("https://{host}/")), "$.original_backup.Host")?;
    if url.host_str() != Some(host) || url.path() != "/" || url.query().is_some() {
        return Err(protocol_error("original backup object host is invalid"));
    }
    url.path_segments_mut()
        .map_err(|_| protocol_error("invalid backup object path"))?
        .clear()
        .extend(path.split('/'));
    Ok(url)
}

#[cfg(test)]
mod tests;
