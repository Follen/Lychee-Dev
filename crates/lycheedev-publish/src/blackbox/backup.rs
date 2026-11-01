use super::{
    protocol::{Api, Session, identifier},
    read::Listing,
    write, *,
};
use crate::{
    contract::input_shape,
    input::{document, invalid},
    provider::Provider,
};
use std::collections::{BTreeMap, BTreeSet};
mod bundle;
mod upload;
pub use bundle::{BackupGenerate, scan as scan_backup};
pub use upload::{BackupCreate, BackupUpload};

pub struct BackupWrite {
    action: String,
    doc: Value,
}
impl BackupWrite {
    pub fn parse(action: &str, bytes: &[u8]) -> Result<Self> {
        let doc = document(bytes)?;
        input_shape(Provider::Blackbox, "backup", action)?.admit(&doc)?;
        let doc = Value::Object(doc);
        if action == "rename" {
            reference(&doc["backup_id"])?;
            forms::text(&doc["name"], "$.name", true)?;
        } else {
            validate_selection(&doc)?;
        }
        Ok(Self {
            action: action.into(),
            doc,
        })
    }
    pub fn schema(action: &str) -> Value {
        let mut s = input_shape(Provider::Blackbox, "backup", action)
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        if action != "rename" {
            s["properties"]["backup_ids"]["items"] = json!({"type":"string","minLength":1});
            s["properties"]["backup_ids"]["uniqueItems"] = json!(true);
            s["oneOf"] = json!([{"required":["backup_ids"],"not":{"anyOf":[{"required":["backup_id"]},{"required":["name_contains"]},{"required":["name_equals"]},{"required":["created_before"]},{"required":["created_after"]}]}},{"required":["backup_id"],"not":{"anyOf":[{"required":["backup_ids"]},{"required":["name_contains"]},{"required":["name_equals"]},{"required":["created_before"]},{"required":["created_after"]}]}},{"anyOf":[{"required":["name_contains"]},{"required":["name_equals"]},{"required":["created_before"]},{"required":["created_after"]}],"not":{"anyOf":[{"required":["backup_id"]},{"required":["backup_ids"]}]}}]);
        }
        s["description"] = json!(
            "Explicit IDs or conjunctive name/time filters, never both. Times are Unix seconds, with strict before/after comparisons. --plan is local only; delete-preview reads complete current-account backup and share lists and resolves missing BackupID references. Delete freezes the original selection, rechecks each remaining item and preserves per-item results. Repeat the exact input for recovery; unknown deletes only read for absence."
        );
        s
    }
    pub fn plan(&self) -> Value {
        json!({"platform":"blackbox","resource":"backup","action":self.action,"planned":true,"credential_reads":false,"remote_read":false,"remote_write":false,"local_write":false,"selection":self.doc,"remote_validation":"Run publish blackbox backup delete-preview to check current selections and share references."})
    }
    pub fn execute(&self, home: &Path, control: &OperationControl) -> Result<Value> {
        auth::Auth::open(home, control)?.finish(|auth| {
            let mut session = auth.renew(false)?;
            self.run(home, &auth.api, &mut session, control)
        })
    }
    pub(super) fn run(
        &self,
        home: &Path,
        api: &Api,
        session: &mut Session,
        control: &OperationControl,
    ) -> Result<Value> {
        if self.action == "delete-preview" {
            return preview(api, session, &self.doc);
        }
        if self.action == "rename" {
            let id = reference(&self.doc["backup_id"])?;
            let mut m = write::open(home, session, "backup", &id, "rename", &self.doc, control)?;
            let recovered = m.pending();
            if !recovered {
                read::execute(api, session, Query::Backup(id.clone()))?;
                m.prepare(control)?;
                api.post(
                    session,
                    "/wow/rename_user_configuration_backup/",
                    &[
                        ("backup_id", id.clone()),
                        ("new_name", self.doc["name"].as_str().unwrap().into()),
                    ],
                    &json!({}),
                )
                .map_err(|e| m.failure(e))?;
                m.acknowledged()?;
            }
            let current = read::execute(api, session, Query::Backup(id.clone()))
                .map_err(|e| m.readback_error("backup rename needs its original readback", e))?;
            if current["backup"]["Name"] != self.doc["name"] {
                return Err(m.pending_error("backup name has not converged; repeat the original input to read without resending"));
            }
            let result = json!({"platform":"blackbox","backup_id":id,"name":self.doc["name"],"verified":true,"operation_id":m.id(),"recovered":recovered});
            m.verified_with(json!({"result":result}))?;
            return Ok(result);
        }
        let mut m = write::open(
            home,
            session,
            "backup-delete",
            "batch",
            "delete",
            &self.doc,
            control,
        )?;
        if let Some(result) = write::completed(&m) {
            return Ok(result);
        }
        let recovered = m.pending();
        let mut evidence = if let Some(e) = m.evidence() {
            e.clone()
        } else {
            let plan = preview(api, session, &self.doc)?;
            if plan["allowed"] != true {
                return Err(ToolkitError::new(
                    "publish.blackbox_backup_in_use",
                    "configuration shares still reference the selected backups",
                    5,
                )
                .with_details(plan));
            }
            json!({"items":plan["items"].as_array().unwrap().iter().map(|r|json!({"backup_id":r["backup_id"],"state":"queued"})).collect::<Vec<_>>()})
        };
        if !recovered {
            for item in evidence["items"].as_array_mut().unwrap() {
                if item["state"] == "rejected" {
                    item["state"] = json!("queued");
                }
            }
            m.prepare_with_evidence(evidence.clone(), control)?;
        }
        let result = (|| {
            for index in 0..evidence["items"].as_array().unwrap().len() {
                if evidence["items"][index]["state"] == "verified" {
                    continue;
                }
                let id = evidence["items"][index]["backup_id"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                if evidence["items"][index]["state"] == "queued" {
                    let rows = read::backups(api, session)?;
                    let row=rows.iter().find(|r|identifier(&r["ID"]).as_deref()==Some(&id)).ok_or_else(||write::not_found("queued backup disappeared before deletion; original batch selection is retained"))?;
                    if !filter_matches(&self.doc, row)? {
                        return Err(invalid(
                            "$.selection",
                            "original backup no longer matches its deletion filters",
                        ));
                    }
                    let fresh = references(api, session, std::slice::from_ref(row))?;
                    if !fresh[&id].is_empty() {
                        return Err(ToolkitError::new(
                            "publish.blackbox_backup_in_use",
                            "backup acquired configuration references; original batch is retained",
                            5,
                        )
                        .with_details(json!({"backup_id":id,"referenced_by":fresh[&id]})));
                    }
                    evidence["items"][index]["state"] = json!("submitted");
                    m.remember(evidence.clone())?;
                    if let Err(error) = api.post(
                        session,
                        "/wow/remove_user_configuration_backup/",
                        &[("backup_id", id.clone())],
                        &json!({}),
                    ) {
                        let error = m.failure(error);
                        if matches!(
                            error.details["submission"].as_str(),
                            Some("not_sent" | "rejected")
                        ) {
                            evidence["items"][index]["state"] = json!("rejected");
                            m.remember(evidence.clone())?;
                        }
                        return Err(error);
                    }
                }
                if read::backups(api, session)?
                    .iter()
                    .any(|r| identifier(&r["ID"]).as_deref() == Some(&id))
                {
                    return Err(m.pending_error("original backup delete awaits complete absence readback; no request was replayed"));
                }
                evidence["items"][index]["state"] = json!("verified");
                m.remember(evidence.clone())?;
            }
            Ok(())
        })();
        if let Err(mut error) = result {
            error.details["items"] = evidence["items"].clone();
            error.details["operation_id"] = json!(m.id());
            return Err(error);
        }
        let result = json!({"platform":"blackbox","verified":true,"complete":true,"items":evidence["items"],"operation_id":m.id(),"recovered":recovered});
        m.verified_with(json!({"result":result}))?;
        Ok(result)
    }
}
fn reference(value: &Value) -> Result<String> {
    let s = identifier(value)
        .ok_or_else(|| invalid("$.backup_id", "expected an opaque backup identity"))?;
    read::reference(&s)?;
    Ok(s)
}
fn validate_selection(doc: &Value) -> Result<()> {
    let ids = doc.get("backup_ids");
    let single = doc.get("backup_id");
    let filters = [
        "name_contains",
        "name_equals",
        "created_before",
        "created_after",
    ]
    .iter()
    .any(|k| doc.get(*k).is_some());
    if (ids.is_some() || single.is_some()) == filters || ids.is_some() && single.is_some() {
        return Err(invalid(
            "$.selection",
            "select backup_ids, backup_id, or name/time filters exclusively",
        ));
    }
    if let Some(ids) = ids {
        let mut seen = BTreeSet::new();
        for value in ids.as_array().unwrap() {
            if !seen.insert(reference(value)?) {
                return Err(invalid("$.backup_ids", "backup identities must be unique"));
            }
        }
    }
    if let Some(id) = single {
        reference(id)?;
    }
    for key in ["name_contains", "name_equals"] {
        if let Some(v) = doc.get(key) {
            forms::text(v, &format!("$.{key}"), true)?;
        }
    }
    if doc["created_before"]
        .as_u64()
        .zip(doc["created_after"].as_u64())
        .is_some_and(|(before, after)| before <= after)
    {
        return Err(invalid(
            "$.created_before",
            "must be later than created_after",
        ));
    }
    Ok(())
}
fn filter_matches(doc: &Value, row: &Value) -> Result<bool> {
    for key in ["name_contains", "name_equals"] {
        if let Some(value) = doc[key].as_str() {
            let name = row["Name"]
                .as_str()
                .ok_or_else(|| protocol_error("backup name is missing; selection is incomplete"))?;
            if (key == "name_contains" && !name.contains(value))
                || (key == "name_equals" && name != value)
            {
                return Ok(false);
            }
        }
    }
    for key in ["created_before", "created_after"] {
        if let Some(value) = doc[key].as_u64() {
            let time = row["SaveTime"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .ok_or_else(|| {
                    protocol_error("backup creation time is missing; selection is incomplete")
                })?;
            if (key == "created_before" && time >= value as f64)
                || (key == "created_after" && time <= value as f64)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
fn preview(api: &Api, session: &mut Session, doc: &Value) -> Result<Value> {
    let rows = read::backups(api, session)?;
    let explicit = if let Some(values) = doc["backup_ids"].as_array() {
        Some(values.iter().map(reference).collect::<Result<Vec<_>>>()?)
    } else {
        doc.get("backup_id")
            .map(reference)
            .transpose()?
            .map(|s| vec![s])
    };
    if explicit.as_ref().is_some_and(|ids| {
        ids.iter().any(|id| {
            !rows
                .iter()
                .any(|r| identifier(&r["ID"]).as_ref() == Some(id))
        })
    }) {
        return Err(invalid(
            "$.backup_ids",
            "one or more backups are absent from the complete current account list",
        ));
    }
    let mut selected = Vec::new();
    for row in &rows {
        if explicit
            .as_ref()
            .map(|ids| ids.contains(&identifier(&row["ID"]).unwrap()))
            .unwrap_or(filter_matches(doc, row)?)
        {
            selected.push(row.clone());
        }
    }
    let references = references(api, session, &selected)?;
    let items=selected.into_iter().map(|row|{let id=identifier(&row["ID"]).unwrap();json!({"backup_id":id,"name":row["Name"],"referenced_by":references[&id],"deletable":references[&id].is_empty()})}).collect::<Vec<_>>();
    Ok(
        json!({"platform":"blackbox","complete":true,"allowed":items.iter().all(|r|r["deletable"]==true),"items":items}),
    )
}
fn references(
    api: &Api,
    session: &mut Session,
    rows: &[Value],
) -> Result<BTreeMap<String, Vec<String>>> {
    let mut refs = rows
        .iter()
        .map(|r| Ok((reference(&r["ID"])?, Vec::new())))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let branches = rows
        .iter()
        .map(forms::backup_branch)
        .collect::<Result<BTreeSet<_>>>()?;
    for branch in branches {
        for summary in read::collect(api, session, Listing::Configs(branch))? {
            let id = identifier(&summary["ShareID"]).unwrap();
            let backup = if let Some(id) = identifier(&summary["BackupID"]) {
                id
            } else {
                reference(
                    &read::execute(api, session, Query::Config(id.clone()))?["config"]["BackupID"],
                )?
            };
            if let Some(values) = refs.get_mut(&backup) {
                values.push(id);
            }
        }
    }
    for values in refs.values_mut() {
        values.sort();
        values.dedup();
    }
    Ok(refs)
}

#[cfg(test)]
mod tests;
