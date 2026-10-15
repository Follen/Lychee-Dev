//! Item, decor and creature navigation share one pinned multi-table request.
//! These frozen Go navigation contracts have no cursor.
mod creature;
mod encounter;
mod item_geosets;
mod item_models;
mod item_textures;
mod spell;
use super::{Row, identifier, integer, matches};
use crate::{
    capture::GoCaptureFormatter,
    db2, fault, installed,
    resource::{Budget, Cost, RetainedReservation},
    table,
};
use lycheedev_core::{
    Invocation, Outcome, Result,
    evidence::{self, CaptureInput},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_DECOR: usize = 5000;
const MAX_RELATIONS: usize = 65536;
const CAPTURE_BYTES: usize = 16 << 20;

/// Serialize directly into an admitted, bounded buffer. In particular an
/// escaped string must not allocate its full expanded JSON before rejection.
struct CaptureWriter<'a> {
    bytes: Vec<u8>,
    allocation: Option<RetainedReservation<'a>>,
    budget: &'a Budget,
    error: Option<lycheedev_core::ToolkitError>,
}
impl CaptureWriter<'_> {
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.budget.check()?;
        let end = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|n| *n <= CAPTURE_BYTES)
            .ok_or_else(|| fault("query.budget_exceeded", 3, "relational.budget_exceeded"))?;
        self.budget
            .charge(Cost::work(bytes.len().saturating_add(1)))?;
        if end > self.bytes.capacity() {
            let capacity = end
                .max(self.bytes.capacity().saturating_mul(2))
                .clamp(4096, CAPTURE_BYTES);
            // A realloc may retain the old buffer until its replacement exists.
            let allocation = self.budget.reserve_retained(capacity)?;
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| {
                    fault(
                        "records.query_resource_budget",
                        3,
                        "Evidence buffer allocation failed",
                    )
                })?;
            self.allocation = Some(allocation);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}
impl Write for CaptureWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Err(error) = self.append(bytes) {
            self.error = Some(error);
            return Err(io::Error::other("Bounded evidence serialization stopped"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn capture_bytes(value: &Value, budget: &Budget) -> Result<Vec<u8>> {
    let mut writer = CaptureWriter {
        bytes: Vec::new(),
        allocation: None,
        budget,
        error: None,
    };
    let mut serializer = serde_json::Serializer::with_formatter(&mut writer, GoCaptureFormatter);
    if let Err(error) = value.serialize(&mut serializer) {
        return Err(writer.error.unwrap_or_else(|| error.into()));
    }
    budget.check()?;
    if let Some(allocation) = writer.allocation.take() {
        allocation.commit();
    }
    Ok(writer.bytes)
}

enum Request {
    SpellInfo { spell: u32, depth: u32 },
    Item(u32),
    ItemModels { item: u32, race: i64, gender: i64 },
    ItemGeosets(u32),
    ItemTextures(u32),
    DecorList(usize),
    DecorGet { field: &'static str, wanted: u32 },
    CreatureDisplay { display: u32, file: u32 },
    CreatureModel(u32),
}
impl Request {
    fn parse(inv: &Invocation) -> Result<Self> {
        match inv.command.as_str() {
            "data spell info" => {
                let spell = identifier(inv, "--spell-id", true)?;
                if spell == 0 {
                    return Err(fault(
                        "command.invalid_arguments",
                        2,
                        "data spell info requires --spell-id",
                    ));
                }
                let depth = inv
                    .flag("--max-depth")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .map_err(|_| fault("command.invalid_arguments", 2, "invalid --max-depth"))?;
                if depth < 0 {
                    return Err(fault("command.invalid_arguments", 2, "invalid --max-depth"));
                }
                if depth > 32 {
                    return Err(fault("records.query_limit", 2, "records.query_limit"));
                }
                Ok(Self::SpellInfo {
                    spell,
                    depth: if depth == 0 { 5 } else { depth as u32 },
                })
            }
            "data creature display" => {
                let display = identifier(inv, "--display-id", true)?;
                let file = identifier(inv, "--file-data-id", true)?;
                if (display == 0) == (file == 0) {
                    return Err(fault(
                        "records.request_conflict",
                        2,
                        "records.request_conflict",
                    ));
                }
                Ok(Self::CreatureDisplay { display, file })
            }
            "data creature model" => {
                if inv.flag("--file-data-id").is_none() {
                    return Err(fault(
                        "command.invalid_arguments",
                        2,
                        "data creature model requires --file-data-id",
                    ));
                }
                let file = identifier(inv, "--file-data-id", true)?;
                if file == 0 {
                    return Err(fault(
                        "records.request_conflict",
                        2,
                        "records.request_conflict",
                    ));
                }
                Ok(Self::CreatureModel(file))
            }
            "data item get" => Ok(Self::Item(identifier(inv, "--item-id", false)?)),
            "data item geosets" => Ok(Self::ItemGeosets(identifier(inv, "--item-id", false)?)),
            "data item textures" => Ok(Self::ItemTextures(identifier(inv, "--item-id", false)?)),
            "data item models" => {
                let item = identifier(inv, "--item-id", false)?;
                let signed = |flag| {
                    inv.flag(flag).unwrap_or("0").parse::<i64>().map_err(|_| {
                        fault("command.invalid_arguments", 2, format!("invalid {flag}"))
                    })
                };
                Ok(Self::ItemModels {
                    item,
                    race: signed("--race-id")?,
                    gender: signed("--gender")?,
                })
            }
            "data decor list" => {
                let text = inv.flag("--limit").ok_or_else(|| {
                    fault(
                        "command.invalid_arguments",
                        2,
                        "data decor list requires --limit",
                    )
                })?;
                let limit = text
                    .parse::<i64>()
                    .map_err(|_| fault("command.invalid_arguments", 2, "invalid --limit"))?;
                if limit <= 0 {
                    return Err(fault("records.limit_required", 2, "records.limit_required"));
                }
                if limit > MAX_DECOR as i64 {
                    return Err(fault(
                        "records.query_limit",
                        2,
                        format!("records.query_limit: {limit} > {MAX_DECOR}"),
                    ));
                }
                Ok(Self::DecorList(limit as usize))
            }
            "data decor get" => {
                let mut key = None;
                for (flag, field) in [
                    ("--id", "ID"),
                    ("--item-id", "ItemID"),
                    ("--model-file-data-id", "ModelFileDataID"),
                ] {
                    let value = identifier(inv, flag, true)?;
                    if value == 0 {
                        continue;
                    }
                    if key.replace((field, value)).is_some() {
                        return Err(fault(
                            "records.request_conflict",
                            2,
                            "records.request_conflict",
                        ));
                    }
                }
                let (field, wanted) = key.ok_or_else(|| {
                    fault("records.request_conflict", 2, "records.request_conflict")
                })?;
                Ok(Self::DecorGet { field, wanted })
            }
            _ => Err(fault(
                "command.unsupported",
                3,
                "Domain profile not implemented",
            )),
        }
    }
    fn tables(&self) -> &'static [&'static str] {
        match self {
            Self::SpellInfo { .. } => spell::TABLES,
            Self::Item(_) => &["Item", "ItemSparse"],
            Self::ItemModels { .. } => item_models::TABLES,
            Self::ItemGeosets(_) => item_geosets::TABLES,
            Self::ItemTextures(_) => item_textures::TABLES,
            Self::CreatureDisplay { .. } | Self::CreatureModel(_) => &[
                "CreatureDisplayInfo",
                "CreatureModelData",
                "CreatureDisplayInfoGeosetData",
            ],
            _ => &["HouseDecor"],
        }
    }
    fn capture(&self) -> (&'static str, String) {
        match self {
            Self::SpellInfo { spell, .. } => ("spell-info", format!("spell:info:{spell}")),
            Self::CreatureDisplay { display, file } => (
                "creature-display",
                if *display != 0 {
                    format!("creature:display:{display}")
                } else {
                    format!("creature:display:file:{file}")
                },
            ),
            Self::CreatureModel(file) => ("creature-model", format!("creature:model:{file}")),
            Self::Item(id) => ("item-get", format!("item:get:{id}")),
            Self::ItemModels { item, .. } => ("item-models", format!("item:models:{item}")),
            Self::ItemGeosets(item) => ("item-geosets", format!("item:geosets:{item}")),
            Self::ItemTextures(item) => ("item-textures", format!("item:textures:{item}")),
            Self::DecorList(_) => ("decor-list", "decor:list".into()),
            Self::DecorGet { field, wanted } => {
                ("decor-get", format!("decor:get:{field}:{wanted}"))
            }
        }
    }
}

struct Opened<'a> {
    prepared: &'a db2::Prepared,
    view: table::Table<'a>,
}
struct Navigator<'a> {
    tables: Vec<Opened<'a>>,
    budget: Arc<Budget>,
    relations: Vec<Value>,
    partial: Vec<&'static str>,
    truncation: Option<&'static str>,
}
impl<'a> Navigator<'a> {
    fn open(prepared: &'a [db2::Prepared], budget: Arc<Budget>) -> Result<Self> {
        let mut nav = Self {
            tables: vec![],
            budget,
            relations: vec![],
            partial: vec![],
            truncation: None,
        };
        for prepared in prepared {
            nav.budget.check()?;
            let view = table::open_budgeted(
                &prepared.file.bytes,
                &prepared.definition,
                Default::default(),
                &Default::default(),
                &[],
                nav.budget.clone(),
            )?;
            // Unavailable rows must not become an absence claim. The current
            // authenticated provider refuses encrypted partial files upstream.
            if view.coverage()["complete"] != true
                || prepared.file.reading["contentVerified"] != true
            {
                nav.partial("encrypted_sections_unavailable");
            }
            nav.tables.push(Opened { prepared, view });
        }
        Ok(nav)
    }
    fn table(&self, name: &str) -> Result<&Opened<'a>> {
        self.tables
            .iter()
            .find(|t| t.prepared.bundle.identity.name == name)
            .ok_or_else(|| fault("records.internal", 8, "Navigator table was not prepared"))
    }
    fn partial(&mut self, reason: &'static str) {
        if !self.partial.contains(&reason) {
            self.partial.push(reason);
        }
    }
    fn truncate(&mut self, reason: &'static str) {
        self.truncation.get_or_insert(reason);
    }
    fn row(&self, name: &str, id: u32) -> Result<Option<Row>> {
        self.budget.check()?;
        match self.table(name)?.view.row(id, 1 << 20) {
            Ok(row) => {
                admit_row(&self.budget, &row)?;
                Ok(Some(row))
            }
            Err(error) if error.code == "table.record_not_found" => Ok(None),
            Err(error) => Err(error),
        }
    }
    fn rows(
        &self,
        name: &str,
        filter: Option<(&str, u32)>,
        limit: usize,
    ) -> Result<(Vec<(u32, Row)>, bool)> {
        let table = self.table(name)?;
        if let Some((field, _)) = filter {
            let definition = table
                .view
                .columns()
                .iter()
                .find(|column| column.name == field)
                .ok_or_else(|| {
                    fault(
                        "records.field_unknown",
                        2,
                        format!("records.field_unknown: {field}"),
                    )
                })?;
            if definition.kind != "int" {
                return Err(fault(
                    "records.field_type_mismatch",
                    2,
                    format!(
                        "records.field_type_mismatch: {field} is {}",
                        definition.kind
                    ),
                ));
            }
        }
        let mut rows = Vec::new();
        for id in table.view.ids() {
            self.budget.charge(Cost::work(1))?;
            // Decode the entire row, including unused fields, as the Go scanner
            // does. One extra matching row proves truncation exactly.
            let row = table.view.row(id, 1 << 20)?;
            if filter.is_some_and(|(field, wanted)| !matches(row.get(field), wanted)) {
                continue;
            }
            if rows.len() == limit {
                return Ok((rows, true));
            }
            admit_row(&self.budget, &row)?;
            rows.push((id, row));
        }
        Ok((rows, false))
    }
    fn link(&mut self, field: &str, id: u32, target: u32) -> Result<()> {
        if self.relations.len() == MAX_RELATIONS {
            self.truncate("relation_budget");
            return Ok(());
        }
        self.budget.charge(Cost::retained(512))?;
        self.relations.push(json!({"fromTable":"HouseDecor","fromField":field,"fromID":id,"toKind":"decor","toID":target,"depth":0}));
        Ok(())
    }
    fn evaluate(&mut self, request: &Request) -> Result<Value> {
        match *request {
            Request::SpellInfo { spell, depth } => self.spell_info(spell, depth),
            Request::CreatureDisplay { display, file } => self.creature_display(display, file),
            Request::CreatureModel(file) => self.creature_model(file),
            Request::ItemModels { item, race, gender } => self.item_models(item, race, gender),
            Request::ItemGeosets(item) => self.item_geosets(item),
            Request::ItemTextures(item) => self.item_textures(item),
            Request::Item(id) => {
                let sparse = self.row("ItemSparse", id)?.ok_or_else(not_found)?;
                let mut item = json!({"id":id,"name":text(&sparse,"Display_lang"),"inventoryType":integer(sparse.get("InventoryType")),"quality":integer(sparse.get("OverallQualityID")),"slotName":slot_name(integer(sparse.get("InventoryType"))),"classID":0,"subclassID":0});
                if let Some(class) = self.row("Item", id)? {
                    item["classID"] = json!(integer(class.get("ClassID")));
                    item["subclassID"] = json!(integer(class.get("SubclassID")));
                } else {
                    self.partial("row_missing");
                }
                Ok(json!({"item":item}))
            }
            Request::DecorList(limit) => {
                let (rows, extra) = self.rows("HouseDecor", None, limit)?;
                if extra {
                    self.truncate("limit_reached");
                }
                let items: Vec<_> = rows.iter().map(|(_, row)| decor(row)).collect();
                Ok(json!({"count":items.len(),"items":items}))
            }
            Request::DecorGet { field, wanted } => {
                let (rows, extra) = self.rows("HouseDecor", Some((field, wanted)), MAX_DECOR)?;
                if extra {
                    self.truncate("row_budget");
                }
                let (_, first) = rows.first().ok_or_else(not_found)?;
                if rows.len() > 1 {
                    self.partial("relation_ambiguous");
                }
                for (id, row) in &rows {
                    self.link(field, *id, unsigned(row.get("ID")))?;
                }
                Ok(json!({"item":decor(first)}))
            }
        }
    }
    fn context(&self) -> Result<Value> {
        let first = self
            .tables
            .first()
            .ok_or_else(|| fault("records.internal", 8, "Empty navigator"))?;
        let pin = &first.prepared.pin;
        let proofs: Vec<_> = self.tables.iter().map(|opened| {
            let t = opened.prepared;
            json!({"contentVerified":t.file.reading["contentVerified"],"keySource":t.file.reading["keySource"],"name":t.bundle.identity.name,"identity":t.bundle.identity,"layoutHash":format!("{:08X}",t.layout.layout_hash),"definitionSHA256":t.definition.sha256,"fileDataID":t.bundle.identity.db2_file_data_id,"contentKey":t.file.reading["entry"]["contentKey"],"logicalRows":opened.view.logical_count()})
        }).collect();
        let verified: Vec<_> = self
            .tables
            .iter()
            .map(|t| &t.prepared.file.reading["verification"])
            .collect();
        Ok(
            json!({"resources":self.budget.snapshot()?,"snapshot":pin.id,"pin":pin.data,"source":first.prepared.file.reading["source"],"tables":proofs,"relations":self.relations,"complete":self.partial.is_empty()&&self.truncation.is_none(),"truncated":self.truncation.is_some(),"truncationReason":self.truncation.unwrap_or("none"),"partial":!self.partial.is_empty(),"partialReasons":if self.partial.is_empty(){Value::Null}else{json!(self.partial)},"tableVerification":verified,"resourceAccounting":{"wholeQueryAccountingComplete":false,"strategy":"conservative cumulative admission","wdcMetadataAdmittedBytes":self.tables.iter().map(|t| t.view.admitted_metadata_bytes()).sum::<u64>(),"remainingGaps":["uninterruptible single codec call"]}}),
        )
    }
}

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

// The file batch verifies the source before/after IO. Recheck after navigation
// too, before publishing any result, so long scans cannot silently cross builds.
pub(super) fn check_source(
    inv: &Invocation,
    prepared: &db2::Prepared,
    budget: &Arc<Budget>,
    remote: Option<&crate::remote::provider::Scope<'_>>,
) -> Result<()> {
    inv.control.check()?;
    budget.check()?;
    if let Some(scope) = remote {
        // Fixed Go readSession.check returns after context checks for CDN, and
        // fileSource has no local verify callback there. Content was already
        // authenticated by the borrowed Session, not by this source selector.
        return scope.check_consumer(inv, budget);
    }
    let pin = prepared.pin.data.as_ref().expect("prepare checked DataPin");
    budget.charge(Cost::metadata(
        inv.required("--installation")?.len()
            + pin.product.len()
            + pin.full_build.len()
            + pin.region.len()
            + pin.language.len()
            + pin.definition_commit.len(),
    ))?;
    let current = installed::resolve_pinned_target_budgeted(
        Path::new(inv.required("--installation")?),
        pin,
        Some(budget),
    )?;
    let proof = &prepared.file.reading;
    if &current.data != pin
        || proof["catalogSHA256"] != current.metadata.catalog_sha256
        || proof["buildConfiguration"]["sha256"] != current.metadata.build_document.sha256
        || proof["cdnConfiguration"]["sha256"] != current.metadata.cdn_document.sha256
    {
        return Err(fault(
            "records.pinned_build_changed",
            4,
            "Local metadata changed during query",
        ));
    }
    budget.check()
}

pub(super) fn dispatch(
    inv: &Invocation,
    after_read: &mut impl FnMut() -> Result<()>,
) -> Result<Outcome> {
    inv.control.check()?;
    if !inv.flag_bool("--cdn") && inv.flag("--installation").is_none_or(str::is_empty) {
        return Err(fault(
            "command.invalid_arguments",
            2,
            format!(
                "{} requires --snapshot, and either --installation or --cdn",
                inv.command
            ),
        ));
    }
    if inv.command == "data encounter get" {
        return encounter::dispatch(inv, after_read);
    }
    let request = Request::parse(inv)?;
    let timeout = inv.usize_flag("--timeout-seconds", 300, 1, 3600)?;
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(timeout as u64),
        inv.control.clone(),
    ));
    let mut remote = if inv.flag_bool("--cdn") {
        Some(crate::remote::provider::OpenedScope::open(inv, &budget)?.admit()?)
    } else {
        None
    };
    let result = (|| {
        let prepared = if matches!(
            request,
            Request::SpellInfo { .. }
                | Request::ItemModels { .. }
                | Request::ItemGeosets(_)
                | Request::ItemTextures(_)
                | Request::CreatureDisplay { .. }
                | Request::CreatureModel(_)
        ) {
            let tables = request.tables();
            let mut next = 1;
            db2::prepare_following_scoped(
                inv,
                &tables[..1],
                &budget,
                remote.as_mut(),
                None,
                &mut |batch| {
                    // Go opens each table completely before resolving the next one.
                    // Keep that failure order in the one shared CASC read session.
                    Navigator::open(batch, budget.clone())?;
                    let names = tables.get(next).copied().into_iter().collect();
                    next += 1;
                    Ok(names)
                },
            )?
        } else {
            db2::prepare_following_scoped(
                inv,
                request.tables(),
                &budget,
                remote.as_mut(),
                None,
                &mut |_| Ok(Vec::new()),
            )?
        };
        let mut nav = Navigator::open(&prepared, budget.clone())?;
        let mut result = nav.evaluate(&request)?;
        result
            .as_object_mut()
            .expect("domain object")
            .extend(nav.context()?.as_object().expect("context object").clone());
        budget.check()?;
        after_read()?;
        check_source(inv, &prepared[0], &budget, remote.as_ref())?;
        let raw = capture_bytes(&result, &budget)?;
        let complete = result["complete"] == true;
        let truncated = result["truncated"] == true;
        let (kind, locator) = request.capture();
        let pin = &prepared[0].pin;
        let capture = evidence::commit_capture_budgeted(
            &inv.home,
            CaptureInput {
                bytes: &raw,
                maximum_bytes: 16 << 20,
                media_type: "application/json",
                provenance: json!({"kind":kind,"locator":locator,"snapshot":pin.id,"dataBuild":pin.data.as_ref().expect("DataPin").full_build}),
                complete,
                truncated,
            },
            &inv.control,
            budget.as_ref(),
        )?;
        let mut outcome = Outcome::new(result);
        outcome.complete = complete;
        outcome.selection = serde_json::to_value(pin)?;
        outcome.provenance = vec![capture];
        if truncated {
            outcome.warnings.push(json!("This result is truncated; preserve the snapshot and the stated truncation reason when continuing."));
        }
        if !nav.partial.is_empty() {
            outcome.warnings.push(json!("This result is only partially covered; inspect partialReasons for the unavailable tables and unresolved relations."));
        }
        Ok(outcome)
    })();
    match remote {
        Some(scope) => scope.finish(result),
        None => result,
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    fn budget(limits: Cost) -> Budget {
        Budget::new(limits, Instant::now() + Duration::from_secs(60), None)
    }

    #[test]
    fn capture_uses_go_html_and_javascript_escaping_before_admission() {
        let ledger = budget(Cost::defaults());
        let raw = capture_bytes(&json!({"<&>":"<>&\u{2028}\u{2029}🙂"}), &ledger).unwrap();
        assert_eq!(
            std::str::from_utf8(&raw).unwrap(),
            r#"{"\u003c\u0026\u003e":"\u003c\u003e\u0026\u2028\u2029🙂"}"#
        );
        let ledger = budget(Cost::defaults());
        let error =
            capture_bytes(&Value::String("<".repeat(CAPTURE_BYTES / 6 + 1)), &ledger).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.message.as_str(), error.exit_code),
            ("query.budget_exceeded", "relational.budget_exceeded", 3)
        );
        assert!(ledger.snapshot().unwrap().retained_bytes <= CAPTURE_BYTES as u64);
    }

    #[test]
    fn capture_writer_accepts_exact_limit_and_rejects_escaped_expansion_without_oversized_buffer() {
        let ledger = budget(Cost::defaults());
        let exact = Value::String("x".repeat(CAPTURE_BYTES - 2));
        assert_eq!(capture_bytes(&exact, &ledger).unwrap().len(), CAPTURE_BYTES);
        let ledger = budget(Cost::defaults());
        let mut writer = CaptureWriter {
            bytes: Vec::new(),
            allocation: None,
            budget: &ledger,
            error: None,
        };
        // Less than 3 MiB of input exceeds 16 MiB once JSON escaping expands it.
        let escaped = Value::String("\0".repeat(CAPTURE_BYTES / 6 + 1));
        assert!(serde_json::to_writer(&mut writer, &escaped).is_err());
        let error = writer.error.as_ref().unwrap();
        assert_eq!(
            (error.code.as_str(), error.exit_code, error.message.as_str()),
            ("query.budget_exceeded", 3, "relational.budget_exceeded")
        );
        assert!(writer.bytes.len() <= CAPTURE_BYTES);
        assert!(writer.bytes.capacity() <= CAPTURE_BYTES);
        assert_eq!(
            ledger.snapshot().unwrap().retained_bytes,
            CAPTURE_BYTES as u64
        );
    }

    #[test]
    fn capture_writer_stops_before_growth_on_resource_failure_and_preserves_cancellation() {
        let ledger = budget(Cost {
            // The replacement fits by itself, but not beside the old buffer.
            retained_bytes: 8192,
            ..Cost::defaults()
        });
        let mut writer = CaptureWriter {
            bytes: Vec::new(),
            allocation: None,
            budget: &ledger,
            error: None,
        };
        writer.write_all(&[0; 4096]).unwrap();
        assert!(writer.write_all(&[0]).is_err());
        assert_eq!(writer.bytes.len(), 4096);
        assert_eq!(writer.bytes.capacity(), 4096);
        assert_eq!(
            writer.error.as_ref().unwrap().code,
            "records.query_resource_budget"
        );
        assert_eq!(ledger.snapshot().unwrap().retained_bytes, 4096);
        drop(writer);
        assert_eq!(ledger.snapshot().unwrap().retained_bytes, 0);
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let ledger = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(10),
            Some(cancel),
        );
        assert_eq!(
            capture_bytes(&json!({"a":1}), &ledger)
                .unwrap_err()
                .exit_code,
            7
        );
    }
}
