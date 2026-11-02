use super::manifest::Snapshot;
use super::*;
use crate::{local_backup as local, mutation::definitive_rejection};
use lycheedev_core::storage;
use reqwest::{
    Url,
    header::{CONTENT_TYPE, HeaderMap},
};
use std::{
    fs,
    io::{Seek, SeekFrom},
    path::PathBuf,
};

pub struct BackupUpload {
    path: PathBuf,
    build: Option<u8>,
}
impl BackupUpload {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        let doc = document(bytes)?;
        input_shape(Provider::Modus, "backup", "upload")?.admit(&doc)?;
        Ok(Self {
            path: std::path::absolute(cwd.join(doc["manifest_file"].as_str().unwrap()))?,
            build: doc
                .get("server_type")
                .and_then(Value::as_u64)
                .map(|n| n as u8),
        })
    }
    pub fn schema() -> Value {
        let mut s = input_shape(Provider::Modus, "backup", "upload")
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        s["x-manifest"] = json!({"schemaVersion":"config-backup.v2","sections":["addons","modifiedAddons","unknownAddons","fonts","materials","wtf"],"description":"Use backup generate or an explicitly selected current native manifest. Known addons retain projectId and locked fileId. All item IDs must be unique. WTF account/server/role hierarchy must match exact local paths; no unselected roles, links, .bak files or BigFootSync.lua. Source.gameId binds the build; a supplied server_type must match. Repeating the same manifest reuses registered objects and the original cloud backup."});
        s
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let s = Snapshot::open(&self.path, self.build, control)?;
        Ok(
            json!({"platform":"modus","resource":"backup","action":"upload","planned":true,"manifest_file":s.path,"local_backup_id":s.manifest["backupId"],"server_type":s.build,"selected_files":s.files.as_object().unwrap().len(),"object_groups":s.groups.len()+1,"local_write":false,"credential_reads":false,"remote_read":false,"remote_write":false,"next":"Execution creates/reuses selected ZIPs, uploads each object once, registers storage then the backup. Uncertain PUT reads original CDN bytes; uncertain storage registration retains its receipt without replay. The original manifest is linked only after exact backup list/detail readback."}),
        )
    }
    pub fn execute(&self, home: &Path, control: &OperationControl) -> Result<Value> {
        let snapshot = Snapshot::open(&self.path, self.build, control)?;
        let api = Api {
            http: crate::transport::Http::new(control)?,
            session: Some(auth::Session::load(Client::Main, control)?),
        };
        self.run(home, &api, &snapshot, control)
    }
    fn run(
        &self,
        home: &Path,
        api: &Api,
        s: &Snapshot,
        control: &OperationControl,
    ) -> Result<Value> {
        let account = api.account()?;
        let scope = digest(&json!({"account":account,"backup_id":s.manifest["backupId"]}))?;
        let mut m = Mutation::open(
            home,
            Provider::Modus,
            "backup-upload",
            u64::from_str_radix(&scope[..16], 16).unwrap().max(1),
            "upload",
            &json!({"account":account,"backup":s.identity()}),
            control,
        )?;
        if let Some(mut done) = completed(&m) {
            s.link(done["backup_id"].as_u64().unwrap(), control)?;
            done["manifest_file"] = json!(s.path);
            return Ok(done);
        }
        if !m.pending() && s.manifest["source"].get("cloudBackupId").is_some() {
            return Err(invalid(
                "$.manifest_file",
                "this manifest is already linked to a cloud backup; inspect backup get using source.cloudBackupId instead of uploading it again",
            ));
        }
        let recovered = m.pending();
        if !recovered {
            m.prepare_with_evidence(
                json!({"build":s.build,"phase":"objects","objects":[],"font_lookup":null}),
                control,
            )?;
        }
        let mut e = m
            .evidence()
            .ok_or_else(|| malformed("original backup upload lost its receipt"))?
            .clone();
        let directory = home.join("publish/modus/backups").join(&scope);
        let result = (|| {
            if e["phase"] == "objects" {
                storage::guard_path(home, &directory)?;
                fs::create_dir_all(&directory)?;
                if e["font_lookup"].is_null() {
                    e["font_lookup"] = json!(missing_fonts(api, &s.fonts)?);
                    m.remember(e.clone())?;
                }
                for group in &s.groups {
                    if group.kind == 3
                        && !e["font_lookup"]
                            .as_array()
                            .unwrap()
                            .contains(&json!(group.key.strip_prefix("font:").unwrap()))
                    {
                        continue;
                    }
                    let index = if let Some(i) = e["objects"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .position(|o| o["group"] == group.key)
                    {
                        i
                    } else {
                        let object = if let Some(path) = &group.font {
                            let font = s
                                .fonts
                                .iter()
                                .find(|f| {
                                    group.key == format!("font:{}", f["md5"].as_str().unwrap())
                                })
                                .unwrap();
                            let relative = local::member_path(path, &s.root)?;
                            json!({"group":group.key,"kind":group.kind,"file":path,"filename":font["md5"],"size":font["size"],"sha256":s.files[relative]["sha256"],"md5":font["md5"],"content_type":"application/octet-stream","stage":"ready"})
                        } else {
                            let filename = format!(
                                "{}_{}.zip",
                                match group.kind {
                                    8 => "modified_addons",
                                    9 => "unknown_addons",
                                    4 => "materials",
                                    _ => "wtf",
                                },
                                &digest(&json!({"scope":scope,"group":group.key}))?[..24]
                            );
                            let path = directory.join(&filename);
                            if path.exists() {
                                return Err(m.pending_error("an unfinished local archive exists without its receipt; retain the original backup and remove only the indicated incomplete archive before repeating this input").with_details(json!({"operation_id":m.id(),"incomplete_archive":path,"submission":"not_sent","replay_allowed":false})));
                            }
                            let artifact =
                                local::archive_entries(&s.root, &group.entries, &path, control)?;
                            let mut file = local::open_source(&path)?;
                            let md5 = local::md5(&mut file, control);
                            let md5 = storage::finish_file(file, md5, control)?;
                            json!({"group":group.key,"kind":group.kind,"file":path,"filename":filename,"size":artifact.size,"sha256":artifact.sha256,"md5":md5,"content_type":"application/zip","stage":"ready","account":group.account})
                        };
                        let objects = e["objects"].as_array_mut().unwrap();
                        objects.push(object);
                        let i = objects.len() - 1;
                        m.remember(e.clone())?;
                        i
                    };
                    send(home, api, &account, &mut m, &mut e, index, control)?;
                }
                let index = if let Some(i) = e["objects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|o| o["group"] == "manifest")
                {
                    i
                } else {
                    let filename = format!(
                        "{}_manifest.v2.json",
                        s.manifest["backupId"].as_str().unwrap()
                    );
                    let path = directory.join(&filename);
                    let bytes = serde_json::to_vec_pretty(&s.cloud_manifest())?;
                    if path.exists() {
                        let (file, sha) = storage::locked_sha256(&path, 32 * 1024 * 1024, control)?;
                        storage::finish_file(file, Ok(()), control)?;
                        if sha != storage::sha256(&bytes) {
                            return Err(m.pending_error("original upload manifest bytes changed; retain the receipt and original file"));
                        }
                    } else {
                        storage::atomic_write_controlled(&path, &bytes, control)?;
                    }
                    let mut file = local::open_source(&path)?;
                    let md5 = local::md5(&mut file, control);
                    let md5 = storage::finish_file(file, md5, control)?;
                    e["objects"].as_array_mut().unwrap().push(json!({"group":"manifest","kind":2,"file":path,"filename":filename,"size":bytes.len(),"sha256":storage::sha256(&bytes),"md5":md5,"content_type":"application/json","stage":"ready"}));
                    m.remember(e.clone())?;
                    e["objects"].as_array().unwrap().len() - 1
                };
                send(home, api, &account, &mut m, &mut e, index, control)?;
                e["body"] = body(s, &e)?;
                e["phase"] = json!("register_ready");
                m.remember(e.clone())?;
            }
            if e["phase"] == "register_ready" {
                e["before_ids"] = json!(
                    read::backups(api, Some(s.build))?
                        .iter()
                        .map(|v| backup_id(v).ok_or_else(|| malformed("backup list omitted an ID")))
                        .collect::<Result<Vec<_>>>()?
                );
                e["phase"] = json!("registering");
                m.remember(e.clone())?;
                let result = api.write(
                    Method::POST,
                    "system/user/backup/create",
                    &[],
                    Some(&e["body"]),
                    Some(s.build),
                );
                match result {
                    Ok(value) => {
                        let value = data(value);
                        let id = identity(
                            value
                                .get("id")
                                .or_else(|| value.get("backupId"))
                                .unwrap_or(&value),
                        )
                        .and_then(|s| s.parse::<u64>().ok())
                        .filter(|n| *n > 0);
                        if let Some(id) = id {
                            e["backup_id"] = json!(id);
                        }
                        e["phase"] = json!("readback");
                        m.remember(e.clone())?;
                    }
                    Err(error) => {
                        if error.details["submission"] == "not_sent" || definitive_rejection(&error)
                        {
                            e["phase"] = json!("register_ready");
                            m.remember(e.clone())?;
                        }
                        return Err(error);
                    }
                }
            }
            let listing = read::backups(api, Some(s.build))?;
            let id = if let Some(id) = e["backup_id"].as_u64() {
                id
            } else {
                let before = e["before_ids"]
                    .as_array()
                    .ok_or_else(|| malformed("original backup registration lost its prior list"))?;
                let mut found = Vec::new();
                for row in &listing {
                    let id =
                        backup_id(row).ok_or_else(|| malformed("backup list omitted an ID"))?;
                    if before.contains(&json!(id)) {
                        continue;
                    }
                    let detail = read::detail(api, "backup", &id.to_string(), Some(s.build))?;
                    if matches_body(&detail, &e["body"]) {
                        found.push(id);
                    }
                }
                if found.len() != 1 {
                    return Err(m.pending_error("original backup registration is not uniquely confirmed; repeat this exact manifest for read-only recovery, without another registration"));
                }
                found[0]
            };
            if !listing.iter().any(|v| backup_id(v) == Some(id)) {
                return Err(m.pending_error(
                    "registered backup is not yet visible in the original account list",
                ));
            }
            let detail = read::detail(api, "backup", &id.to_string(), Some(s.build))?;
            if !matches_body(&detail, &e["body"]) {
                return Err(m.pending_error("registered backup metadata differs from the original manifest and object receipts; no registration was repeated"));
            }
            e["backup_id"] = json!(id);
            e["phase"] = json!("readback");
            m.remember(e.clone())?;
            s.link(id, control)?;
            Ok(
                json!({"platform":"modus","local_backup_id":s.manifest["backupId"],"backup_id":id,"backup_name":s.manifest["backupName"],"server_type":s.build,"manifest_file":s.path,"uploaded":true,"verified":true,"verification_scope":"exact_backup_metadata_and_list_with_object_put_storage_receipts","objects":receipts(&e),"font_lookup":e["font_lookup"],"artifact_dir":directory,"operation_id":m.id(),"recovered":recovered}),
            )
        })();
        match result {
            Ok(result) => {
                e["result"] = result.clone();
                m.verified_with(e)?;
                Ok(result)
            }
            Err(error) => {
                let mut error=m.readback_error("backup upload retains original objects and completed steps; repeat the same manifest to continue or read back the uncertain step",error);
                error.details["stage"] = e["phase"].clone();
                error.details["objects"] = receipts(&e);
                error.details["manifest_file"] = json!(s.path);
                Err(error)
            }
        }
    }
}
fn missing_fonts(api: &Api, fonts: &[Value]) -> Result<Vec<String>> {
    if fonts.is_empty() {
        return Ok(vec![]);
    }
    let hashes = fonts
        .iter()
        .map(|f| f["md5"].as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    let value = data(api.post(
        "game/data/file/upload/check/font",
        &json!({"fontMd5":hashes}),
        None,
        false,
    )?);
    let values = value["notExist"]
        .as_array()
        .ok_or_else(|| malformed("font lookup omitted notExist"))?;
    let mut missing = BTreeSet::new();
    for v in values {
        let s = v
            .as_str()
            .filter(|s| hashes.contains(*s))
            .ok_or_else(|| malformed("font lookup returned an unknown hash"))?;
        if !missing.insert(s.to_owned()) {
            return Err(malformed("font lookup repeats a hash"));
        }
    }
    Ok(missing.into_iter().collect())
}
fn send(
    home: &Path,
    api: &Api,
    account: &str,
    m: &mut Mutation,
    e: &mut Value,
    i: usize,
    control: &OperationControl,
) -> Result<()> {
    let mut object = e["objects"][i].clone();
    if object["stage"] == "done" {
        return Ok(());
    }
    let persist = |m: &mut Mutation, e: &mut Value, o: &Value| {
        e["objects"][i] = o.clone();
        m.remember(e.clone())
    };
    if object["stage"] == "signing" {
        return Err(m.pending_error("the original object signature response is missing; no new object was allocated or uploaded"));
    }
    if object["stage"] == "putting" {
        let key = object["cosPath"]
            .as_str()
            .ok_or_else(|| malformed("original object lost its key"))?;
        let url = super::super::plugin::resource_url(key)?;
        if !api.http.verify_download(
            home,
            Provider::Modus,
            url,
            object["size"].as_u64().unwrap(),
            object["sha256"].as_str(),
            object["md5"].as_str(),
        )? {
            return Err(m.pending_error("original object bytes are not yet confirmed by the CDN; no PUT or storage registration was repeated"));
        }
        object["bytes_verified"] = json!(true);
        object["stage"] = json!("storage_ready");
        persist(m, e, &object)?;
    }
    if object["stage"] == "registering" {
        if object["kind"] == 3 && missing_fonts(api, &[json!({"md5":object["md5"]})])?.is_empty() {
            object["stage"] = json!("done");
            persist(m, e, &object)?;
            return Ok(());
        }
        return Err(m.pending_error("original storage registration has no acknowledgement or read endpoint; retain its object key and receipt for provider verification, without repeating registration"));
    }
    if object["stage"] == "ready" {
        let path = Path::new(object["file"].as_str().unwrap());
        let (mut file, sha) = storage::locked_sha256(path, u64::MAX, control)?;
        let result = (|| {
            if sha != object["sha256"].as_str().unwrap()
                || file.metadata()?.len() != object["size"].as_u64().unwrap()
            {
                return Err(invalid(
                    "$.manifest_file",
                    "original upload artifact bytes changed",
                ));
            }
            let filename = object["filename"].as_str().unwrap().to_owned();
            let sign = storage::sha256(
                format!("{account}{filename}modus_upload_sign_key_2026").as_bytes(),
            );
            object["stage"] = json!("signing");
            persist(m, e, &object)?;
            let signed = api.write(
                Method::POST,
                "game/data/file/getSignature",
                &[
                    ("fileName", filename.into()),
                    ("fileType", object["kind"].to_string()),
                    ("sign", sign),
                ],
                None,
                None,
            );
            let signed = match signed {
                Ok(v) => data(v),
                Err(error) => {
                    if error.details["submission"] == "not_sent" || definitive_rejection(&error) {
                        object["stage"] = json!("ready");
                        persist(m, e, &object)?;
                    }
                    return Err(error);
                }
            };
            let key = signed["key"]
                .as_str()
                .filter(|s| !s.is_empty() && !s.contains(['?', '#', ':', '\\']))
                .ok_or_else(|| malformed("backup signature omitted an ordinary object key"))?;
            super::super::plugin::resource_url(key)?;
            let url = Url::parse(
                signed["signedUrl"]
                    .as_str()
                    .ok_or_else(|| malformed("backup signature omitted signedUrl"))?,
            )
            .map_err(|_| malformed("invalid backup object upload URL"))?;
            object["cosPath"] = json!(key);
            object["stage"] = json!("putting");
            persist(m, e, &object)?;
            let mut headers = HeaderMap::new();
            headers.insert(
                CONTENT_TYPE,
                object["content_type"].as_str().unwrap().parse().unwrap(),
            );
            file.seek(SeekFrom::Start(0))?;
            if let Err(error) = api.http.put_file(url, headers, file.try_clone()?) {
                if error.details["submission"] == "not_sent" || definitive_rejection(&error) {
                    object["stage"] = json!("ready");
                    persist(m, e, &object)?;
                }
                return Err(error);
            }
            object["stage"] = json!("storage_ready");
            persist(m, e, &object)
        })();
        storage::finish_file(file, result, control)?;
    }
    if object["stage"] == "storage_ready" {
        let body = json!({"fileType":object["kind"],"cosStoreUrl":object["cosPath"],"originalFileName":object["filename"],"fileName":object["filename"],"fileSize":object["size"],"md5":object["md5"]});
        object["stage"] = json!("registering");
        persist(m, e, &object)?;
        if let Err(error) = api.write(
            Method::POST,
            "game/data/file/upload/data/storage",
            &[],
            Some(&body),
            None,
        ) {
            if error.details["submission"] == "not_sent" || definitive_rejection(&error) {
                object["stage"] = json!("storage_ready");
                persist(m, e, &object)?;
            }
            return Err(error);
        }
        object["stage"] = json!("done");
        persist(m, e, &object)?;
    }
    Ok(())
}
fn descriptor(e: &Value, key: &str) -> Value {
    e["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["group"] == key)
        .map(|o| json!({"cosPath":o["cosPath"],"md5":o["md5"],"size":o["size"],"layoutVersion":1}))
        .unwrap_or_else(|| json!({"cosPath":"","md5":"","size":0,"layoutVersion":1}))
}
fn body(s: &Snapshot, e: &Value) -> Result<Value> {
    let known = &s.sections["addons"];
    let csv = |field: &str, locked: bool| {
        known
            .iter()
            .map(|i| {
                if locked && i["meta"]["isLocked"] != true {
                    String::new()
                } else {
                    identity(&i["meta"][field]).unwrap_or_default()
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    let projects = csv("projectId", false);
    let known_info = json!({"list":known.iter().map(|i|i["name"].clone()).collect::<Vec<_>>(),"displayList":known.iter().map(|i|json!({"name":i["name"],"icon":i["meta"].get("icon").cloned().unwrap_or(json!("")),"author":i["meta"].get("author").cloned().unwrap_or(json!("")),"isLocked":i["meta"]["isLocked"]==true})).collect::<Vec<_>>(),"fileIds":csv("fileId",true),"projectIds":projects,"projectids":projects});
    let info = |key: &str| {
        let a = descriptor(e, key);
        json!({"list":s.sections[key].iter().map(|i|i["name"].clone()).collect::<Vec<_>>(),"zipPath":a["cosPath"],"md5":a["md5"],"size":a["size"],"layoutVersion":1}).to_string()
    };
    let accounts = s
        .groups
        .iter()
        .filter_map(|g| {
            g.account.as_ref().map(|account| {
                let mut value = descriptor(e, &g.key);
                value["name"] = account["name"].clone();
                value["roles"] = account["roles"].clone();
                value
            })
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"serverType":s.build,"backupName":s.manifest["backupName"],"knownAddons":known_info.to_string(),"knownAddonsNum":known.len(),"modifiedNum":s.sections["modifiedAddons"].len(),"modifiedCosInfo":info("modifiedAddons"),"unknownAddons":info("unknownAddons"),"unknownAddonsNum":s.sections["unknownAddons"].len(),"materialNum":s.sections["materials"].len(),"materialCosInfo":info("materials"),"wtfNum":accounts.len(),"wtfAccounts":serde_json::to_string(&accounts)?,"fonts":serde_json::to_string(&s.fonts)?,"fontNum":s.fonts.len(),"manifestPath":descriptor(e,"manifest")["cosPath"]}),
    )
}
fn matches_body(actual: &Value, expected: &Value) -> bool {
    let normalized = |v: &Value| {
        v.as_str()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .filter(|v| v.is_array() || v.is_object())
            .unwrap_or_else(|| v.clone())
    };
    expected.as_object().unwrap().iter().all(|(k, v)| {
        actual
            .get(k)
            .is_some_and(|a| normalized(a) == normalized(v))
    })
}
fn receipts(e: &Value) -> Value {
    json!(e["objects"].as_array().unwrap().iter().map(|o|json!({"group":o["group"],"file_type":o["kind"],"key":o.get("cosPath"),"size":o["size"],"sha256":o["sha256"],"md5":o["md5"],"stage":o["stage"],"bytes_verified":o["bytes_verified"]==true})).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests;
