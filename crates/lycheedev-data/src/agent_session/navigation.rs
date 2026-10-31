//! Recovered typed domain bridge. Every row comes from the unified owned Engine.
use crate::{
    domain::{
        Row,
        catalog::{
            async_access::DomainAccess, basic::BasicAccess, creature::CreatureAccess,
            encounter::EncounterAccess, item_geosets::ItemGeosetAccess,
            item_models::ItemModelAccess, item_textures::ItemTextureAccess, spell::SpellAccess,
        },
    },
    engine::{
        DecorSelector, DomainRequest, DomainResult, Engine, OwnedCapture, OwnedDomainRows,
        OwnedProvenance, SourceMode, TableRequest,
    },
    fault,
    resource::{Budget, Cost},
};
use arrow_array::Array;
use lycheedev_core::{Result, control::OperationControl};
use serde_json::{Value, json};
use std::sync::Arc;
struct Access<'a> {
    engine: &'a mut Engine,
    mode: &'a SourceMode,
    captures: &'a [String],
    control: OperationControl,
    budget: Arc<Budget>,
    sources: Vec<OwnedProvenance>,
    relations: Vec<Value>,
    partial: Vec<&'static str>,
    truncation: Option<&'static str>,
    required_lookup: Option<(&'static str, u32)>,
    required_scan_tables: &'static [&'static str],
}
fn overflow() -> lycheedev_core::ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Domain row admission overflow",
    )
}
impl Access<'_> {
    fn table(&self, name: &str) -> Result<TableRequest> {
        let n = self.captures.iter().try_fold(name.len() + 256, |n, s| {
            n.checked_add(s.len())
                .and_then(|v| v.checked_add(64))
                .ok_or_else(overflow)
        })?;
        self.budget.charge(Cost::retained(n))?;
        Ok(TableRequest {
            name: name.into(),
            mode: self.mode.clone(),
            hotfix_captures: self.captures.to_vec(),
        })
    }
    fn records(&mut self, owner: OwnedDomainRows) -> Result<Vec<(u32, Row)>> {
        if !owner.complete() {
            let encrypted = owner.provenance()["sources"].as_array().is_some_and(|s| {
                s.iter().any(|s| {
                    ["missing", "unavailablePartitions"].iter().any(|k| {
                        s["sourceIdentity"][*k]
                            .as_array()
                            .is_some_and(|v| !v.is_empty())
                    })
                })
            });
            self.mark_partial(if encrypted {
                "encrypted_sections_unavailable"
            } else {
                "source_coverage_incomplete"
            });
        }
        self.budget.charge(Cost::retained(
            owner.row_count().checked_mul(64).ok_or_else(overflow)?,
        ))?;
        let rows = owner.visit_records(&self.control, |schema, batches, ids| {
            if batches.len() != ids.len() {
                return Err(fault(
                    "data.domain.identity",
                    8,
                    "Mismatched physical locator batches",
                ));
            }
            let mut rows = Vec::with_capacity(owner.row_count());
            for (batch, ids) in batches.iter().zip(ids) {
                if batch.num_rows() != ids.len() || batch.num_columns() != schema.fields().len() {
                    return Err(fault(
                        "data.domain.identity",
                        8,
                        "Mismatched physical locator rows",
                    ));
                }
                for i in 0..batch.num_rows() {
                    self.budget.check()?;
                    if ids.is_null(i) {
                        return Err(fault("data.domain.identity", 8, "Null physical locator"));
                    }
                    self.budget.charge(Cost::retained(
                        batch
                            .num_columns()
                            .checked_mul(256)
                            .and_then(|n| n.checked_add(256))
                            .ok_or_else(overflow)?,
                    ))?;
                    let mut row = Row::new();
                    for (field, array) in schema.fields().iter().zip(batch.columns()) {
                        self.budget.charge(Cost::retained(
                            field.name().len().checked_mul(8).ok_or_else(overflow)?,
                        ))?;
                        let value =
                            super::domain::convert_array(array.as_ref(), i, &self.budget, 0)?;
                        if row.insert(field.name().clone(), value).is_some() {
                            return Err(fault("data.domain.value", 8, "Duplicate source column"));
                        }
                    }
                    rows.push((ids.value(i), row));
                }
            }
            Ok(rows)
        })?;
        self.budget.charge(Cost::retained(128))?;
        self.sources.push(owner.owned_provenance()?);
        Ok(rows)
    }
}
impl DomainAccess for Access<'_> {
    async fn lookup(&mut self, name: &str, id: u32) -> Result<Option<Row>> {
        self.budget.charge(Cost::retained(64))?;
        let table = self.table(name)?;
        let owner = self
            .engine
            .lookup_domain_rows(table, vec![id], self.control.clone())
            .await?;
        if owner.unresolved_ids().contains(&id) {
            if self.required_lookup == Some((name, id)) {
                return Err(fault(
                    "data.domain.record_unavailable",
                    3,
                    "Primary record is in unavailable source coverage",
                ));
            }
            self.mark_partial("record_unavailable");
        }
        let mut rows = self.records(owner)?;
        if rows.len() > 1 || rows.first().is_some_and(|(actual, _)| *actual != id) {
            return Err(fault(
                "data.domain.identity",
                8,
                "Physical lookup returned another record",
            ));
        }
        Ok(rows.pop().map(|(_, row)| row))
    }
    async fn scan(
        &mut self,
        name: &str,
        filter: Option<(&str, u32)>,
        limit: usize,
    ) -> Result<(Vec<(u32, Row)>, bool)> {
        let table = self.table(name)?;
        if let Some((f, _)) = filter {
            self.budget.charge(Cost::retained(f.len() + 64))?;
        }
        let filter = filter.map(|(f, id)| (f.to_owned(), id));
        let owner = self
            .engine
            .scan_domain_records(table, filter, limit, self.control.clone())
            .await?;
        if owner.row_count() == 0 && !owner.complete() && self.required_scan_tables.contains(&name)
        {
            return Err(fault(
                "data.domain.record_unavailable",
                3,
                "Selected record cannot be resolved in unavailable source coverage",
            ));
        }
        let extra = owner.extra();
        Ok((self.records(owner)?, extra))
    }
    fn budget(&self) -> &Budget {
        &self.budget
    }
    fn budget_owner(&self) -> Arc<Budget> {
        self.budget.clone()
    }
    fn complete(&self) -> bool {
        self.partial.is_empty() && self.truncation.is_none()
    }
    fn mark_partial(&mut self, r: &'static str) {
        if !self.partial.contains(&r) {
            self.partial.push(r);
        }
    }
    fn mark_truncated(&mut self, r: &'static str) {
        if self.truncation.is_none() {
            self.truncation = Some(r);
        }
    }
    fn link(
        &mut self,
        table: &str,
        field: &str,
        id: u32,
        kind: &str,
        target: u64,
        depth: u32,
    ) -> Result<()> {
        self.budget.check()?;
        if self.relations.len() == 65536 {
            self.mark_truncated("relation_budget");
            return Ok(());
        }
        let bytes = table
            .len()
            .checked_add(field.len())
            .and_then(|n| n.checked_add(kind.len()))
            .and_then(|n| n.checked_add(512))
            .ok_or_else(overflow)?;
        self.budget.charge(Cost::retained(bytes))?;
        self.relations.push(json!({"fromTable":table,"fromField":field,"fromID":id,"toKind":kind,"toID":target,"depth":depth}));
        Ok(())
    }
}
fn positive(id: u32) -> Result<()> {
    if id == 0 {
        Err(fault("records.invalid_id", 2, "A positive ID is required"))
    } else {
        Ok(())
    }
}
fn selector(field: &DecorSelector) -> &'static str {
    match field {
        DecorSelector::Id => "ID",
        DecorSelector::ItemId => "ItemID",
        DecorSelector::ModelFileDataId => "ModelFileDataID",
    }
}
pub(super) async fn execute(
    engine: &mut Engine,
    mut request: DomainRequest,
    control: OperationControl,
) -> Result<(DomainResult, OwnedCapture)> {
    control.check()?;
    match &mut request {
        DomainRequest::SpellInfo { max_depth, .. } => {
            if *max_depth == 0 {
                *max_depth = 5;
            }
            if *max_depth > 32 {
                return Err(fault("records.query_depth", 2, "Depth must be 1..32"));
            }
        }
        DomainRequest::EncounterGet { max_depth, .. } => {
            if *max_depth == 0 {
                *max_depth = 32;
            }
            if *max_depth > 32 {
                return Err(fault("records.query_depth", 2, "Depth must be 1..32"));
            }
        }
        _ => {}
    }
    let (mode, captures) = request.binding();
    super::domain::validate_captures(mode, captures)?;
    let owner = engine.begin_domain_request(&request, control.clone())?;
    let budget = owner.budget();
    budget.charge(Cost::retained(4096))?;
    let required_lookup = match &request {
        DomainRequest::ItemGet { item_id, .. }
        | DomainRequest::ItemModels { item_id, .. }
        | DomainRequest::ItemTextures { item_id, .. }
        | DomainRequest::ItemGeosets { item_id, .. } => Some(("ItemSparse", *item_id)),
        DomainRequest::CreatureDisplay { display_id, .. } if *display_id != 0 => {
            Some(("CreatureDisplayInfo", *display_id))
        }
        _ => None,
    };
    let mut access = Access {
        engine,
        mode,
        captures,
        control: control.clone(),
        budget,
        sources: vec![],
        relations: vec![],
        partial: vec![],
        truncation: None,
        required_lookup,
        required_scan_tables: match &request {
            DomainRequest::DecorGet { .. } => &["HouseDecor"],
            DomainRequest::CreatureDisplay { display_id: 0, .. } => {
                &["CreatureModelData", "CreatureDisplayInfo"]
            }
            _ => &[],
        },
    };
    let mut report = match &request {
        DomainRequest::ItemGet { item_id, .. } => {
            positive(*item_id)?;
            access.item_get(*item_id).await?
        }
        DomainRequest::ItemModels {
            item_id,
            race_id,
            gender,
            ..
        } => {
            positive(*item_id)?;
            access.item_models(*item_id, *race_id, *gender).await?
        }
        DomainRequest::ItemTextures { item_id, .. } => {
            positive(*item_id)?;
            access.item_textures(*item_id).await?
        }
        DomainRequest::ItemGeosets { item_id, .. } => {
            positive(*item_id)?;
            access.item_geosets(*item_id).await?
        }
        DomainRequest::SpellInfo {
            spell_id,
            max_depth,
            ..
        } => {
            positive(*spell_id)?;
            access.spell_info(*spell_id, *max_depth).await?
        }
        DomainRequest::SpellAuras { spell_id, .. } => {
            positive(*spell_id)?;
            crate::domain::effects_domain(&mut access, *spell_id, 0, false).await?
        }
        DomainRequest::SpellSummons {
            spell_id, npc_id, ..
        } => {
            positive(*spell_id)?;
            crate::domain::effects_domain(&mut access, *spell_id, *npc_id, true).await?
        }
        DomainRequest::EncounterGet {
            encounter_id,
            max_depth,
            ..
        } => {
            positive(*encounter_id)?;
            access.encounter_get(*encounter_id, *max_depth).await?
        }
        DomainRequest::DecorList { limit, .. } => {
            if !(1..=5000).contains(limit) {
                return Err(fault(
                    "records.query_limit",
                    2,
                    "Decor limit must be 1..5000",
                ));
            }
            access.decor_list(*limit as usize).await?
        }
        DomainRequest::DecorGet { field, wanted, .. } => {
            positive(*wanted)?;
            access.decor_get(selector(field), *wanted).await?
        }
        DomainRequest::CreatureDisplay {
            display_id,
            file_data_id,
            ..
        } => {
            if (*display_id == 0) == (*file_data_id == 0) {
                return Err(fault(
                    "records.request_conflict",
                    2,
                    "Exactly one creature selector is required",
                ));
            }
            let mut report = access.creature_display(*display_id, *file_data_id).await?;
            if *display_id == 0 {
                access.budget.charge(Cost::metadata(128))?;
                report
                    .as_object_mut()
                    .ok_or_else(|| fault("data.domain.value", 8, "Expected object"))?
                    .insert("requestedFileDataID".into(), json!(file_data_id));
            }
            report
        }
        DomainRequest::CreatureModel { file_data_id, .. } => {
            positive(*file_data_id)?;
            access.creature_model(*file_data_id).await?
        }
    };
    let complete = access.complete();
    let Access {
        sources,
        relations,
        partial,
        truncation,
        ..
    } = access;
    let context = json!({"relations":relations,"complete":complete,"partial":!partial.is_empty(),"partialReasons":if partial.is_empty(){Value::Null}else{json!(partial)},"truncated":truncation.is_some(),"truncationReason":truncation.unwrap_or("none")});
    if let Value::Object(context) = context {
        report
            .as_object_mut()
            .ok_or_else(|| {
                fault(
                    "data.domain.value",
                    8,
                    "Domain presentation must be an object",
                )
            })?
            .extend(context);
    }
    let report = owner.finish(report, sources)?;
    let capture = engine.capture_domain(&report, &request, control)?;
    Ok((report, capture))
}

pub(super) fn schema() -> Value {
    let positive = json!({"type":"integer","minimum":1,"maximum":4294967295u64});
    let optional = json!({"type":"integer","minimum":0,"maximum":4294967295u64});
    let depth = json!({"type":"integer","minimum":0,"maximum":32,"description":"0 selects default: spell 5, encounter 32"});
    let mut variants = Vec::new();
    for (kind, fields) in [
        ("item_get", json!({"item_id":positive})),
        (
            "item_models",
            json!({"item_id":positive,"race_id":{"type":"integer","minimum":i64::MIN,"maximum":i64::MAX},"gender":{"type":"integer","minimum":i64::MIN,"maximum":i64::MAX}}),
        ),
        ("item_textures", json!({"item_id":positive})),
        ("item_geosets", json!({"item_id":positive})),
        ("spell_info", json!({"spell_id":positive,"max_depth":depth})),
        ("spell_auras", json!({"spell_id":positive})),
        (
            "spell_summons",
            json!({"spell_id":positive,"npc_id":optional}),
        ),
        (
            "encounter_get",
            json!({"encounter_id":positive,"max_depth":depth}),
        ),
        (
            "decor_list",
            json!({"limit":{"type":"integer","minimum":1,"maximum":5000}}),
        ),
        (
            "decor_get",
            json!({"field":{"enum":["id","item_id","model_file_data_id"]},"wanted":positive}),
        ),
        (
            "creature_display",
            json!({"display_id":optional,"file_data_id":optional}),
        ),
        ("creature_model", json!({"file_data_id":positive})),
    ] {
        let mut props = fields.as_object().unwrap().clone();
        let mut required: Vec<Value> = props.keys().map(|s| json!(s)).collect();
        required.extend([json!("kind"), json!("mode")]);
        props.insert("kind".into(), json!({"const":kind}));
        props.insert("mode".into(), json!({"enum":["static","effective"]}));
        props.insert("hotfix_captures".into(),json!({"type":"array","maxItems":16,"uniqueItems":true,"default":[],"items":{"type":"string","minLength":1,"maxLength":256}}));
        let mut v = json!({"type":"object","properties":props,"required":required,"additionalProperties":false,"allOf":[{"if":{"properties":{"mode":{"const":"static"}}},"then":{"properties":{"hotfix_captures":{"maxItems":0}}},"else":{"required":["hotfix_captures"],"properties":{"hotfix_captures":{"minItems":1}}}}]});
        if kind == "creature_display" {
            v["oneOf"] = json!([{"properties":{"display_id":{"minimum":1},"file_data_id":{"const":0}}},{"properties":{"display_id":{"const":0},"file_data_id":{"minimum":1}}}]);
        }
        variants.push(v);
    }
    json!({"oneOf":variants,"evidence":"Full report including relations and explicit completeness/truncation plus original typed request and source receipts is automatically captured. Post-commit delivery errors preserve publication identity; never replay automatically.","identity":"Physical locators remain private and separate from public ID. Creature display file selection returns requestedFileDataID separately from computed fileDataID."})
}
