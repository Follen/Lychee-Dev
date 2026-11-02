use super::{
    protocol::{Api, data, identity},
    *,
};
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::{Mutation, completed, digest},
    provider::Provider,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use reqwest::Method;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
mod local;
mod manifest;
mod upload;
pub use local::BackupLocal;
pub use upload::BackupUpload;

pub struct BackupCloud {
    action: String,
    doc: Value,
}
impl BackupCloud {
    pub fn parse(action: &str, bytes: &[u8]) -> Result<Self> {
        if !["rename", "delete", "delete-preview"].contains(&action) {
            return Err(ToolkitError::arguments("unknown ModUs cloud backup action"));
        }
        let doc = document(bytes)?;
        input_shape(Provider::Modus, "backup", action)?.admit(&doc)?;
        let doc = Value::Object(doc);
        if let Some(ids) = doc.get("backup_ids") {
            let ids = ids.as_array().unwrap();
            let mut seen = BTreeSet::new();
            if ids.is_empty()
                || ids.len() > 1000
                || ids
                    .iter()
                    .any(|id| !id.as_u64().is_some_and(|n| n > 0 && seen.insert(n)))
            {
                return Err(invalid(
                    "$.backup_ids",
                    "select 1..1000 unique positive integer backup IDs",
                ));
            }
        }
        if action == "delete-preview"
            && [
                "backup_ids",
                "name_contains",
                "name_equals",
                "created_before",
                "created_after",
            ]
            .iter()
            .all(|k| doc.get(*k).is_none())
        {
            return Err(invalid(
                "$",
                "select explicit backup_ids or a name/date filter",
            ));
        }
        for key in ["backup_name", "name_contains", "name_equals"] {
            if doc
                .get(key)
                .is_some_and(|v| v.as_str().unwrap().trim().is_empty())
            {
                return Err(invalid(&format!("$.{key}"), "value must not be blank"));
            }
        }
        for key in ["created_before", "created_after"] {
            if let Some(v) = doc.get(key) {
                date(v.as_str().unwrap()).ok_or_else(|| {
                    invalid(
                        &format!("$.{key}"),
                        "use an ISO 8601 date/time; a missing timezone means UTC",
                    )
                })?;
            }
        }
        Ok(Self {
            action: action.into(),
            doc,
        })
    }
    pub fn schema(action: &str) -> Value {
        let mut s = input_shape(Provider::Modus, "backup", action)
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        if action != "rename" {
            s["properties"]["backup_ids"] = json!({"type":"array","minItems":1,"maxItems":1000,"uniqueItems":true,"items":{"type":"integer","minimum":1},"description":"Exact account-global backup IDs from backup list; each selected backup must belong to server_type."});
        }
        if action == "delete-preview" {
            s["anyOf"] = json!([{"required":["backup_ids"]},{"required":["name_contains"]},{"required":["name_equals"]},{"required":["created_before"]},{"required":["created_after"]}]);
        }
        s
    }
    pub fn plan(&self) -> Value {
        json!({"platform":"modus","resource":"backup","action":self.action,"planned":true,"local_write":false,"credential_reads":false,"remote_read":false,"remote_write":false,"selection":self.doc,"next":"Execution verifies the actual backup build. Delete preview/commit checks complete configuration references across all five ModUs builds; unknown coverage blocks deletion. Pending batches retain original IDs and completed items."})
    }
    pub fn execute(&self, home: &Path, control: &OperationControl) -> Result<Value> {
        let api = Api {
            http: crate::transport::Http::new(control)?,
            session: Some(auth::Session::load(Client::Main, control)?),
        };
        self.run(home, &api, control)
    }
    fn run(&self, home: &Path, api: &Api, control: &OperationControl) -> Result<Value> {
        if self.action == "delete-preview" {
            return self.preview(api);
        }
        let account = api.account()?;
        let scope = digest(
            &json!({"account":account,"action":self.action,"backup":self.doc.get("backup_id"),"selected_build":self.doc.get("server_type")}),
        )?;
        let mut m = Mutation::open(
            home,
            Provider::Modus,
            "backup-cloud",
            u64::from_str_radix(&scope[..16], 16).unwrap().max(1),
            &self.action,
            &json!({"account":account,"input":self.doc}),
            control,
        )?;
        if self.action == "delete" {
            if let Some(done) = completed(&m) {
                return Ok(done);
            }
        }
        let recovered = m.pending();
        let build = if recovered {
            m.evidence()
                .and_then(|v| v["build"].as_u64())
                .filter(|n| *n <= 4)
                .ok_or_else(|| malformed("original backup request lost its build"))?
                as u8
        } else {
            api.build(self.doc["server_type"].as_u64().map(|n| n as u8))?
        };
        if self.action == "rename" {
            let id = self.doc["backup_id"].as_u64().unwrap();
            if !recovered {
                if !read::backups(api, Some(build))?
                    .iter()
                    .any(|r| backup_id(r) == Some(id))
                {
                    return Err(invalid(
                        "$.backup_id",
                        "backup is absent from this account; inspect backup list",
                    ));
                }
                let current = read::detail(api, "backup", &id.to_string(), Some(build))?;
                require_build(&current, build)?;
                if current.get("backupName").or_else(|| current.get("name"))
                    == self.doc.get("backup_name")
                {
                    return Ok(
                        json!({"platform":"modus","backup_id":id,"backup_name":self.doc["backup_name"],"server_type":build,"renamed":true,"verified":true,"reused":true,"verification_scope":"exact_backup_name","remote_write":false}),
                    );
                }
                m.prepare_with_evidence(json!({"build":build}), control)?;
                api.write(
                    Method::POST,
                    "system/user/backup/update",
                    &[],
                    Some(&json!({"id":id,"backupName":self.doc["backup_name"]})),
                    Some(build),
                )
                .map_err(|e| m.failure(e))?;
                m.acknowledged()?;
            }
            let result=(||{let actual=read::detail(api,"backup",&id.to_string(),Some(build))?;require_build(&actual,build)?;
                if actual.get("backupName").or_else(||actual.get("name"))!=self.doc.get("backup_name"){return Err(m.pending_error("original backup name is not confirmed; repeat this input without another rename"));}
                Ok(json!({"platform":"modus","backup_id":id,"backup_name":self.doc["backup_name"],"server_type":build,"renamed":true,"verified":true,"verification_scope":"exact_backup_name","operation_id":m.id(),"recovered":recovered}))
            })().map_err(|e|m.readback_error("backup rename needs original target readback",e))?;
            m.verified_with(json!({"result":result}))?;
            return Ok(result);
        }
        if !recovered {
            let ids = self.doc["backup_ids"].as_array().unwrap();
            let owned = read::backups(api, Some(build))?;
            for id in ids {
                if !owned.iter().any(|r| backup_id(r) == id.as_u64()) {
                    return Err(invalid(
                        "$.backup_ids",
                        "a selected backup is absent from the current account; inspect backup list",
                    ));
                }
                require_build(
                    &read::detail(api, "backup", &id.to_string(), Some(build))?,
                    build,
                )?;
            }
            m.prepare_with_evidence(json!({"build":build,"items":ids.iter().map(|id|json!({"backup_id":id,"status":"ready"})).collect::<Vec<_>>()}),control)?;
        }
        let mut e = m.evidence().unwrap().clone();
        let result = (|| {
            let mut links = None;
            for i in 0..e["items"].as_array().unwrap().len() {
                let item = e["items"][i].clone();
                let id = item["backup_id"]
                    .as_u64()
                    .ok_or_else(|| malformed("original delete batch lost its backup IDs"))?;
                if item["status"] == "done" {
                    continue;
                }
                if item["status"] == "deleting" {
                    if !absent(api, id, build)? {
                        return Err(m.pending_error(
                            "original backup deletion is uncertain; no DELETE was repeated",
                        ));
                    }
                } else {
                    require_build(
                        &read::detail(api, "backup", &id.to_string(), Some(build))?,
                        build,
                    )?;
                    if links.is_none() {
                        links = Some(references(api)?);
                    }
                    if let Some(used) = links.as_ref().unwrap().get(&id) {
                        e["items"][i] =
                            json!({"backup_id":id,"status":"protected","references":used});
                        m.remember(e.clone())?;
                        continue;
                    }
                    e["items"][i] = json!({"backup_id":id,"status":"deleting"});
                    m.remember(e.clone())?;
                    if let Err(error) = api.write(
                        Method::DELETE,
                        &format!("system/user/backup/delete/{id}"),
                        &[],
                        None,
                        Some(build),
                    ) {
                        if error.details["submission"] == "not_sent"
                            || crate::mutation::definitive_rejection(&error)
                        {
                            e["items"][i]["status"] = json!("ready");
                            m.remember(e.clone())?;
                        }
                        return Err(m.readback_error("backup deletion needs original ID readback; completed items are retained",error));
                    }
                    if !absent(api, id, build)? {
                        return Err(m.pending_error(
                            "deleted backup remains visible; no deletion will be repeated",
                        ));
                    }
                }
                e["items"][i] =
                    json!({"backup_id":id,"status":"done","deleted":true,"verified":true});
                m.remember(e.clone())?;
            }
            let complete = e["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["status"] == "done");
            let result = json!({"platform":"modus","server_type":build,"items":e["items"],"completed":complete,"operation_id":m.id(),"recovered":recovered,"verification_scope":"global_list_and_exact_detail_absence","reference_coverage":"all_five_modus_builds"});
            if !complete {
                return Err(ToolkitError::new("publish.modus_backup_in_use","selected backups are still referenced by configuration shares; completed deletions are retained",5).with_details(crate::output::public(result,&[])));
            }
            Ok(result)
        })();
        match result {
            Ok(result) => {
                m.verified_with(json!({"result":result}))?;
                Ok(result)
            }
            Err(mut error) => {
                if error.code != "publish.modus_backup_in_use" {
                    error=m.readback_error("backup batch remains pending; use the original input and completed IDs will not be deleted again",error);
                    error.details["items"] = crate::output::public(e["items"].clone(), &[]);
                }
                Err(error)
            }
        }
    }
    fn preview(&self, api: &Api) -> Result<Value> {
        let build = api.build(self.doc["server_type"].as_u64().map(|n| n as u8))?;
        let links = references(api)?;
        let mut out = Vec::new();
        for mut row in read::backups(api, Some(build))? {
            let id =
                backup_id(&row).ok_or_else(|| malformed("backup list omitted a numeric ID"))?;
            if self
                .doc
                .get("backup_ids")
                .is_some_and(|a| !a.as_array().unwrap().contains(&json!(id)))
            {
                continue;
            }
            let name = row
                .get("backupName")
                .or_else(|| row.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if self.doc["name_contains"]
                .as_str()
                .is_some_and(|s| !name.contains(s))
                || self.doc["name_equals"].as_str().is_some_and(|s| name != s)
            {
                continue;
            }
            if self.doc.get("created_before").is_some() || self.doc.get("created_after").is_some() {
                let when = row
                    .get("createTime")
                    .or_else(|| row.get("createdAt"))
                    .and_then(Value::as_str)
                    .and_then(date)
                    .ok_or_else(|| malformed("backup creation time is missing or invalid"))?;
                if self.doc["created_before"]
                    .as_str()
                    .is_some_and(|s| when >= date(s).unwrap())
                    || self.doc["created_after"]
                        .as_str()
                        .is_some_and(|s| when <= date(s).unwrap())
                {
                    continue;
                }
            }
            if row.get("serverType").is_none() {
                row = read::detail(api, "backup", &id.to_string(), Some(build))?;
            }
            if backup_build(&row)? != build {
                continue;
            }
            let refs = links.get(&id).cloned().unwrap_or_default();
            out.push(json!({"backup_id":id,"backup_name":row.get("backupName").or_else(||row.get("name")),"server_type":build,"deletable":refs.is_empty(),"references":refs}));
        }
        Ok(crate::output::public(
            json!({"platform":"modus","server_type":build,"items":out,"complete":true,"reference_coverage":"all_five_modus_builds","remote_write":false}),
            &[],
        ))
    }
}
fn date(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&Utc));
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(t) = NaiveDateTime::parse_from_str(s, format) {
            return Some(t.and_utc());
        }
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)
        .map(|d| d.and_utc())
}
fn backup_id(value: &Value) -> Option<u64> {
    identity(
        value
            .get("id")
            .or_else(|| value.get("backupId"))
            .unwrap_or(&Value::Null),
    )
    .and_then(|s| s.parse().ok())
}
fn backup_build(value: &Value) -> Result<u8> {
    value["serverType"]
        .as_u64()
        .filter(|n| *n <= 4)
        .map(|n| n as u8)
        .ok_or_else(|| malformed("backup serverType is missing or invalid"))
}
fn require_build(value: &Value, build: u8) -> Result<()> {
    if backup_build(value)? != build {
        Err(invalid(
            "$.server_type",
            "backup belongs to another build; select its actual server_type from backup list/get",
        ))
    } else {
        Ok(())
    }
}
fn absent(api: &Api, id: u64, build: u8) -> Result<bool> {
    if read::backups(api, Some(build))?
        .iter()
        .any(|r| backup_id(r) == Some(id))
    {
        return Ok(false);
    }
    Ok(read::optional_detail(api, "backup", &id.to_string(), Some(build))?.is_none())
}
fn references(api: &Api) -> Result<BTreeMap<u64, Vec<Value>>> {
    let mut found: BTreeMap<u64, Vec<Value>> = BTreeMap::new();
    let mut seen = BTreeMap::new();
    for build in 0..auth::BUILD_CODES.len() as u8 {
        let list = read::pages(
            api,
            Listing::Configs,
            Some(build),
            None,
            100,
            &json!({"mine":true}),
        )?;
        if list["total"].as_u64().is_none() {
            return Err(malformed(
                "configuration reference pagination omitted its total; deletion is blocked",
            ));
        }
        let ids = list["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                identity(&r["id"]).ok_or_else(|| malformed("configuration reference omitted an ID"))
            })
            .collect::<Result<Vec<_>>>()?;
        for chunk in ids.chunks(100) {
            let detail = read::rows(&data(api.post(
                "system/user/share/detail",
                &json!({"shareIds":chunk}),
                Some(build),
                false,
            )?))?;
            let mut exact = BTreeSet::new();
            for row in detail {
                let id = identity(
                    row.get("id")
                        .or_else(|| row.get("shareId"))
                        .unwrap_or(&Value::Null),
                )
                .ok_or_else(|| malformed("configuration detail omitted its identity"))?;
                if !chunk.contains(&id) || !exact.insert(id.clone()) {
                    return Err(malformed(
                        "configuration details do not match the reference list",
                    ));
                }
                let backup = identity(
                    row.get("backupId")
                        .or_else(|| row.get("backup_id"))
                        .unwrap_or(&Value::Null),
                )
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| {
                    malformed("configuration backup association is unknown; deletion is blocked")
                })?;
                if let Some(old) = seen.insert(id.clone(), backup) {
                    if old != backup {
                        return Err(malformed(
                            "configuration backup association changed across builds",
                        ));
                    }
                    continue;
                }
                found.entry(backup).or_default().push(
                    json!({"share_id":id,"title":row.get("title"),"checked_server_type":build}),
                );
            }
            if exact.len() != chunk.len() {
                return Err(malformed(
                    "configuration details are incomplete; deletion is blocked",
                ));
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
