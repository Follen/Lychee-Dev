use super::{
    protocol::{Api, Session, identifier},
    write::{self, Write},
    *,
};
use crate::{input::invalid, mutation::Mutation};
use std::{collections::BTreeSet, fs::File};

const MODULE: &[(&str, &str)] = &[
    ("id", "id"),
    ("name", "name"),
    ("logo_url", "logoUrl"),
    ("category_ids", "categoryIds"),
    ("type", "type"),
    ("desc", "desc"),
    ("official", "official"),
    ("official_url", "officialUrl"),
    ("core_folders", "coreFolders"),
];
pub(super) fn validate(resource: &str, action: &str, doc: &Value) -> Result<()> {
    for key in ["id", "module_id", "version_id"] {
        if doc
            .get(key)
            .is_some_and(|v| v.as_u64().is_none_or(|n| n == 0))
        {
            return Err(invalid(
                &format!("$.{key}"),
                "expected a positive integer from the plugin list/versions command",
            ));
        }
    }
    if action == "delete" {
        return Ok(());
    }
    if resource == "plugin" && action == "edit" {
        if let Some(value) = doc.get("category_ids") {
            comparable_list(value)?;
        }
        if let Some(value) = doc.get("core_folders") {
            for value in forms::strings(value, "$.core_folders", false)? {
                crate::local_backup::name(&value)?;
            }
        }
    } else {
        forms::text(&doc["name"], "$.name", true)?;
        let games = comparable_list(&doc["game_versions"])?;
        if games.is_empty() {
            return Err(invalid(
                "$.game_versions",
                "select gameVersions from publish blackbox plugin versions",
            ));
        }
        if let Some(value) = doc.get("file_url") {
            forms::url(value, "$.file_url")?;
        }
    }
    Ok(())
}
pub(super) fn schema(schema: &mut Value) {
    for field in ["category_ids", "game_versions"] {
        if schema["properties"].get(field).is_some() {
            schema["properties"][field]["items"] =
                json!({"type":["string","integer"],"minLength":1});
            schema["properties"][field]["uniqueItems"] = json!(true);
        }
    }
    if schema["properties"].get("core_folders").is_some() {
        schema["properties"]["core_folders"]["items"] = json!({"type":"string","minLength":1,"description":"One exact addon directory name, not a path."});
    }
    schema["description"] = json!(
        "Metadata edit preserves omitted fields; empty strings/arrays clear them. Plugin update creates a version using an original ZIP. Version edit preserves the current archive when file/file_url are omitted; file overrides file_url. game_versions are current branch option values from plugin versions. Archive acceptance/readback verifies metadata and canonical branch, not downloaded bytes. Unknown PUT uses original-URL byte readback; unknown version POST only rereads its original target."
    );
}
fn comparable_list(value: &Value) -> Result<Vec<String>> {
    let values = if let Some(rows) = value.as_array() {
        rows.clone()
    } else if let Some(s) = value.as_str() {
        s.split(',')
            .filter(|s| !s.is_empty())
            .map(|s| json!(s))
            .collect()
    } else if value.is_null() {
        Vec::new()
    } else {
        return Err(invalid(
            "$.selection",
            "expected an array of string/integer option values",
        ));
    };
    let mut set = BTreeSet::new();
    for value in values {
        let value = if value.is_object() {
            value
                .get("id")
                .or_else(|| value.get("value"))
                .cloned()
                .unwrap_or(Value::Null)
        } else {
            value
        };
        let s = identifier(&value)
            .filter(|s| !s.is_empty() && !s.contains(','))
            .ok_or_else(|| {
                invalid(
                    "$.selection",
                    "option value must be a nonempty string/integer without a comma",
                )
            })?;
        if !set.insert(s) {
            return Err(invalid("$.selection", "option values must be distinct"));
        }
    }
    Ok(set.into_iter().collect())
}
fn comparable(mut row: Value) -> Result<Value> {
    for key in ["categoryIds", "gameVersions", "coreFolders"] {
        if let Some(value) = row.get(key) {
            row[key] = json!(comparable_list(value)?);
        }
    }
    for key in ["id", "type", "moduleId"] {
        if let Some(value) = row.get(key) {
            row[key] = json!(
                identifier(value)
                    .and_then(|s| s.parse::<u64>().ok())
                    .ok_or_else(|| protocol_error("plugin metadata contains an invalid integer"))?
            );
        }
    }
    Ok(row)
}
fn file_open(path: &Path, control: &OperationControl) -> Result<(File, Value)> {
    let (mut file, sha256) = storage::locked_sha256(path, 5_000_000_000, control)?;
    let result = (|| {
        let bytes = file.metadata()?.len();
        let archive = zip::ZipArchive::new(&mut file)
            .map_err(|_| invalid("$.file", "file must be a valid ZIP archive"))?;
        if archive.is_empty() {
            return Err(invalid("$.file", "plugin ZIP must contain files"));
        }
        drop(archive);
        let md5 = cos::md5(&mut file, control)?;
        Ok(
            json!({"bytes":bytes,"sha256":sha256,"md5":md5,"filename":crate::local_backup::filename(path)?}),
        )
    })();
    match result {
        Ok(info) => Ok((file, info)),
        Err(error) => storage::finish_file(file, Err(error), control),
    }
}
pub(super) fn inspect_zip(path: &Path, control: &OperationControl) -> Result<Value> {
    let (file, info) = file_open(path, control)?;
    storage::finish_file(file, Ok(info), control)
}
pub(super) fn execute(
    request: &Write,
    home: &Path,
    api: &Api,
    session: &mut Session,
    control: &OperationControl,
) -> Result<Value> {
    if request.resource == "plugin" && request.action == "edit" {
        return module_edit(request, home, api, session, control);
    }
    if let Some(path) = request.doc["file"].as_str() {
        let (mut file, info) = file_open(&request.cwd.join(path), control)?;
        let result = version(
            request,
            home,
            api,
            session,
            Some((&mut file, info)),
            control,
        );
        storage::finish_file(file, result, control)
    } else {
        version(request, home, api, session, None, control)
    }
}
fn module_edit(
    request: &Write,
    home: &Path,
    api: &Api,
    session: &mut Session,
    control: &OperationControl,
) -> Result<Value> {
    let id = request.doc["id"].as_u64().unwrap();
    let mut mutation = write::open(
        home,
        session,
        "plugin",
        &id.to_string(),
        "edit",
        &request.doc,
        control,
    )?;
    let recovered = mutation.pending();
    if !recovered {
        let current = read::module(api, session, id)?;
        let mut form = json!({"name":"","logoUrl":"","id":id,"categoryIds":[],"type":1,"desc":"","official":"","officialUrl":"","coreFolders":""});
        for (input, wire) in MODULE {
            if let Some(value) = request.doc.get(*input).or_else(|| current.get(*wire)) {
                form[*wire] = value.clone();
            }
        }
        form["id"] = json!(id);
        form["coreFolders"] = json!(comparable_list(&form["coreFolders"])?.join(","));
        let expected = comparable(form.clone())?;
        mutation
            .prepare_with_evidence(json!({"expected":write::fingerprints(&expected)?}), control)?;
        api.post(session, "/wow/open_platform/module/update/", &[], &form)
            .map_err(|e| mutation.failure(e))?;
        mutation.acknowledged()?;
    }
    let actual = read::module(api, session, id)
        .and_then(comparable)
        .map_err(|e| mutation.readback_error("plugin edit requires original target readback", e))?;
    if !write::matches_fingerprints(&mutation.evidence().unwrap()["expected"], &actual)? {
        return Err(mutation.pending_error("plugin edit fields are not yet confirmed; repeat the exact input to read without resending"));
    }
    let result = json!({"platform":"blackbox","module_id":id,"verified":true,"verification_scope":"remote_metadata","operation_id":mutation.id(),"recovered":recovered});
    mutation.verified_with(json!({"result":result}))?;
    Ok(result)
}
fn archive(row: &Value) -> Option<&str> {
    ["fileUrlHeybox", "fileUrl", "file_url"]
        .into_iter()
        .find_map(|k| row[k].as_str().filter(|s| !s.is_empty()))
}
fn canonical(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("wow.max-c.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let path = url.path().strip_prefix("/wow/wow_module_file/")?;
    let (branch, file) = path.split_once('/')?;
    forms::branch(&json!(branch), "$.branch").ok()?;
    let hash = file.strip_suffix(".zip")?;
    (hash.len() == 32
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then(|| branch.to_owned())
}
fn intake(url: &str) -> bool {
    reqwest::Url::parse(url).ok().is_some_and(|u| {
        u.scheme() == "https"
            && u.host_str() == Some("wow.max-c.com")
            && u.port().is_none()
            && u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && u.path().strip_prefix("/wow/module/").is_some_and(|s| {
                !s.contains('/')
                    && s.to_ascii_lowercase().ends_with(".zip")
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_%.~-".contains(&b))
            })
    })
}
fn branch_for(options: &Value, games: &[String]) -> Option<String> {
    let rows = options.as_array()?;
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    for row in rows {
        let b = forms::branch(&row["strappid"], "$.strappid").ok()?;
        if !seen.insert(b.clone()) {
            return None;
        }
        let supported = forms::strings(&row["gameVersions"], "$.game_versions", true).ok()?;
        if games.iter().all(|g| supported.contains(g)) {
            found.push(b);
        }
    }
    if found.len() == 1 { found.pop() } else { None }
}
fn version(
    request: &Write,
    home: &Path,
    api: &Api,
    session: &mut Session,
    mut file: Option<(&mut File, Value)>,
    control: &OperationControl,
) -> Result<Value> {
    let doc = &request.doc;
    let module = doc["module_id"].as_u64().unwrap();
    let target = doc["version_id"].as_u64();
    let delete = request.action == "delete";
    let create = target.is_none();
    let identity = json!({"input":doc,"archive":file.as_ref().map(|(_,i)|i)});
    let scope = target
        .map(|id| format!("{module}:{id}"))
        .unwrap_or_else(|| "new".into());
    let mut mutation = write::open(
        home,
        session,
        "plugin-version",
        &scope,
        &request.action,
        &identity,
        control,
    )?;
    if create || delete {
        if let Some(result) = write::completed(&mutation) {
            return Ok(result);
        }
    }
    let recovered = mutation.pending();
    if !recovered {
        let (rows, mut options) = read::versions(api, session, module)?;
        let current = target
            .map(|id| {
                rows.iter()
                    .find(|r| identifier(&r["id"]) == Some(id.to_string()))
                    .ok_or_else(|| write::not_found("version is absent from the selected module"))
            })
            .transpose()?;
        if current.is_some_and(|r| {
            r.get("moduleId")
                .is_some_and(|v| identifier(v) != Some(module.to_string()))
        }) {
            return Err(protocol_error("version identifies another module"));
        }
        if delete {
            mutation.prepare_with_evidence(
                json!({"stage":"submitted","delete":true,"target":target}),
                control,
            )?;
            api.post(
                session,
                "/wow/open_platform/module_version/delete/",
                &[],
                &json!({"moduleId":module,"versionId":target}),
            )
            .map_err(|e| mutation.failure(e))?;
            mutation.acknowledged()?;
        } else {
            let games = comparable_list(&doc["game_versions"])?;
            if options.is_null() {
                options = read::execute(api, session, Query::GameTypes)?["items"].clone();
            }
            let branch = branch_for(&options, &games);
            if branch.is_none() {
                return Err(invalid(
                    "$.game_versions",
                    "select gameVersions from one unambiguous current module branch",
                ));
            }
            let source = current.and_then(archive);
            let source_branch = source.and_then(canonical);
            if file.is_some()
                && target.is_some()
                && (source_branch.is_none()
                    || source_branch
                        != branch_for(
                            &options,
                            &comparable_list(&current.unwrap()["gameVersions"])?,
                        ))
            {
                return Err(invalid(
                    "$.file",
                    "replacement requires an existing official canonical archive in its confirmed original branch",
                ));
            }
            if file.is_some() && target.is_some() && source_branch != branch {
                return Err(invalid(
                    "$.game_versions",
                    "replacement must retain the original archive branch",
                ));
            }
            let input_url = doc["file_url"].as_str().or(source);
            if file.is_none() && input_url.is_none() {
                return Err(invalid(
                    "$.file_url",
                    "version has no current archive; supply file or file_url",
                ));
            }
            let expected = json!({"name":doc["name"],"type":doc["type"],"gameVersions":games});
            let evidence = json!({"stage":if file.is_some(){"allocate"}else{"submitted"},"delete":false,"create":create,"target":target,"previous":write::ids(&rows,"id")?,"expected":write::fingerprints(&expected)?,"branch":branch,"source_sha256":source.map(|s|storage::sha256(s.as_bytes())),"source_branch":source_branch,"existing_source":file.is_none()&&doc.get("file_url").is_none(),"local_upload":file.is_some(),"replacement":file.is_some()&&target.is_some(),"archive_sha256":input_url.map(|s|storage::sha256(s.as_bytes()))});
            mutation.prepare_with_evidence(evidence, control)?;
            if file.is_none() {
                submit_version(request, api, session, &mut mutation, input_url.unwrap())?;
            }
        }
    }
    let mut evidence = mutation
        .evidence()
        .cloned()
        .ok_or_else(|| mutation.pending_error("version write evidence is missing"))?;
    if evidence["stage"] == "allocate" {
        let (original, info) = file
            .as_mut()
            .ok_or_else(|| mutation.pending_error("retain the original ZIP for upload recovery"))?;
        // Capability allocation is a read: no object or version has been submitted.
        let response=api.request(session,"/wow/cos/upload/token/",&[],Some(&json!({"jsonData":json!({"upload_infos":[{"filename":info["filename"],"file_size":info["bytes"].as_u64().unwrap() as f64/1048576.0,"type":"module"}]}).to_string()})),false,None)?;
        let object = cos::Object::plugin(&protocol::checked_result(&response)?)?;
        evidence["stage"] = json!("uploading");
        evidence["upload_url"] = json!(object.public_url);
        evidence["archive_sha256"] = json!(storage::sha256(object.public_url.as_bytes()));
        mutation.remember(evidence.clone())?;
        object
            .put(&api.http, original, info["md5"].as_str().unwrap(), false)
            .map_err(|e| mutation.failure(e))?;
        evidence["stage"] = json!("uploaded");
        mutation.remember(evidence.clone())?;
    } else if evidence["stage"] == "uploading" {
        let (_, info) = file.as_ref().ok_or_else(|| {
            mutation.pending_error("retain the original ZIP for object verification")
        })?;
        let url = forms::url(&evidence["upload_url"], "$.original_archive")?;
        if !intake(url.as_str()) {
            return Err(protocol_error(
                "original object URL is outside the Workshop intake",
            ));
        }
        let valid = api
            .http
            .verify_download(
                home,
                crate::provider::Provider::Blackbox,
                url,
                info["bytes"].as_u64().unwrap(),
                info["sha256"].as_str(),
                None,
            )
            .map_err(|e| {
                mutation.readback_error(
                    "original object PUT requires byte readback, not another upload",
                    e,
                )
            })?;
        if !valid {
            return Err(mutation.pending_error(
                "original object does not yet match the selected ZIP; no upload was replayed",
            ));
        }
        evidence["stage"] = json!("uploaded");
        evidence["original_object_bytes_verified"] = json!(true);
        mutation.remember(evidence.clone())?;
    }
    if evidence["stage"] == "uploaded" {
        submit_version(
            request,
            api,
            session,
            &mut mutation,
            evidence["upload_url"].as_str().unwrap(),
        )?;
    }
    let evidence = mutation.evidence().unwrap().clone();
    let (rows, options) = read::versions(api, session, module).map_err(|e| {
        mutation.readback_error("version write needs complete original module readback", e)
    })?;
    if delete {
        let found = rows
            .iter()
            .find(|r| identifier(&r["id"]) == target.map(|v| v.to_string()));
        if found.is_some_and(|r| r["auditState"] != 4) {
            return Err(mutation.pending_error(
                "version delete is not confirmed; repeat the same input to read without resending",
            ));
        }
        let result = json!({"platform":"blackbox","module_id":module,"version_id":target,"verified":true,"audit_state":found.map(|r|&r["auditState"]),"operation_id":mutation.id(),"recovered":recovered});
        mutation.verified_with(json!({"result":result}))?;
        return Ok(result);
    }
    let previous = evidence["previous"].as_array().unwrap();
    let mut candidates = Vec::new();
    let selected = evidence["response_id"].as_u64().or(target);
    for row in rows {
        let id = identifier(&row["id"]).unwrap();
        if selected.is_some_and(|n| id != n.to_string())
            || (create && previous.contains(&json!(id)))
        {
            continue;
        }
        if row
            .get("moduleId")
            .is_some_and(|v| identifier(v) != Some(module.to_string()))
        {
            continue;
        }
        let actual = comparable(row.clone())?;
        if !write::matches_fingerprints(&evidence["expected"], &actual)? {
            continue;
        }
        if let Some(mode) = archive_mode(&evidence, &row, &options)? {
            candidates.push((id, row, mode));
        }
    }
    if candidates.len() != 1 {
        return Err(mutation.pending_error(if candidates.len()>1{"new version readback is ambiguous; no version was resubmitted"}else{"version metadata/archive binding needs readback; repeat the original input without reuploading"}));
    }
    let (id, row, mode) = candidates.remove(0);
    let result = json!({"platform":"blackbox","module_id":module,"version_id":id,"verified":true,"metadata_verified":true,"verification_scope":"remote_metadata","archive_verification":{"mode":mode,"content_verified":false,"public_download_verified":false},"unverified_fields":["archive_bytes"],"upload":file.as_ref().map(|(_,i)|i),"operation_id":mutation.id(),"recovered":recovered,"readback":crate::output::public(row,&[])});
    mutation.verified_with(json!({"result":result}))?;
    Ok(result)
}
fn submit_version(
    request: &Write,
    api: &Api,
    session: &mut Session,
    mutation: &mut Mutation,
    url: &str,
) -> Result<()> {
    let doc = &request.doc;
    let mut body = json!({"moduleId":doc["module_id"],"name":doc["name"],"type":doc["type"],"gameVersions":comparable_list(&doc["game_versions"])?.join(","),"fileUrl":url});
    if let Some(id) = doc.get("version_id") {
        body["versionId"] = id.clone();
    }
    let mut evidence = mutation.evidence().unwrap().clone();
    evidence["stage"] = json!("submitted");
    mutation.remember(evidence.clone())?;
    let response = api
        .post(
            session,
            "/wow/open_platform/module_version/upsert/",
            &[],
            &body,
        )
        .map_err(|e| mutation.failure(e))?;
    mutation.acknowledged()?;
    if let Some(id) = response
        .get("versionId")
        .or_else(|| response.get("id"))
        .and_then(identifier)
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
    {
        evidence["response_id"] = json!(id);
        mutation.remember(evidence)?;
    }
    Ok(())
}
fn archive_mode(evidence: &Value, row: &Value, options: &Value) -> Result<Option<&'static str>> {
    let Some(actual) = archive(row) else {
        return Ok(None);
    };
    let hash = storage::sha256(actual.as_bytes());
    let exact = evidence["archive_sha256"] == hash;
    if evidence["replacement"] == true && evidence["source_sha256"] == hash {
        return Ok(None);
    }
    if exact {
        return Ok(Some("url_exact"));
    }
    if evidence["local_upload"] != true && evidence["existing_source"] != true {
        return Ok(None);
    }
    let games = comparable_list(&row["gameVersions"])?;
    let branch = branch_for(options, &games);
    if branch.is_none() || json!(branch) != evidence["branch"] || canonical(actual) != branch {
        return Ok(None);
    }
    if evidence["local_upload"] == true {
        if !evidence["upload_url"].as_str().is_some_and(intake) {
            return Ok(None);
        }
        return Ok(Some(if evidence["replacement"] == true {
            "server_replacement_canonicalization"
        } else {
            "server_canonicalization"
        }));
    }
    if evidence["source_branch"] == evidence["branch"]
        && evidence["target"]
            .as_u64()
            .is_some_and(|id| identifier(&row["id"]) == Some(id.to_string()))
    {
        Ok(Some("server_recanonicalization"))
    } else {
        Ok(None)
    }
}
