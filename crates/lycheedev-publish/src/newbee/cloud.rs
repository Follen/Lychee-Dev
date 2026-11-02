use super::*;
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::Mutation,
    provider::Provider,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};

pub enum BackupRead {
    List,
    Get(NonZeroU64),
    DeletePreview(BackupSelection),
}
pub struct BackupSelection {
    ids: Vec<NonZeroU64>,
    name_contains: Option<String>,
    name_equals: Option<String>,
    before: Option<DateTime<Utc>>,
    after: Option<DateTime<Utc>>,
}
impl BackupSelection {
    pub fn new(
        ids: Vec<NonZeroU64>,
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
            admit_ids(&ids)?;
        } else if !filtered {
            return Err(ToolkitError::arguments(
                "backup delete-preview requires --ids or at least one name/date filter",
            ));
        }
        let date = |value: &str| {
            timestamp(&json!(value)).ok_or_else(||ToolkitError::arguments("backup creation filters require a valid ISO 8601 date; dates without an offset use UTC"))
        };
        Ok(Self {
            ids,
            name_contains,
            name_equals,
            before: created_before.map(date).transpose()?,
            after: created_after.map(date).transpose()?,
        })
    }
    fn filtered(&self) -> bool {
        self.name_contains.is_some()
            || self.name_equals.is_some()
            || self.before.is_some()
            || self.after.is_some()
    }
    fn select(&self, rows: &[Value]) -> Result<Vec<NonZeroU64>> {
        let mut selected = Vec::new();
        for row in rows {
            let id = NonZeroU64::new(wire_id(row["t_id"].clone())?).unwrap();
            let name = row["t_name"].as_str().unwrap_or("");
            if (!self.ids.is_empty() && !self.ids.contains(&id))
                || self
                    .name_contains
                    .as_ref()
                    .is_some_and(|value| !name.contains(value))
                || self.name_equals.as_ref().is_some_and(|value| name != value)
            {
                continue;
            }
            if self.before.is_some() || self.after.is_some() {
                let date = timestamp(&row["t_create_time"])
                    .ok_or_else(|| remote("selected cloud backup has an invalid creation date"))?;
                if self.before.is_some_and(|value| date >= value)
                    || self.after.is_some_and(|value| date <= value)
                {
                    continue;
                }
            }
            selected.push(id);
        }
        Ok(selected)
    }
}
fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(value) = value.as_f64().filter(|value| value.is_finite()) {
        let seconds = if value > 100_000_000_000.0 {
            value / 1000.0
        } else {
            value
        };
        let whole = seconds.floor();
        if whole < i64::MIN as f64 || whole > i64::MAX as f64 {
            return None;
        }
        return DateTime::from_timestamp(
            whole as i64,
            ((seconds - whole) * 1_000_000_000.0)
                .round()
                .min(999_999_999.0) as u32,
        );
    }
    let text = value.as_str()?;
    if let Ok(value) = DateTime::parse_from_rfc3339(text) {
        return Some(value.with_timezone(&Utc));
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(value) = NaiveDateTime::parse_from_str(text, format) {
            return Some(value.and_utc());
        }
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)
        .map(|value| value.and_utc())
}
pub struct BackupRename {
    id: NonZeroU64,
    name: String,
}
pub struct BackupDelete {
    ids: Vec<NonZeroU64>,
}
impl BackupRename {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value = document(bytes)?;
        input_shape(Provider::Newbee, "backup", "rename")?.admit(&value)?;
        let name = value["name"].as_str().unwrap();
        if name.trim().is_empty() {
            return Err(invalid("$.name", "backup name must not be blank"));
        }
        Ok(Self {
            id: NonZeroU64::new(value["id"].as_u64().unwrap()).unwrap(),
            name: name.to_owned(),
        })
    }
    pub fn schema() -> Value {
        let mut value = input_shape(Provider::Newbee, "backup", "rename")
            .unwrap()
            .schema();
        value.as_object_mut().unwrap().remove("x-validation-scope");
        value
    }
}
impl BackupDelete {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value = document(bytes)?;
        input_shape(Provider::Newbee, "backup", "delete")?.admit(&value)?;
        let ids = value["backup_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .and_then(NonZeroU64::new)
                    .ok_or_else(|| invalid("$.backup_ids", "backup IDs must be positive integers"))
            })
            .collect::<Result<Vec<_>>>()?;
        admit_ids(&ids)
            .map_err(|_| invalid("$.backup_ids", "select 1..1000 unique positive backup IDs"))?;
        Ok(Self { ids })
    }
    pub fn schema() -> Value {
        let mut value = input_shape(Provider::Newbee, "backup", "delete")
            .unwrap()
            .schema();
        value.as_object_mut().unwrap().remove("x-validation-scope");
        value["properties"]["backup_ids"]["items"] = json!({"type":"integer","minimum":1});
        value["properties"]["backup_ids"]["uniqueItems"] = json!(true);
        value
    }
}
fn admit_ids(ids: &[NonZeroU64]) -> Result<()> {
    if ids.is_empty()
        || ids.len() > 1000
        || ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
    {
        return Err(ToolkitError::arguments(
            "select 1..1000 unique positive backup IDs",
        ));
    }
    Ok(())
}
impl Newbee {
    pub(super) fn backup_post(
        &mut self,
        path: &str,
        body: &Value,
        mutation: bool,
    ) -> Result<Value> {
        if self.session.is_none() {
            self.session = Some(CreatorSession::open(&self.control)?);
        }
        let headers = self
            .session
            .as_mut()
            .unwrap()
            .desktop_headers(&self.http, &self.control)?;
        envelope(
            self.http
                .post(Origin::NewbeeCreator, path, headers, body, mutation)?,
            path,
            mutation,
        )
    }
    pub(super) fn cloud_rows(&mut self) -> Result<Vec<Value>> {
        let mut value = self.backup_post("/v3/share/list", &json!({}), false)?;
        if value
            .as_array()
            .is_some_and(|rows| rows.first().is_some_and(Value::is_array))
        {
            if value.as_array().unwrap().len() != 1 {
                return Err(remote(
                    "cloud backup list contains unexplained result groups",
                ));
            }
            value = value.as_array_mut().unwrap().remove(0);
        }
        let rows = array(value)?;
        let mut ids = BTreeSet::new();
        for row in &rows {
            if !row.is_object() || !ids.insert(wire_id(row["t_id"].clone())?) {
                return Err(remote(
                    "cloud backup list has invalid or duplicate identities",
                ));
            }
        }
        Ok(rows)
    }
    pub fn read_backup(&mut self, request: &BackupRead) -> Result<Value> {
        let result = match request {
            BackupRead::List => {
                let items = self
                    .cloud_rows()?
                    .iter()
                    .map(summary)
                    .collect::<Result<Vec<_>>>()?;
                json!({"total":items.len(),"items":items,"coverage":"complete"})
            }
            BackupRead::Get(id) => {
                let row = self
                    .cloud_rows()?
                    .into_iter()
                    .find(|row| {
                        wire_uint(row["t_id"].clone(), 0).is_ok_and(|value| value == id.get())
                    })
                    .ok_or_else(|| {
                        ToolkitError::new(
                            "publish.backup_not_found",
                            "selected cloud backup was not found",
                            5,
                        )
                    })?;
                let mut safe = row.clone();
                if safe.get("wtflist").is_none() {
                    safe["wtflist"] = safe.get("t_WTF_list").cloned().unwrap_or(Value::Null);
                }
                let mut result = config::backup(&safe)?;
                result["id"] = json!(id);
                result["created_at"] = row.get("t_create_time").cloned().unwrap_or(Value::Null);
                result
            }
            BackupRead::DeletePreview(selection) => {
                if !selection.filtered() {
                    self.backup_preview(&selection.ids)?
                } else {
                    let rows = self.cloud_rows()?;
                    let ids = selection.select(&rows)?;
                    if ids.is_empty() {
                        json!({"items":[],"deletable":false,"references_complete":true,"matched":0})
                    } else {
                        let mut result = self.preview_rows(&ids, &rows)?;
                        result["matched"] = json!(ids.len());
                        result
                    }
                }
            }
        };
        Ok(output::public(result, &[]))
    }
    pub(super) fn backup_references(&mut self) -> Result<BTreeMap<u64, Vec<Value>>> {
        let mut references: BTreeMap<u64, Vec<Value>> = BTreeMap::new();
        for row in self.author_rows(Resource::Config)? {
            let id = wire_id(pick(&row, &["t_id", "id"]))?;
            let mut cloud = pick(&row, &["t_cloudblackid", "cloud_id"]);
            if cloud.is_null() {
                cloud = pick(
                    &self.raw(Resource::Config, id)?,
                    &["t_cloudblackid", "cloud_id"],
                );
            }
            let cloud = wire_id(cloud).map_err(|_| {
                remote(
                    "configuration backup association is unknown; deletion coverage is incomplete",
                )
            })?;
            references
                .entry(cloud)
                .or_default()
                .push(json!({"id":id,"title":pick(&row,&["t_title","title"])}));
        }
        Ok(references)
    }
    pub(super) fn backup_preview(&mut self, ids: &[NonZeroU64]) -> Result<Value> {
        admit_ids(ids)?;
        let rows = self.cloud_rows()?;
        self.preview_rows(ids, &rows)
    }
    fn preview_rows(&mut self, ids: &[NonZeroU64], rows: &[Value]) -> Result<Value> {
        let references = self.backup_references()?;
        let mut items = Vec::new();
        for id in ids {
            let row = rows
                .iter()
                .find(|row| wire_uint(row["t_id"].clone(), 0).is_ok_and(|value| value == id.get()));
            let linked = references.get(&id.get()).cloned().unwrap_or_default();
            items.push(json!({"id":id,"name":row.and_then(|value|value.get("t_name")),"present":row.is_some(),"references":linked,"deletable":row.is_some()&&linked.is_empty(),"reason":if !linked.is_empty(){Some("configuration_share_reference")}else if row.is_none(){Some("not_found")}else{None}}));
        }
        Ok(
            json!({"deletable":items.iter().all(|row|row["deletable"]==true),"items":items,"references_complete":true}),
        )
    }
    pub fn rename_backup(&mut self, request: &BackupRename) -> Result<Value> {
        let id = request.id.get();
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "backup",
            id,
            "rename",
            &json!({"id":id,"name":request.name}),
            &self.control,
        )?;
        let recovered = mutation.pending();
        let rows = self.cloud_rows().map_err(|error| {
            if recovered {
                mutation.readback_error("backup rename recovery needs a complete cloud list", error)
            } else {
                error
            }
        })?;
        let row = rows
            .into_iter()
            .find(|row| wire_uint(row["t_id"].clone(), 0).is_ok_and(|value| value == id))
            .ok_or_else(|| {
                if recovered {
                    mutation.pending_error("original backup rename target is absent from readback")
                } else {
                    ToolkitError::new(
                        "publish.backup_not_found",
                        "backup rename target was not found",
                        5,
                    )
                }
            })?;
        if row["t_name"] == request.name {
            if recovered {
                mutation.verified()?;
            }
            return Ok(output::public(
                json!({"id":id,"recovered":recovered,"author_writes":0,"verification":"readback","readback":summary(&row)?}),
                &[],
            ));
        }
        if recovered {
            return Err(mutation.pending_error(
                "the original backup name is not present; rename was not repeated",
            ));
        }
        mutation.prepare(&self.control)?;
        self.backup_post(
            "/share/update",
            &json!({"t_id":id,"t_name":request.name}),
            true,
        )
        .map_err(|error| mutation.failure(error))?;
        mutation.acknowledged().map_err(|_| {
            mutation.pending_error("backup rename was accepted; finish saving its receipt")
        })?;
        let rows = self
            .cloud_rows()
            .map_err(|error| mutation.readback_error("backup rename readback is pending", error))?;
        let row = rows
            .iter()
            .find(|row| {
                wire_uint(row["t_id"].clone(), 0).is_ok_and(|value| value == id)
                    && row["t_name"] == request.name
            })
            .ok_or_else(|| mutation.pending_error("backup rename is not present in readback"))?;
        mutation.verified()?;
        Ok(output::public(
            json!({"id":id,"recovered":false,"author_writes":1,"verification":"readback","readback":summary(row)?}),
            &[],
        ))
    }
    pub fn delete_backups(&mut self, request: &BackupDelete) -> Result<Value> {
        let preview = self.backup_preview(&request.ids)?;
        // Check all selected references before the first deletion. Missing
        // rows are admitted only by this client's original pending/completed receipt.
        for row in preview["items"].as_array().unwrap() {
            let id = row["id"].as_u64().unwrap();
            let mutation = Mutation::open(
                &self.home,
                Provider::Newbee,
                "backup",
                id,
                "delete",
                &json!({"id":id,"confirm_delete":true}),
                &self.control,
            )?;
            if !row["references"].as_array().unwrap().is_empty()
                || (row["present"] != true
                    && !mutation.pending()
                    && !mutation
                        .evidence()
                        .is_some_and(|value| value["deleted"] == true))
            {
                return Err(ToolkitError::new(
                    "publish.backup_delete_blocked",
                    "cloud backup deletion is blocked",
                    5,
                )
                .with_details(output::public(preview.clone(), &[])));
            }
        }
        let mut completed = Vec::new();
        let result = (|| {
            for id in &request.ids {
                let id = id.get();
                let mut mutation = Mutation::open(
                    &self.home,
                    Provider::Newbee,
                    "backup",
                    id,
                    "delete",
                    &json!({"id":id,"confirm_delete":true}),
                    &self.control,
                )?;
                let recovered = mutation.pending();
                let fresh = self.backup_preview(&[NonZeroU64::new(id).unwrap()])?;
                let row = &fresh["items"][0];
                if row["present"] != true
                    && (recovered
                        || mutation
                            .evidence()
                            .is_some_and(|value| value["deleted"] == true))
                {
                    let result =
                        json!({"id":id,"deleted":true,"recovered":recovered,"author_writes":0});
                    if recovered {
                        mutation.verified_with(result.clone())?;
                    }
                    completed.push(result);
                    continue;
                }
                if recovered {
                    return Err(mutation.pending_error(
                        "the original backup remains present; deletion was not repeated",
                    ));
                }
                if fresh["deletable"] != true {
                    return Err(ToolkitError::new(
                        "publish.backup_delete_blocked",
                        "backup gained a reference or disappeared before deletion",
                        5,
                    )
                    .with_details(output::public(fresh, &[])));
                }
                mutation.prepare(&self.control)?;
                self.backup_post("/v3/share/delete", &json!({"t_id":id}), true)
                    .map_err(|error| mutation.failure(error))?;
                mutation.acknowledged().map_err(|_| {
                    mutation
                        .pending_error("backup deletion was accepted; finish saving its receipt")
                })?;
                if self
                    .cloud_rows()
                    .map_err(|error| {
                        mutation.readback_error("backup deletion readback is pending", error)
                    })?
                    .iter()
                    .any(|row| wire_uint(row["t_id"].clone(), 0).is_ok_and(|value| value == id))
                {
                    return Err(mutation.pending_error("deleted backup remains in readback"));
                }
                let result = json!({"id":id,"deleted":true,"recovered":false,"author_writes":1});
                mutation.verified_with(result.clone())?;
                completed.push(result);
            }
            Ok(json!({"deleted":completed.len(),"items":completed}))
        })();
        result.map_err(|mut error| {
            error.details["completed"] = json!(completed);
            error
        })
    }
}
pub(super) fn summary(row: &Value) -> Result<Value> {
    Ok(
        json!({"id":wire_id(row["t_id"].clone())?,"cloud_id":wire_id(row["t_id"].clone())?,"name":row.get("t_name"),"game_version_id":row.get("t_Versionid"),"created_at":row.get("t_create_time"),"known_plugins":optional_array(row["t_Known_plug"].clone())?.len(),"unknown_plugins":optional_array(row["t_unKnown_list"].clone())?.len(),"materials":optional_array(row["t_material_list"].clone())?.len(),"fonts":optional_array(row["t_font_list"].clone())?.len()}),
    )
}

#[cfg(test)]
mod tests;
