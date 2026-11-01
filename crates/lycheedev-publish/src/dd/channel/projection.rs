use super::{Choice, Choices, dependency, invalid};
use lycheedev_core::Result;
use serde_json::Value;
use std::collections::BTreeSet;

fn text(value: Option<&Value>) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok((!value.is_empty()).then(|| value.clone())),
        Some(Value::Number(value)) => Ok(Some(value.to_string())),
        _ => Err(invalid()),
    }
}
fn alias(value: &Value, keys: &[&str]) -> Result<Option<String>> {
    for key in keys {
        if let Some(value) = text(value.get(*key))? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
pub(super) fn choices(payload: &Value) -> Result<Choices> {
    let raw = payload.get("data").ok_or_else(invalid)?;
    let rows = if let Some(rows) = raw.as_array() {
        rows
    } else {
        let mut selected = None;
        if !raw.is_object() {
            return Err(invalid());
        }
        for key in ["list", "items", "channels", "data"] {
            if let Some(value) = raw.get(key) {
                let rows = value.as_array().ok_or_else(invalid)?;
                if selected.is_none() || !rows.is_empty() {
                    selected = Some(rows);
                }
                if !rows.is_empty() {
                    break;
                }
            }
        }
        selected.ok_or_else(invalid)?
    };
    let mut items = Vec::new();
    walk(rows, "", None, 0, &mut BTreeSet::new(), &mut items)?;
    let result = Choices {
        total: items.len(),
        items,
        dependencies: dependency(),
    };
    result.validate()?;
    Ok(result)
}
fn walk(
    rows: &[Value],
    inherited_room: &str,
    inherited_name: Option<&str>,
    depth: usize,
    seen: &mut BTreeSet<(String, String, String)>,
    items: &mut Vec<Choice>,
) -> Result<()> {
    if depth >= 64 {
        return Err(invalid());
    }
    for row in rows {
        if !row.is_object() {
            return Err(invalid());
        }
        let explicit_room = alias(row, &["teamId", "team_id", "room_id"])?;
        let room = explicit_room
            .clone()
            .unwrap_or_else(|| inherited_room.into());
        let room_name = alias(row, &["teamName", "team_name", "room_name"])?
            .or_else(|| inherited_name.map(str::to_owned));
        let channel = alias(row, &["channelId", "channel_id"])?.unwrap_or_default();
        let channel_type = alias(row, &["channelType", "channel_type"])?.unwrap_or_default();
        if !room.is_empty()
            && (explicit_room.is_some() || !channel.is_empty())
            && seen.insert((room.clone(), channel.clone(), channel_type.clone()))
        {
            let channel_name = alias(
                row,
                if channel.is_empty() {
                    &["channelName", "channel_name"]
                } else {
                    &["channelName", "channel_name", "name"]
                },
            )?;
            items.push(Choice {
                room_id: room.clone(),
                room_name: room_name.clone(),
                channel_id: channel,
                channel_name,
                channel_type,
            });
            if items.len() > 10000 {
                return Err(invalid());
            }
        }
        for key in ["channelList", "channels", "children", "items"] {
            if let Some(value) = row.get(key).filter(|v| !v.is_null()) {
                walk(
                    value.as_array().ok_or_else(invalid)?,
                    &room,
                    room_name.as_deref(),
                    depth + 1,
                    seen,
                    items,
                )?;
            }
        }
    }
    Ok(())
}
