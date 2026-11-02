use super::super::{Read, Resource, read::Attached, task::Client};
use super::*;
use crate::{contract, input, mutation::Mutation, provider::Provider};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
trait Api: read::Api {
    fn submit(&mut self, path: &str, body: &Value) -> Result<Value>;
}
impl Api for Attached<'_> {
    fn submit(&mut self, p: &str, b: &Value) -> Result<Value> {
        self.client.post(p, &[], b, true, self.control)
    }
}
pub struct BackupSelection {
    ids: Vec<String>,
    contains: Option<String>,
    equals: Option<String>,
    before: Option<DateTime<Utc>>,
    after: Option<DateTime<Utc>>,
}
fn admit_ids(ids: &[String]) -> Result<()> {
    if ids.is_empty() || ids.len() > 1000 || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
    {
        return Err(invalid(
            "$.backup_ids",
            "select 1..1000 unique nonempty exact DD backup SNs",
        ));
    }
    for sn in ids {
        Reference::parse(sn).map_err(|_| invalid("$.backup_ids", "invalid exact DD backup SN"))?;
    }
    Ok(())
}
fn date(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(d) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(d.and_utc());
        }
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)
        .map(|d| d.and_utc())
}
fn created(v: &Value) -> Result<DateTime<Utc>> {
    let text = if let Some(s) = v.as_str() {
        if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
            return Err(data("DD backup creation time is not Unix seconds"));
        }
        s.to_owned()
    } else if v.is_number() {
        v.to_string()
    } else {
        return Err(data("DD backup creation time is unavailable"));
    };
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    if !fraction.bytes().all(|c| c.is_ascii_digit()) || fraction.len() > 9 {
        return Err(data("DD backup creation time has unsupported precision"));
    }
    let mut seconds = whole
        .parse::<i64>()
        .map_err(|_| data("DD backup creation time is outside its supported range"))?;
    let mut nanos = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u32>()
            .map_err(|_| data("DD backup creation time is invalid"))?
            * 10u32.pow(9 - fraction.len() as u32)
    };
    if whole.starts_with('-') && nanos > 0 {
        seconds = seconds
            .checked_sub(1)
            .ok_or_else(|| data("DD backup creation time is outside its supported range"))?;
        nanos = 1_000_000_000 - nanos;
    }
    DateTime::from_timestamp(seconds, nanos)
        .ok_or_else(|| data("DD backup creation time is outside its supported range"))
}
impl BackupSelection {
    pub fn new(
        ids: Vec<String>,
        name_contains: Option<String>,
        name_equals: Option<String>,
        created_before: Option<&str>,
        created_after: Option<&str>,
    ) -> Result<Self> {
        let filtered = name_contains.is_some()
            || name_equals.is_some()
            || created_before.is_some()
            || created_after.is_some();
        if !ids.is_empty() {
            admit_ids(&ids).map_err(|e| ToolkitError::arguments(e.message))?;
        } else if !filtered {
            return Err(ToolkitError::arguments(
                "backup delete-preview requires --ids as a JSON array or an explicit name/date filter",
            ));
        }
        for v in [&name_contains, &name_equals].into_iter().flatten() {
            if v.len() > 4096 || v.contains('\0') {
                return Err(ToolkitError::arguments(
                    "backup name filter exceeds its supported text range",
                ));
            }
        }
        let parse = |s: &str| {
            date(s).ok_or_else(|| {
                ToolkitError::arguments(
                    "backup date filter requires ISO 8601; dates without an offset use UTC",
                )
            })
        };
        Ok(Self {
            ids,
            contains: name_contains,
            equals: name_equals,
            before: created_before.map(parse).transpose()?,
            after: created_after.map(parse).transpose()?,
        })
    }
    fn select(&self, rows: &[Value]) -> Result<Vec<String>> {
        if self.contains.is_none()
            && self.equals.is_none()
            && self.before.is_none()
            && self.after.is_none()
        {
            return Ok(self.ids.clone());
        }
        let mut ids = Vec::new();
        for row in rows {
            let sn = reference(row)?;
            let name = row["name"].as_str().unwrap_or("");
            if !self.ids.is_empty() && !self.ids.contains(&sn)
                || self.contains.as_ref().is_some_and(|s| !name.contains(s))
                || self.equals.as_ref().is_some_and(|s| s != name)
            {
                continue;
            }
            if self.before.is_some() || self.after.is_some() {
                let t = created(&row["ctime"])?;
                if self.before.is_some_and(|d| t >= d) || self.after.is_some_and(|d| t <= d) {
                    continue;
                }
            }
            ids.push(sn);
        }
        Ok(ids)
    }
    pub fn execute(&self, client: &Client, control: &OperationControl) -> Result<Value> {
        self.execute_with(&mut Attached { client, control }, control)
    }
    fn execute_with(&self, api: &mut impl read::Api, control: &OperationControl) -> Result<Value> {
        control.check()?;
        let rows = list(api)?;
        let ids = self.select(&rows)?;
        if ids.is_empty() {
            return Ok(
                json!({"items":[],"deletable":false,"matched":0,"references_complete":true,"reference_check":"not_needed_no_matches","remote_write":false}),
            );
        }
        preview(api, &rows, &ids, control)
    }
}
fn references(
    api: &mut impl read::Api,
    rows: &[Value],
    control: &OperationControl,
) -> Result<BTreeMap<String, Vec<Value>>> {
    let options = Read::GameTypes.execute(api, control)?;
    let mut games = BTreeSet::new();
    for row in options["items"].as_array().unwrap() {
        games.insert(forms::game(row)?);
    }
    if games.is_empty() {
        return Err(data(
            "DD game types are unavailable for complete backup reference checks",
        ));
    }
    for row in rows {
        if row.get("game_type").is_some_and(forms::truth) {
            games.insert(forms::game(row)?);
        }
    }
    let mut refs: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for game in games {
        for row in read::all_author_required_total(api, Resource::Config, game, control)? {
            let sn = forms::reference(&row)?;
            let backup = if let Some(s) = row["backup_sn"].as_str().filter(|s| !s.is_empty()) {
                Reference::parse(s)
                    .map_err(|_| data("DD configuration has an invalid backup association"))?;
                s.into()
            } else {
                let raw = read::detail(api, Resource::Config, &Reference::parse(&sn)?)?;
                if raw["is_owner"] == false || forms::reference(&raw)? != sn {
                    return Err(data(
                        "DD reference detail does not identify the original owned configuration",
                    ));
                }
                let b = raw["backup_sn"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| data("DD configuration backup association is unknown"))?;
                Reference::parse(b)
                    .map_err(|_| data("DD configuration has an invalid backup association"))?;
                b.into()
            };
            refs.entry(backup).or_default().push(json!({"sn":sn,"title":row.get("title").or_else(||row.get("name")),"game_type":game.get()}));
        }
    }
    Ok(refs)
}
fn preview(
    api: &mut impl read::Api,
    rows: &[Value],
    ids: &[String],
    control: &OperationControl,
) -> Result<Value> {
    let refs = references(api, rows, control).map_err(|mut e| {
        if !e.details.is_object() {
            e.details = json!({});
        }
        e.details["stage"] = json!("backup_reference_check");
        e
    })?;
    let mut items = Vec::new();
    let mut all = true;
    for sn in ids {
        let row = rows.iter().find(|r| reference(r).ok().as_ref() == Some(sn));
        let references = refs.get(sn).cloned().unwrap_or_default();
        let deletable = row.is_some() && references.is_empty();
        all &= deletable;
        items.push(json!({"sn":sn,"name":row.and_then(|r|r.get("name")),"present":row.is_some(),"references":references,"deletable":deletable,"reason":if !references.is_empty(){Some("configuration_share_reference")}else if row.is_none(){Some("not_found")}else{None}}));
    }
    Ok(forms::public(
        json!({"items":items,"deletable":all,"references_complete":true,"matched":ids.len(),"remote_write":false}),
    ))
}
pub struct BackupRename {
    sn: String,
    name: String,
}
pub struct BackupDelete {
    ids: Vec<String>,
}
impl BackupRename {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let v = input::document(bytes)?;
        contract::input_shape(Provider::Dd, "backup", "rename")?.admit(&v)?;
        let sn = v["sn"].as_str().unwrap();
        Reference::parse(sn).map_err(|_| invalid("$.sn", "invalid exact DD backup SN"))?;
        let name = v["name"].as_str().unwrap();
        if name.trim().is_empty() {
            return Err(invalid(
                "$.name",
                "backup name must contain 1..14 characters",
            ));
        }
        Ok(Self {
            sn: sn.into(),
            name: name.into(),
        })
    }
    pub fn schema() -> Value {
        let mut s = contract::input_shape(Provider::Dd, "backup", "rename")
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        s["properties"]["sn"]["maxLength"] = json!(1024);
        s
    }
    pub fn plan(&self) -> Value {
        forms::public(
            json!({"platform":"dd","resource":"backup","action":"rename","sn":self.sn,"name":self.name,"remote_read":false,"remote_write":false,"credential_reads":false,"uncertain_submission_replay":false}),
        )
    }
    pub fn execute(
        &self,
        home: &Path,
        client: &Client,
        control: &OperationControl,
    ) -> Result<Value> {
        let actor = author(client, control)?;
        self.execute_with(home, &actor, &mut Attached { client, control }, control)
    }
    fn execute_with(
        &self,
        home: &Path,
        actor: &str,
        api: &mut impl Api,
        control: &OperationControl,
    ) -> Result<Value> {
        control.check()?;
        let input = json!({"author":actor,"sn":self.sn,"name":self.name});
        let mut m = Mutation::open(
            home,
            Provider::Dd,
            "backup",
            target(actor, &self.sn)?,
            "rename",
            &input,
            control,
        )?;
        let original = m
            .evidence()
            .map(|v| {
                serde_json::from_value::<RenameContext>(v.clone())
                    .map_err(|_| ToolkitError::invalid("invalid original DD backup rename context"))
            })
            .transpose()?;
        let hash = lycheedev_core::storage::sha256(self.name.as_bytes());
        if let Some(c) = &original {
            if c.sn != self.sn || c.name_sha256 != hash || !c.before.is_object() {
                return Err(ToolkitError::invalid(
                    "DD backup rename context differs from its original target",
                ));
            }
        }
        let raw = selected(api, &self.sn).map_err(|e| {
            if m.pending() {
                m.readback_error("original DD backup rename readback is pending", e)
            } else {
                e
            }
        })?;
        if raw["name"].as_str() == Some(&self.name) {
            if m.pending() {
                m.verified()?;
            }
            return Ok(forms::public(
                json!({"sn":self.sn,"name":self.name,"changed":false,"operation_id":m.id(),"recovered":original.is_some(),"submitted_this_call":false,"verification":"fresh_backup_name","readback":read::backup_view(&raw,&self.sn)?}),
            ));
        }
        if m.pending() {
            return Err(m.pending_error("the original DD backup name is not visible; recover by reading and do not submit another rename"));
        }
        let context = RenameContext {
            sn: self.sn.clone(),
            name_sha256: hash,
            before: forms::public(read::backup_view(&raw, &self.sn)?),
        };
        m.prepare_with_evidence(serde_json::to_value(&context)?, control)?;
        api.submit(
            "/backup/modify_name",
            &json!({"sn":self.sn,"name":self.name}),
        )
        .map_err(|e| m.failure(e))?;
        m.acknowledged().map_err(|_| {
            m.pending_error("DD rename was accepted; finish saving the original acknowledgement")
        })?;
        let raw = selected(api, &self.sn)
            .map_err(|e| m.readback_error("DD backup rename readback is pending", e))?;
        if raw["name"].as_str() != Some(&self.name) {
            return Err(m.pending_error("DD backup rename was accepted but its name is not visible; read back the original request"));
        }
        m.verified()?;
        Ok(forms::public(
            json!({"sn":self.sn,"name":self.name,"changed":true,"operation_id":m.id(),"recovered":false,"submitted_this_call":true,"verification":"fresh_backup_name","readback":read::backup_view(&raw,&self.sn)?}),
        ))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameContext {
    sn: String,
    name_sha256: String,
    before: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteContext {
    sn: String,
    before: Value,
    deleted: bool,
}
impl BackupDelete {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let v = input::document(bytes)?;
        contract::input_shape(Provider::Dd, "backup", "delete")?.admit(&v)?;
        let ids = v["backup_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid("$.backup_ids", "backup SNs must be strings"))
            })
            .collect::<Result<Vec<_>>>()?;
        admit_ids(&ids)?;
        Ok(Self { ids })
    }
    pub fn schema() -> Value {
        let mut s = contract::input_shape(Provider::Dd, "backup", "delete")
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        s["properties"]["backup_ids"]["items"] =
            json!({"type":"string","minLength":1,"maxLength":1024});
        s["properties"]["backup_ids"]["uniqueItems"] = json!(true);
        s
    }
    pub fn plan(&self) -> Value {
        json!({"platform":"dd","resource":"backup","action":"delete","backup_ids":self.ids,"confirm_delete":true,"remote_read":false,"remote_write":false,"credential_reads":false,"execution_requires":["original retained DDT","complete current backup membership","all game types and complete author reference pages","a fresh reference check immediately before each delete POST"],"uncertain_submission_replay":false})
    }
    pub fn execute(
        &self,
        home: &Path,
        client: &Client,
        control: &OperationControl,
    ) -> Result<Value> {
        let actor = author(client, control)?;
        self.execute_with(home, &actor, &mut Attached { client, control }, control)
    }
    fn execute_with(
        &self,
        home: &Path,
        actor: &str,
        api: &mut impl Api,
        control: &OperationControl,
    ) -> Result<Value> {
        control.check()?;
        let mut sorted = self.ids.clone();
        sorted.sort();
        let mut leases = BTreeMap::new();
        for sn in sorted {
            let input = json!({"author":actor,"sn":sn,"confirm_delete":true});
            leases.insert(
                sn.clone(),
                Mutation::open(
                    home,
                    Provider::Dd,
                    "backup",
                    target(actor, &sn)?,
                    "delete",
                    &input,
                    control,
                )?,
            );
        }
        let mut completed = Vec::new();
        let work = (|| {
            let rows = list(api)?;
            let mut fresh = Vec::new();
            for sn in &self.ids {
                let m = leases.get_mut(sn).unwrap();
                let c = m
                    .evidence()
                    .map(|v| {
                        serde_json::from_value::<DeleteContext>(v.clone()).map_err(|_| {
                            ToolkitError::invalid("invalid original DD backup deletion context")
                        })
                    })
                    .transpose()?;
                if let Some(c) = &c {
                    if c.sn != *sn || !c.before.is_object() {
                        return Err(ToolkitError::invalid(
                            "DD backup deletion context differs from its original SN",
                        ));
                    }
                }
                let present = rows.iter().any(|r| reference(r).ok().as_ref() == Some(sn));
                if m.pending() || c.as_ref().is_some_and(|c| c.deleted) {
                    let mut c=c.ok_or_else(||m.pending_error("retain the original DD backup deletion while its context is incomplete"))?;
                    if !present {
                        c.deleted = true;
                        if m.pending() {
                            m.verified_with(serde_json::to_value(&c)?)?;
                        }
                        completed.push(deleted(&c, m, true));
                        continue;
                    }
                    if m.pending() {
                        return Err(with_sn(m.pending_error("the original DD backup remains present; do not replay its delete POST"),sn));
                    }
                }
                fresh.push(sn.clone());
            }
            if !fresh.is_empty() {
                let p = preview(api, &rows, &fresh, control)?;
                if p["deletable"] != true {
                    return Err(blocked(p));
                }
            }
            for sn in fresh {
                control.check()?;
                let rows = list(api)?;
                let p = preview(api, &rows, std::slice::from_ref(&sn), control)?;
                if p["deletable"] != true {
                    return Err(with_sn(blocked(p), &sn));
                }
                let row = rows
                    .iter()
                    .find(|r| reference(r).ok().as_ref() == Some(&sn))
                    .unwrap();
                let mut c = DeleteContext {
                    sn: sn.clone(),
                    before: forms::public(read::backup_view(row, &sn)?),
                    deleted: false,
                };
                let m = leases.get_mut(&sn).unwrap();
                m.prepare_with_evidence(serde_json::to_value(&c)?, control)?;
                api.submit("/backup/delete", &json!({"sn":sn}))
                    .map_err(|e| with_sn(m.failure(e), &sn))?;
                m.acknowledged().map_err(|_| {
                    with_sn(
                        m.pending_error(
                            "DD backup delete was accepted; finish saving its acknowledgement",
                        ),
                        &sn,
                    )
                })?;
                let after = list(api).map_err(|e| {
                    with_sn(
                        m.readback_error("DD backup deletion readback is pending", e),
                        &sn,
                    )
                })?;
                if after
                    .iter()
                    .any(|r| reference(r).ok().as_ref() == Some(&sn))
                {
                    return Err(with_sn(m.pending_error("deleted DD backup is still present; recovery only reads the original SN"),&sn));
                }
                c.deleted = true;
                m.verified_with(serde_json::to_value(&c)?).map_err(|_| {
                    with_sn(
                        m.pending_error(
                            "DD backup deletion is verified; finish saving the original receipt",
                        ),
                        &sn,
                    )
                })?;
                completed.push(deleted(&c, m, false));
            }
            Ok(())
        })();
        if let Err(mut error) = work {
            if !error.details.is_object() {
                error.details = json!({"cause":error.details});
            }
            error.details["completed"] = json!(completed);
            error.details["automatic_retry"] = json!(false);
            return Err(error);
        }
        Ok(
            json!({"items":completed,"deleted":completed.len(),"complete":true,"replay_performed":false}),
        )
    }
}
fn deleted(c: &DeleteContext, m: &Mutation, recovered: bool) -> Value {
    forms::public(
        json!({"sn":c.sn,"deleted":true,"before":c.before,"operation_id":m.id(),"recovered":recovered,"submitted_this_call":!recovered,"verification":"complete_backup_list_absence","readback":{"present":false,"complete":true}}),
    )
}
fn blocked(preview: Value) -> ToolkitError {
    ToolkitError::new(
        "publish.dd_backup_delete_blocked",
        "DD backup deletion is blocked by a missing backup or configuration-share references",
        5,
    )
    .with_details(preview)
}
fn with_sn(mut e: ToolkitError, sn: &str) -> ToolkitError {
    if !e.details.is_object() {
        e.details = json!({"cause":e.details});
    }
    e.details["sn"] = json!(sn);
    e
}
#[cfg(test)]
mod tests;
