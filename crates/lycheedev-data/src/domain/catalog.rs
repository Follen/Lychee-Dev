pub(crate) mod async_access;
pub(crate) mod basic;
// Item, decor and creature navigation share one pinned multi-table request.
// These frozen Go navigation contracts have no cursor.
pub(crate) mod creature;
pub(crate) mod encounter;
pub(crate) mod item_geosets;
pub(crate) mod item_models;
pub(crate) mod item_textures;
pub(crate) mod spell;
use super::{Row, integer};
use crate::{
    fault,
    resource::{Budget, Cost},
    table,
};
use lycheedev_core::Result;
use serde_json::{Value, json};
const MAX_DECOR: usize = 5000;
fn not_found() -> lycheedev_core::ToolkitError {
    fault("records.record_not_found", 3, "records.record_not_found")
}
fn text<'a>(row: &'a Row, name: &str) -> &'a str {
    match row.get(name) {
        Some(table::Value::String(value)) => value,
        _ => "",
    }
}
// rowUint32 in frozen Go permits float32 presentation values. Exact relation
// matching above deliberately does not coerce them into integral identities.
fn unsigned(value: Option<&table::Value>) -> u32 {
    match value {
        Some(table::Value::Float32(v)) if *v > 0.0 && f64::from(*v) <= f64::from(u32::MAX) => {
            *v as u32
        }
        value => super::unsigned(value),
    }
}
fn slot_name(slot: i64) -> &'static str {
    const NAMES: &[&str] = &[
        "None",
        "Head",
        "Neck",
        "Shoulder",
        "Shirt",
        "Chest",
        "Waist",
        "Legs",
        "Feet",
        "Wrist",
        "Hands",
        "Finger",
        "Trinket",
        "One-Hand",
        "Shield",
        "Ranged",
        "Back",
        "Two-Hand",
        "Bag",
        "Tabard",
        "Robe",
        "Main Hand",
        "Off Hand",
        "Held In Off-Hand",
        "Ammo",
        "Thrown",
        "Ranged Right",
        "Quiver",
        "Relic",
    ];
    usize::try_from(slot)
        .ok()
        .and_then(|index| NAMES.get(index))
        .copied()
        .unwrap_or("Unknown")
}
fn decor(row: &Row) -> Value {
    json!({"id":unsigned(row.get("ID")),"name":text(row,"Name_lang"),"modelFileDataID":unsigned(row.get("ModelFileDataID")),"thumbnailFileDataID":unsigned(row.get("ThumbnailFileDataID")),"itemID":unsigned(row.get("ItemID")),"gameObjectID":unsigned(row.get("GameObjectID")),"type":integer(row.get("Type")),"modelType":integer(row.get("ModelType"))})
}
fn admit_row(budget: &Budget, row: &Row) -> Result<()> {
    fn value_cost(value: &table::Value) -> Option<usize> {
        match value {
            table::Value::String(text) => text.len().checked_mul(6)?.checked_add(128),
            table::Value::Array(values) => values
                .iter()
                .try_fold(128usize, |n, v| n.checked_add(value_cost(v)?)),
            _ => Some(128),
        }
    }
    let bytes = row
        .iter()
        .try_fold(256usize, |n, (key, value)| {
            n.checked_add(key.len().checked_mul(6)?)?
                .checked_add(value_cost(value)?)
        })
        .ok_or_else(|| {
            fault(
                "records.query_resource_budget",
                3,
                "Domain row admission overflow",
            )
        })?;
    budget.charge(Cost::retained(bytes))
}
