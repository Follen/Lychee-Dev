use super::{
    protocol::{Api, data, identity},
    *,
};
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::{Mutation, completed, digest, fingerprints, matches_fingerprints},
    provider::Provider,
};
use lycheedev_core::storage;
use reqwest::{
    Method, Url,
    header::{CONTENT_TYPE, HeaderMap},
};
use std::{
    fs::File,
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub struct PluginWrite {
    action: String,
    doc: Value,
    cwd: PathBuf,
}
impl PluginWrite {
    pub fn parse(action: &str, bytes: &[u8], cwd: &Path) -> Result<Self> {
        if !["create", "upload", "update", "edit", "delete"].contains(&action) {
            return Err(ToolkitError::arguments("unknown ModUs plugin operation"));
        }
        let doc = document(bytes)?;
        input_shape(Provider::Modus, "plugin", action)?.admit(&doc)?;
        let doc = Value::Object(doc);
        for name in ["version", "type", "path", "file", "toc_version"] {
            if doc
                .get(name)
                .is_some_and(|v| v.as_str().is_some_and(|s| s.trim().is_empty()))
            {
                return Err(invalid(&format!("$.{name}"), "value must not be blank"));
            }
        }
        if let Some(v) = doc.get("supported_game_versions") {
            package::games(v)?;
        }
        if let Some(v) = doc.get("toc_version") {
            package::derive(v.as_str().unwrap())?;
        }
        if doc.get("md5").is_some_and(|v| {
            !v.as_str()
                .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        }) {
            return Err(invalid(
                "$.md5",
                "expected 32 hexadecimal digits matching the ZIP",
            ));
        }
        if ["update", "edit"].contains(&action) && doc.as_object().unwrap().len() == 3 {
            return Err(invalid("$", "supply at least one release field or ZIP"));
        }
        Ok(Self {
            action: action.into(),
            doc,
            cwd: cwd.into(),
        })
    }
    pub fn schema(action: &str) -> Value {
        let mut s = input_shape(Provider::Modus, "plugin", action)
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        if action != "delete" {
            s["properties"]["supported_game_versions"] = json!({"type":"array","minItems":1,"uniqueItems":true,"items":{"type":"object","additionalProperties":false,"required":["game_version","server"],"properties":{"game_version":{"type":"string","minLength":1},"server":{"type":"string","minLength":1}}},"description":"Current choices from options game-versions --keys '[\"wow_builds\"]'. Must belong to ZIP client branches. Omission with a ZIP chooses only exact TOC versions still offered by the service."});
        }
        s
    }
    fn package(&self, control: &OperationControl) -> Result<Option<(File, Value)>> {
        self.doc["file"]
            .as_str()
            .map(|p| package::open(&self.cwd.join(p), control))
            .transpose()
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let package = self.package(control)?;
        let result = (|| {
            if let Some((_, info)) = &package {
                self.check_package(info)?;
            }
            Ok(
                json!({"platform":"modus","resource":"plugin","action":self.action,"planned":true,"local_write":false,"credential_reads":false,"remote_read":false,"remote_write":false,"project_id":self.doc["project_id"],"file_id":self.doc.get("file_id"),"archive":package.as_ref().map(|(_,v)|v),"fields":self.doc.as_object().unwrap().keys().filter(|k|k.as_str()!="schema").collect::<Vec<_>>(),"next":"Execution checks the Creator project and current wow_builds choices. New releases reserve one file ID; updates retain the original ID. Repeat the exact input for pending readback without replaying unknown writes."}),
            )
        })();
        if let Some((file, _)) = package {
            storage::finish_file(file, result, control)
        } else {
            result
        }
    }
    fn check_package(&self, info: &Value) -> Result<()> {
        for key in ["md5", "zip_size", "unzip_size", "toc_version"] {
            if let Some(value) = self.doc.get(key) {
                let same = if key == "md5" {
                    value
                        .as_str()
                        .unwrap()
                        .eq_ignore_ascii_case(info[key].as_str().unwrap())
                } else {
                    value == &info[key]
                };
                if !same {
                    return Err(invalid(
                        &format!("$.{key}"),
                        "field differs from the selected ZIP",
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn execute(
        &self,
        home: &Path,
        recover: Option<u64>,
        control: &OperationControl,
    ) -> Result<Value> {
        let mut package = self.package(control)?;
        let result = (|| {
            if let Some((_, info)) = &package {
                self.check_package(info)?;
            }
            let api = Api {
                http: crate::transport::Http::new(control)?,
                session: Some(auth::Session::load(Client::Creator, control)?),
            };
            self.run(
                home,
                &api,
                package.as_mut().map(|(f, i)| (f, &*i)),
                recover,
                control,
            )
        })();
        if let Some((file, _)) = package {
            storage::finish_file(file, result, control)
        } else {
            result
        }
    }
    fn run(
        &self,
        home: &Path,
        api: &Api,
        mut package: Option<(&mut File, &Value)>,
        recover: Option<u64>,
        control: &OperationControl,
    ) -> Result<Value> {
        let project = self.doc["project_id"].as_u64().unwrap();
        let create = ["create", "upload"].contains(&self.action.as_str());
        let delete = self.action == "delete";
        let account = api.account()?;
        let info = package.as_ref().map(|(_, i)| *i);
        if let Some(info) = info {
            self.check_package(info)?;
        }
        let scope = digest(
            &json!({"account":account,"project":project,"file":self.doc.get("file_id"),"create":if create{Some(&self.doc)}else{None}}),
        )?;
        let mut m = Mutation::open(
            home,
            Provider::Modus,
            "plugin",
            u64::from_str_radix(&scope[..16], 16).unwrap().max(1),
            &self.action,
            &json!({"account":account,"input":self.doc,"archive":info}),
            control,
        )?;
        if create || delete {
            if let Some(done) = completed(&m) {
                return Ok(done);
            }
        }
        let recovered = m.pending();
        if !recovered {
            if recover.is_some() {
                return Err(ToolkitError::arguments(
                    "--recover-file-id applies only to an existing pending allocation",
                ));
            }
            if !project::owned(api)?.contains(&project.to_string()) {
                return Err(ToolkitError::new(
                    "publish.modus_not_owned",
                    "project is absent from the current Creator author list",
                    5,
                ));
            }
            let current = if !create {
                Some(release(
                    api,
                    project,
                    self.doc["file_id"].as_u64().unwrap(),
                )?)
            } else {
                None
            };
            if create && self.doc.get("file_id").is_some() {
                let id = self.doc["file_id"].as_u64().unwrap();
                if ids(api, project)?.contains(&id) {
                    return Err(invalid(
                        "$.file_id",
                        "create requires a reserved unused file ID; use plugin update for an existing version",
                    ));
                }
            }
            let mut wire = wire(&self.doc, !create)?;
            if let Some(info) = info {
                for (local, key) in [
                    ("md5", "md5"),
                    ("zip_size", "zipSize"),
                    ("unzip_size", "unzipSize"),
                    ("toc_version", "tocVersion"),
                ] {
                    wire[key] = info[local].clone();
                }
            }
            if !delete && (info.is_some() || self.doc.get("supported_game_versions").is_some()) {
                let derived = if let Some(info) = info {
                    info["derived_games"].clone()
                } else {
                    package::derive(
                        current.as_ref().unwrap()["tocVersion"]
                            .as_str()
                            .ok_or_else(|| malformed("current release omitted tocVersion"))?,
                    )?
                };
                let choices = api.post(
                    "game/data/config/detail",
                    &json!({"keys":["wow_builds"]}),
                    None,
                    false,
                )?;
                wire["supportedGameVersionsReqs"] =
                    package::select(&derived, &choices, self.doc.get("supported_game_versions"))?;
            }
            let file = self.doc["file_id"].as_u64();
            let expected = normalize(&wire)?;
            m.prepare_with_evidence(json!({"stage":if create&&file.is_none(){"allocating"}else{"allocated"},"file_id":file,"wire":wire,"expected":fingerprints(&expected)?,"archive":info,"verify_bytes":info.is_some()||self.doc.get("path").is_some()||self.doc.get("md5").is_some()||self.doc.get("zip_size").is_some()}),control)?;
            if create && file.is_none() {
                let value = data(
                    api.write(
                        Method::GET,
                        &format!("game/data/author/project/fileId/{project}"),
                        &[],
                        None,
                        None,
                    )
                    .map_err(|e| m.failure(e))?,
                );
                let file=identity(&value).or_else(||identity(&value["fileId"])).or_else(||identity(&value["id"])).and_then(|id|id.parse::<u64>().ok()).ok_or_else(||m.pending_error("file-ID allocation returned no usable ID; query plugin list/get, then use --recover-file-id only for a matching recorded release; allocation is not repeated"))?;
                let mut e = m.evidence().unwrap().clone();
                e["file_id"] = json!(file);
                e["stage"] = json!("allocated");
                m.remember(e)?;
            }
        }
        let result=(||{
            if m.evidence().unwrap()["stage"]=="allocating"{
                let id=recover.ok_or_else(||m.pending_error("original file-ID allocation is uncertain; use plugin list/get to find a matching release and supply --recover-file-id; no allocation was repeated"))?;
                let actual=release(api,project,id)?;
                if !matches_fingerprints(&m.evidence().unwrap()["expected"],&normalize(&actual)?)?{return Err(m.pending_error("selected recovery file does not match the original release metadata"));}
                let mut e=m.evidence().unwrap().clone();e["file_id"]=json!(id);e["stage"]=json!("readback");m.remember(e)?;
            }else if recover.is_some_and(|id|Some(id)!=m.evidence().unwrap()["file_id"].as_u64()){return Err(ToolkitError::arguments("--recover-file-id differs from the original reserved file ID"));}
            let file=m.evidence().unwrap()["file_id"].as_u64().ok_or_else(||malformed("original release lost its file ID"))?;
            if m.evidence().unwrap()["stage"]=="allocated"{
                let mut e=m.evidence().unwrap().clone();let mut body=e["wire"].clone();
                if delete{body=json!({"projectId":project,"fileId":file});}
                else if !create{body["fileId"]=json!(file);}
                if !delete&&info.is_some()&&body.get("path").is_none(){body["path"]=json!(format!("modus/wow/addons/{project}/files/{file}.zip"));}
                e["expected"]=fingerprints(&normalize(&body)?)?;e["stage"]=json!(if delete{"deleting"}else{"metadata"});m.remember(e)?;
                api.write(Method::POST,if delete{"game/data/author/project/delete"}else if create{"game/data/author/project/upload"}else{"game/data/author/project/file/update"},&[],Some(&body),None).map_err(|e|stage_failure(&mut m,"allocated","original release request needs readback; it was not repeated",e))?;
                m.acknowledged()?;
                let mut e=m.evidence().unwrap().clone();e["stage"]=json!(if delete{"deleting"}else{"metadata_done"});m.remember(e)?;
            }
            if delete{
                if ids(api,project)?.contains(&file){return Err(m.pending_error("original release deletion is still present; no deletion was repeated"));}
                return Ok(json!({"platform":"modus","project_id":project,"file_id":file,"deleted":true,"verified":true,"verification_scope":"complete_project_release_list_absence","operation_id":m.id(),"recovered":recovered}));
            }
            if m.evidence().unwrap()["stage"]=="metadata"{
                let actual=release(api,project,file)?;
                if !matches_fingerprints(&m.evidence().unwrap()["expected"],&normalize(&actual)?)?{return Err(m.pending_error("original release metadata is not confirmed; no metadata was resubmitted"));}
                let mut e=m.evidence().unwrap().clone();e["stage"]=json!("metadata_done");m.remember(e)?;
            }
            if m.evidence().unwrap()["stage"]=="metadata_done"{
                if let Some((archive,_))=package.as_mut(){
                    let value=data(api.get(&format!("game/data/author/project/file/upload/signature/{project}/{file}"),&[],None)?);
                    let url=value.as_str().or_else(||value["signedUrl"].as_str()).and_then(|s|Url::parse(s).ok()).ok_or_else(||malformed("release signature omitted its upload URL"))?;
                    let mut e=m.evidence().unwrap().clone();e["stage"]=json!("binary");m.remember(e)?;
                    archive.seek(SeekFrom::Start(0))?;let mut headers=HeaderMap::new();headers.insert(CONTENT_TYPE,"application/zip".parse().unwrap());
                    api.http.put_file(url,headers,archive.try_clone()?).map_err(|e|stage_failure(&mut m,"metadata_done","original ZIP PUT needs byte readback; no object was resent",e))?;
                }
                let mut e=m.evidence().unwrap().clone();e["stage"]=json!("readback");m.remember(e)?;
            }
            let actual=release(api,project,file)?;
            let evidence=m.evidence().unwrap();
            if !matches_fingerprints(&evidence["expected"],&normalize(&actual)?)?{return Err(m.pending_error("saved release fields differ from the original request; read back without repeating writes"));}
            let bytes_verified=evidence["verify_bytes"]==true;
            if bytes_verified{
                let url=resource_url(actual["path"].as_str().ok_or_else(||malformed("saved release omitted its package path"))?)?;
                let bytes=actual["zipSize"].as_u64().filter(|n|*n<=package::MAX_ZIP).ok_or_else(||malformed("saved release omitted its package size"))?;
                let md5=actual["md5"].as_str().filter(|s|s.len()==32&&s.bytes().all(|b|b.is_ascii_hexdigit())).ok_or_else(||malformed("saved release omitted its MD5"))?.to_lowercase();
                if !api.http.verify_download(home, Provider::Modus,url,bytes,evidence["archive"]["sha256"].as_str(),Some(&md5))?{return Err(m.pending_error("CDN package bytes differ from the original ZIP; metadata and archive remain separate, and no PUT was repeated"));}
            }
            Ok(json!({"platform":"modus","project_id":project,"file_id":file,"verified":true,"archive_bytes_verified":bytes_verified,"verification_scope":if bytes_verified{"remote_metadata_and_archive_bytes"}else{"remote_metadata"},"operation_id":m.id(),"recovered":recovered,"readback":crate::output::public(actual,&[])}))
        })().map_err(|e|m.readback_error("ModUs release needs original-target verification; no uncertain write was replayed",e))?;
        m.verified_with(json!({"result":result}))?;
        Ok(result)
    }
}
fn wire(doc: &Value, update: bool) -> Result<Value> {
    let mut out = json!({});
    for (local, key) in [
        ("project_id", "projectId"),
        ("file_id", "fileId"),
        ("version", "version"),
        ("type", "type"),
        ("md5", "md5"),
        ("zip_size", "zipSize"),
        ("unzip_size", "unzipSize"),
        ("path", "path"),
        ("toc_version", "tocVersion"),
        ("changelog", "changelog"),
    ] {
        if let Some(v) = doc.get(local) {
            out[key] = if update
                && key == "changelog"
                && (v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty()))
            {
                json!("<null>")
            } else {
                v.clone()
            };
        }
    }
    if !update {
        out.as_object_mut().unwrap().remove("fileId");
    }
    if let Some(g) = doc.get("supported_game_versions") {
        out["supportedGameVersionsReqs"] = package::games(g)?;
    }
    Ok(out)
}
fn normalize(value: &Value) -> Result<Value> {
    let mut out = value.clone();
    if !out.is_object() {
        return Err(malformed("release metadata must be an object"));
    }
    if let Some(v) = out.get("supportedGameVersions").cloned() {
        out["supportedGameVersionsReqs"] = v;
    }
    if let Some(rows) = out["supportedGameVersionsReqs"].as_array() {
        let mut rows = rows
            .iter()
            .map(|r| json!({"gameVersion":r["gameVersion"],"server":r["server"]}))
            .collect::<Vec<_>>();
        rows.sort_by_key(Value::to_string);
        out["supportedGameVersionsReqs"] = json!(rows);
    }
    for key in ["projectId", "fileId", "zipSize", "unzipSize"] {
        if let Some(n) = out[key].as_str().and_then(|s| s.parse::<u64>().ok()) {
            out[key] = json!(n);
        }
    }
    if let Some(md5) = out["md5"].as_str() {
        out["md5"] = json!(md5.to_lowercase());
    }
    if out["changelog"] == "<null>"
        || out["changelog"]
            .as_str()
            .is_some_and(|s| s.trim().is_empty())
    {
        out["changelog"] = Value::Null;
    }
    Ok(out)
}
fn release(api: &Api, project: u64, file: u64) -> Result<Value> {
    let mut value = read::execute(api, Query::Release { project, file })?["data"].clone();
    // Query::Release already checked these IDs, including complete parent-list
    // membership when the native detail omitted its project ID.
    value["projectId"] = json!(project);
    value["fileId"] = json!(file);
    Ok(value)
}
fn ids(api: &Api, project: u64) -> Result<Vec<u64>> {
    read::pages(api, Listing::Releases(project), None, None, 100, &json!({}))?["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            identity(
                r.get("fileId")
                    .or_else(|| r.get("id"))
                    .unwrap_or(&Value::Null),
            )
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| malformed("release list omitted file ID"))
        })
        .collect()
}
pub(super) fn resource_url(path: &str) -> Result<Url> {
    if path.trim() != path || path.chars().any(char::is_control) {
        return Err(malformed("package path needs manual origin verification"));
    }
    let text = if Url::parse(path).is_ok() {
        path.into()
    } else {
        format!("https://cdn.modus.cool/{}", path.trim_start_matches('/'))
    };
    let url = Url::parse(&text).map_err(|_| malformed("package path is invalid"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("cdn.modus.cool")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(malformed(
            "custom package origin needs manual verification; the saved path is preserved",
        ));
    }
    Ok(url)
}

#[cfg(test)]
mod tests;

fn stage_failure(
    m: &mut Mutation,
    not_sent_stage: &str,
    message: &str,
    error: ToolkitError,
) -> ToolkitError {
    if error.details["submission"] == "not_sent" || crate::mutation::definitive_rejection(&error) {
        let mut evidence = m.evidence().unwrap().clone();
        evidence["stage"] = json!(not_sent_stage);
        if let Err(save) = m.remember(evidence) {
            return m.readback_error(
                "recording the confirmed rejection failed; retain the original operation",
                save,
            );
        }
    }
    m.readback_error(message, error)
}
