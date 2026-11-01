//! Shared DD editor controls and their scalar wire representation.
use super::{
    ObjectUpload, Read, Resource,
    read::{self, Attached},
    upload,
};
use crate::{input::invalid, output};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{collections::BTreeSet, num::NonZeroU64};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteAction {
    Create,
    Update,
    Edit,
}
impl WriteAction {
    pub fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Edit => "edit",
        }
    }
}
pub(super) trait Api: read::Api {
    fn submit(&mut self, path: &str, body: &Value) -> Result<Value>;
    fn upload(&mut self, selection: &ObjectUpload) -> Result<Value>;
    fn channels(&mut self) -> Result<Value>;
}
impl Api for Attached<'_> {
    fn submit(&mut self, path: &str, body: &Value) -> Result<Value> {
        self.client.post(path, &[], body, true, self.control)
    }
    fn upload(&mut self, selection: &ObjectUpload) -> Result<Value> {
        selection.execute(self.client, self.control)
    }
    fn channels(&mut self) -> Result<Value> {
        self.client.channels(self.control)
    }
}
pub(super) fn data(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_form", message, 5)
}
pub(super) fn truth(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Number(n) => n.as_i64().map_or_else(|| n.as_u64() != Some(0), |v| v != 0),
    }
}
pub(super) fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) if n.is_u64() || n.is_i64() => Some(n.to_string()),
        _ => None,
    }
}
pub(super) fn first<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| value.get(*key).filter(|v| !v.is_null()))
}
pub(super) fn candidate<'a>(value: &'a Value, aliases: &[&str]) -> Option<&'a Value> {
    if value.is_object() {
        first(value, aliases)
    } else {
        Some(value)
    }
}
pub(super) fn integer(value: &Value, aliases: &[&str]) -> Result<Value> {
    let value =
        candidate(value, aliases).ok_or_else(|| data("DD editor integer projection is missing"))?;
    if value.is_u64() || value.is_i64() {
        return Ok(value.clone());
    }
    let value = value
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| data("DD editor projection contains an invalid integer"))?;
    Ok(json!(value))
}
pub(super) fn remote_array(
    value: Option<&Value>,
    aliases: &[&str],
    integers: bool,
    allow_int_text: bool,
) -> Result<Value> {
    let rows = match value {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(a)) => a,
        _ => return Err(data("DD editor projection is not an array")),
    };
    let mut result = Vec::new();
    for row in rows {
        let value = candidate(row, aliases)
            .ok_or_else(|| data("DD editor array item has no scalar identity"))?;
        if integers {
            result.push(integer(value, &[])?);
        } else if let Some(s) = value.as_str().filter(|s| !s.is_empty()) {
            result.push(json!(s));
        } else if allow_int_text && (value.is_i64() || value.is_u64()) {
            result.push(json!(value.to_string()));
        } else {
            return Err(data("DD editor array item has an invalid scalar type"));
        }
    }
    Ok(json!(result))
}
pub(super) fn game(value: &Value) -> Result<NonZeroU64> {
    let v = value
        .get("game_type")
        .filter(|v| truth(v))
        .or_else(|| value.get("game_types")?.as_array()?.first())
        .ok_or_else(|| data("DD editor omitted its game type"))?;
    let n = integer(v, &["game_type", "id", "value"])?;
    n.as_u64()
        .and_then(NonZeroU64::new)
        .ok_or_else(|| data("DD editor game type is not positive"))
}
pub(super) fn reference(value: &Value) -> Result<String> {
    let mut selected = None;
    for key in ["sn", "share_sn"] {
        if let Some(v) = value
            .get(key)
            .filter(|v| !v.is_null() && v.as_str() != Some(""))
        {
            let id = scalar(v)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| data("DD author row has an invalid SN"))?;
            super::Reference::parse(&id).map_err(|_| data("DD author row has an invalid SN"))?;
            if selected.as_ref().is_some_and(|old| old != &id) {
                return Err(data("DD author row has contradictory resource SNs"));
            }
            selected = Some(id);
        }
    }
    selected.ok_or_else(|| data("DD author row omitted its resource SN"))
}
pub(super) fn target(author: &str, resource: Resource, reference: &str) -> Result<u64> {
    let hash = storage::sha256(&storage::identity_json(
        &json!({"author":author,"resource":resource,"sn":reference}),
    )?);
    Ok(u64::from_str_radix(&hash[..16], 16).unwrap().max(1))
}
pub(super) fn permanent(value: &str, path: &str) -> Result<()> {
    upload::permanent(value).map_err(|_| invalid(path,"a permanent HTTPS artifact URL without credentials or temporary authorization is required"))
}
pub(super) fn admit_controls(doc: &Map<String, Value>) -> Result<()> {
    if doc.get("need_buy") == Some(&Value::Bool(true)) {
        for key in ["price_fen", "buy_life_type"] {
            if doc
                .get(key)
                .is_none_or(|v| v.is_null() || v.as_str() == Some(""))
            {
                return Err(invalid(
                    &format!("$.{key}"),
                    "explicit need_buy=true requires price_fen and buy_life_type in this input",
                ));
            }
        }
    }
    if doc.get("jump_room") == Some(&Value::Bool(true)) {
        if !doc.get("room_id").is_some_and(truth) {
            return Err(invalid(
                "$.room_id",
                "explicit jump_room=true requires room_id",
            ));
        }
        if doc.get("channel_id").is_some_and(truth) != doc.get("channel_type").is_some_and(truth) {
            return Err(invalid(
                "$.channel_id",
                "channel_id and channel_type must both be empty or both be nonempty",
            ));
        }
    }
    if doc.get("sync_room") == Some(&Value::Bool(true))
        && (doc.get("jump_room") == Some(&Value::Bool(false))
            || doc.get("scope").and_then(Value::as_str) == Some("private"))
    {
        return Err(invalid(
            "$.sync_room",
            "sync_room=true requires an enabled room and public scope",
        ));
    }
    if doc.get("need_anchor_vip") == Some(&Value::Bool(true))
        && doc.get("scope").and_then(Value::as_str) == Some("private")
    {
        return Err(invalid(
            "$.need_anchor_vip",
            "need_anchor_vip=true requires public scope",
        ));
    }
    if doc.get("with_associate") == Some(&Value::Bool(true))
        && !doc.get("associated_acts").is_some_and(truth)
    {
        return Err(invalid(
            "$.associated_acts",
            "explicit with_associate=true requires selected associations in this input",
        ));
    }
    for key in ["share_code_life_type", "buy_life_type"] {
        if let Some(value) = doc.get(key) {
            if !life(value) {
                return Err(invalid(
                    &format!("$.{key}"),
                    "select a DD life-types enum value",
                ));
            }
        }
    }
    if let Some(value) = doc.get("price_fen") {
        let n = value.as_u64().ok_or_else(|| {
            invalid(
                "$.price_fen",
                "price_fen must be zero or an exact integer from 10 to 20000",
            )
        })?;
        if n != 0 && !(10..=20000).contains(&n) {
            return Err(invalid(
                "$.price_fen",
                "price_fen must be zero or an exact integer from 10 to 20000",
            ));
        }
    }
    if let Some(rows) = doc.get("vip_levels") {
        if !rows
            .as_array()
            .is_some_and(|rows| rows.iter().all(Value::is_u64))
        {
            return Err(invalid(
                "$.vip_levels",
                "VIP levels must be scalar nonnegative integer IDs",
            ));
        }
    }
    if let Some(rows) = doc.get("associated_acts") {
        let rows = rows
            .as_array()
            .ok_or_else(|| invalid("$.associated_acts", "expected an array"))?;
        let mut seen = BTreeSet::new();
        for row in rows {
            let Some(object) = row.as_object() else {
                return Err(invalid(
                    "$.associated_acts",
                    "each association must contain only sn and act_type",
                ));
            };
            let sn = row.get("sn").and_then(Value::as_str).ok_or_else(|| {
                invalid(
                    "$.associated_acts",
                    "each association requires an opaque sn",
                )
            })?;
            super::Reference::parse(sn)
                .map_err(|_| invalid("$.associated_acts", "invalid association SN"))?;
            if object.len() != 2
                || !matches!(row["act_type"].as_str(), Some("addon" | "share" | "wa"))
                || !seen.insert((row["act_type"].as_str().unwrap(), sn))
            {
                return Err(invalid(
                    "$.associated_acts",
                    "use distinct {sn,act_type} objects with addon, share or wa namespaces",
                ));
            }
        }
    }
    Ok(())
}
pub(super) fn admit_create(resource: Resource, doc: &Map<String, Value>) -> Result<()> {
    if doc.get("scope").and_then(Value::as_str) == Some("private")
        && !doc.get("share_code_life_type").is_some_and(truth)
        && !(resource == Resource::Config && doc.get("need_buy") == Some(&Value::Bool(true)))
    {
        return Err(invalid(
            "$.share_code_life_type",
            "private creation requires an explicit share-code lifetime",
        ));
    }
    if resource == Resource::Wa && doc.get("with_file") == Some(&Value::Bool(true)) {
        for key in ["file", "file_install_path"] {
            if !doc.get(key).is_some_and(truth) {
                return Err(invalid(
                    &format!("$.{key}"),
                    "WA creation with_file=true requires an explicit file and install path",
                ));
            }
        }
    }
    Ok(())
}
pub(super) fn schema_conditions(schema: &mut Value, resource: Resource, action: WriteAction) {
    let p = schema["properties"].as_object().unwrap();
    let mut conditions = Vec::new();
    if p.contains_key("need_buy") {
        conditions.push(json!({"if":{"required":["need_buy"],"properties":{"need_buy":{"const":true}}},"then":{"required":["price_fen","buy_life_type"]}}));
    }
    if p.contains_key("jump_room") {
        conditions.push(json!({"if":{"required":["jump_room"],"properties":{"jump_room":{"const":true}}},"then":{"required":["room_id"],"properties":{"room_id":{"minLength":1}},"anyOf":[{"properties":{"channel_id":{"maxLength":0},"channel_type":{"maxLength":0}}},{"required":["channel_id","channel_type"],"properties":{"channel_id":{"minLength":1},"channel_type":{"minLength":1}}}]}}));
    }
    if p.contains_key("sync_room") {
        conditions.push(json!({"if":{"required":["sync_room"],"properties":{"sync_room":{"const":true}}},"then":{"properties":{"scope":{"const":"public"},"jump_room":{"const":true}}}}));
    }
    if p.contains_key("need_anchor_vip") {
        conditions.push(json!({"if":{"required":["need_anchor_vip"],"properties":{"need_anchor_vip":{"const":true}}},"then":{"properties":{"scope":{"const":"public"}}}}));
    }
    if p.contains_key("with_associate") {
        conditions.push(json!({"if":{"required":["with_associate"],"properties":{"with_associate":{"const":true}}},"then":{"required":["associated_acts"],"properties":{"associated_acts":{"minItems":1}}}}));
    }
    if action == WriteAction::Create && resource != Resource::Config {
        conditions.push(json!({"if":{"required":["scope"],"properties":{"scope":{"const":"private"}}},"then":{"required":["share_code_life_type"]}}));
    }
    if let Some(Value::Array(existing)) = schema.get_mut("allOf") {
        existing.extend(conditions);
    } else {
        schema["allOf"] = json!(conditions);
    }
}
fn life(v: &Value) -> bool {
    matches!(
        v.as_str(),
        Some("seven_day" | "fourteen_day" | "thirty_day" | "sixty_day" | "ninety_day" | "forever")
    )
}
pub(super) fn normalize(
    form: &mut Map<String, Value>,
    resource: Resource,
    create: bool,
) -> Result<()> {
    let get = |f: &Map<String, Value>, key: &str| f.get(key).cloned().unwrap_or(Value::Null);
    if resource == Resource::Config {
        form.insert(
            "need_buy".into(),
            json!(u8::from(truth(&get(form, "need_buy")))),
        );
    }
    if (resource == Resource::Config || create) && !truth(&get(form, "buy_life_type")) {
        form.insert("buy_life_type".into(), json!("seven_day"));
    }
    if form.get("price_fen").is_none_or(Value::is_null)
        || (create && !truth(&get(form, "need_buy")))
    {
        form.insert("price_fen".into(), json!(0));
    }
    if get(form, "scope") == "private" {
        form.insert("sync_room".into(), json!(false));
        form.insert("need_anchor_vip".into(), json!(false));
        form.insert("vip_levels".into(), json!([]));
    } else if get(form, "scope") == "public" {
        if resource == Resource::Config {
            form.remove("share_code_life_type");
        } else {
            form.insert("share_code_life_type".into(), json!("forever"));
        }
    }
    if !truth(&get(form, "jump_room")) {
        for k in ["room_id", "channel_id", "channel_type"] {
            form.insert(k.into(), json!(""));
        }
        form.insert("sync_room".into(), json!(false));
    }
    if truth(&get(form, "with_associate")) {
        let mut rows = Vec::new();
        let source = get(form, "associated_acts");
        for row in source
            .as_array()
            .ok_or_else(|| data("DD associations are not an array"))?
        {
            let sn = reference(row)?;
            if !matches!(row["act_type"].as_str(), Some("addon" | "share" | "wa")) {
                return Err(data("DD association has an invalid resource namespace"));
            }
            rows.push(json!({"sn":sn,"act_type":row["act_type"]}));
        }
        form.insert("associated_acts".into(), json!(rows));
    } else {
        form.insert("associated_acts".into(), json!([]));
    }
    Ok(())
}
pub(super) fn validate_controls(form: &Value, resource: Resource) -> Result<()> {
    if !matches!(form["scope"].as_str(), Some("public" | "private")) {
        return Err(invalid("$.scope", "scope must be public or private"));
    }
    for key in ["share_code_life_type", "buy_life_type"] {
        if let Some(v) = form.get(key).filter(|v| truth(v)) {
            if !life(v) {
                return Err(invalid(&format!("$.{key}"), "invalid DD lifetime"));
            }
        }
    }
    if form["scope"] == "private"
        && !truth(&form["share_code_life_type"])
        && (resource != Resource::Config || !truth(&form["need_buy"]))
    {
        return Err(invalid(
            "$.share_code_life_type",
            "private publication requires a share-code lifetime",
        ));
    }
    if truth(&form["need_buy"]) {
        let n = form["price_fen"]
            .as_u64()
            .ok_or_else(|| invalid("$.price_fen", "price_fen must be an exact integer"))?;
        if n != 0 && !(10..=20000).contains(&n) {
            return Err(invalid(
                "$.price_fen",
                "price_fen must be zero or 10..20000",
            ));
        }
        if !truth(&form["buy_life_type"]) {
            return Err(invalid(
                "$.buy_life_type",
                "paid publication requires a buy lifetime",
            ));
        }
    }
    if truth(&form["jump_room"]) && !truth(&form["room_id"]) {
        return Err(invalid("$.room_id", "room association requires room_id"));
    }
    if !matches!(
        form["creation_statement"].as_str(),
        Some("original" | "chinesize" | "renovate" | "second")
    ) {
        return Err(invalid(
            "$.creation_statement",
            "select a DD creation statement",
        ));
    }
    if truth(&form["with_associate"]) && !truth(&form["associated_acts"]) {
        return Err(invalid(
            "$.associated_acts",
            "content association requires selected author items",
        ));
    }
    Ok(())
}
pub(super) fn locked_usage(current: &Value, form: &Value, doc: &Value) -> Result<()> {
    if ["need_buy", "need_anchor_vip", "scope"]
        .iter()
        .any(|k| doc.get(*k).is_some())
        && (truth(&current["need_buy"]) || truth(&current["need_anchor_vip"]))
            != (truth(&form["need_buy"]) || truth(&form["need_anchor_vip"]))
    {
        return Err(invalid(
            "$.need_buy",
            "the outer free/paid usage mode is locked after creation",
        ));
    }
    Ok(())
}
pub(super) fn choices(rows: &Value, selected: &[Value], keys: &[&str], path: &str) -> Result<()> {
    let available = rows["items"]
        .as_array()
        .ok_or_else(|| data("DD options omitted selectable items"))?
        .iter()
        .filter_map(|r| candidate(r, keys).and_then(scalar))
        .collect::<BTreeSet<_>>();
    if !selected.is_empty() && available.is_empty() {
        return Err(data("DD live options contain no selectable values"));
    }
    if selected
        .iter()
        .any(|v| scalar(v).is_none_or(|v| !available.contains(&v)))
    {
        return Err(invalid(
            path,
            "selection contains an unavailable live option",
        ));
    }
    Ok(())
}
pub(super) fn optional_choices(
    api: &mut impl Api,
    form: &Value,
    current: Option<&Value>,
    game: NonZeroU64,
    control: &OperationControl,
) -> Result<()> {
    if truth(&form["need_anchor_vip"]) {
        let rows = Read::VipLevels.execute(api, control)?;
        choices(
            &rows,
            form["vip_levels"]
                .as_array()
                .ok_or_else(|| data("DD VIP levels are not an array"))?,
            &["id", "level", "value"],
            "$.vip_levels",
        )?;
    }
    let selection = |v: &Value| {
        ["room_id", "channel_id", "channel_type"].map(|k| scalar(&v[k]).unwrap_or_default())
    };
    if truth(&form["jump_room"])
        && current.is_none_or(|c| !truth(&c["jump_room"]) || selection(c) != selection(form))
    {
        let rows = api.channels()?;
        let available = rows["items"]
            .as_array()
            .ok_or_else(|| data("DD channel choices are not an array"))?;
        if available.is_empty() {
            return Err(data("DD channels contain no selectable values"));
        }
        if !available.iter().any(|r| selection(r) == selection(form)) {
            return Err(invalid(
                "$.channel_id",
                "room/channel selection is unavailable",
            ));
        }
    }
    if truth(&form["with_associate"]) {
        let rows = Read::AssociatedActs(game).execute(api, control)?;
        let available = rows["items"].as_array().unwrap();
        for row in form["associated_acts"]
            .as_array()
            .ok_or_else(|| data("DD associations are not an array"))?
        {
            if !available
                .iter()
                .any(|r| r["sn"] == row["sn"] && r["act_type"] == row["act_type"])
            {
                return Err(invalid(
                    "$.associated_acts",
                    "association is absent from the complete author lists",
                ));
            }
        }
    }
    Ok(())
}
/// Compare scalar numeric IDs as text and arrays as multisets, as the DD
/// editors do. Booleans remain booleans; no float conversion is involved.
fn normalized(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.to_string()),
        Value::Array(rows) => {
            let mut r = rows.iter().map(normalized).collect::<Vec<_>>();
            r.sort_by_cached_key(Value::to_string);
            json!(r)
        }
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), normalized(v))).collect())
        }
        _ => v.clone(),
    }
}
pub(super) fn digest(value: &Value) -> Result<String> {
    Ok(storage::sha256(&storage::identity_json(&normalized(
        value,
    ))?))
}
pub(super) fn public(value: Value) -> Value {
    output::public(value, &[])
}
