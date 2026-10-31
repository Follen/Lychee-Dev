//! Bounded, request-bound domain presentation and authenticated source roles.
//! Validation never executes a second query or rebuilds the business result.
use super::{
    OwnedProvenance,
    api::{DecorSelector, DomainRequest, SourceMode},
    lifetime::RequestLifetime,
};
use crate::{
    fault,
    resource::{Budget, Cost},
};
use lycheedev_core::{Result, ToolkitError};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub(crate) struct DomainOwner {
    lifetime: Arc<RequestLifetime>,
    session: Arc<Budget>,
    // Only Engine's begin_domain_request can bind this before the algorithm runs.
    // It is neither caller-editable JSON nor a request supplied at capture time.
    bound_request: Option<DomainRequest>,
}
impl DomainOwner {
    pub(super) fn new(lifetime: Arc<RequestLifetime>, session: Arc<Budget>) -> Self {
        Self {
            lifetime,
            session,
            bound_request: None,
        }
    }
    pub(super) fn bind_request(mut self, request: &DomainRequest) -> Result<Self> {
        self.lifetime.budget.check()?;
        if self.bound_request.is_some() {
            return Err(identity());
        }
        validate_input(request)?;
        let (_, captures) = request.binding();
        let bytes =
            captures
                .iter()
                .try_fold(std::mem::size_of::<DomainRequest>() + 256, |n, id| {
                    n.checked_add(id.len())
                        .and_then(|n| n.checked_add(std::mem::size_of::<String>()))
                        .ok_or_else(resource)
                })?;
        // Admit before cloning strings/vector, not after creating a request copy.
        self.lifetime.budget.charge(Cost::retained(bytes))?;
        self.bound_request = Some(request.clone());
        Ok(self)
    }
    pub(crate) fn budget(&self) -> Arc<Budget> {
        self.lifetime.budget.clone()
    }
    /// The domain algorithm must admit all presentation allocations first.
    pub(crate) fn finish(
        self,
        report: Value,
        sources: Vec<OwnedProvenance>,
    ) -> Result<DomainResult> {
        self.lifetime.budget.check()?;
        if sources
            .iter()
            .any(|source| !source.belongs_to(&self.session))
        {
            return Err(fault(
                "records.source_identity",
                3,
                "Domain sources belong to a different session",
            ));
        }
        self.lifetime.budget.charge(Cost::retained(
            sources
                .len()
                .checked_mul(64)
                .and_then(|n| n.checked_add(256))
                .ok_or_else(resource)?,
        ))?;
        if let Some(request) = &self.bound_request {
            validate_bound(request, &report, &sources, &self.lifetime.budget)?;
        }
        let size = crate::capture::json_size(&report, &self.lifetime.budget)?;
        if size > 16 << 20 {
            return Err(fault(
                "data.domain.result_limit",
                3,
                "Domain report exceeds the explicit 16MiB serialized evidence limit; no truncated success is returned",
            ));
        }
        self.lifetime.settle()?;
        Ok(DomainResult {
            report,
            sources,
            _owner: self,
        })
    }
}
impl Drop for DomainOwner {
    fn drop(&mut self) {
        // Release the retained request before its lifetime/session admission.
        drop(self.bound_request.take());
    }
}
#[derive(Serialize)]
pub struct DomainResult {
    report: Value,
    sources: Vec<OwnedProvenance>,
    #[serde(skip)]
    _owner: DomainOwner,
}
impl Drop for DomainResult {
    fn drop(&mut self) {
        drop(std::mem::replace(&mut self.report, Value::Null));
        drop(std::mem::take(&mut self.sources));
    }
}
impl DomainResult {
    pub(crate) fn sources(&self) -> &[OwnedProvenance] {
        &self.sources
    }
    pub(super) fn session(&self) -> &Arc<Budget> {
        &self._owner.session
    }
    pub(super) fn truncated(&self) -> Result<bool> {
        completeness(&self.report, "truncated")
    }
    pub(super) fn complete(&self) -> Result<bool> {
        completeness(&self.report, "complete")
    }
    pub(super) fn validate_request(&self, request: &DomainRequest, budget: &Budget) -> Result<()> {
        budget.check()?;
        if self
            .sources
            .iter()
            .any(|source| !source.belongs_to(self.session()))
        {
            return Err(fault(
                "records.source_identity",
                3,
                "Domain source session differs from its presentation owner",
            ));
        }
        match &self._owner.bound_request {
            Some(bound) if bound == request => {
                validate_bound(request, &self.report, &self.sources, budget)
            }
            Some(_) => Err(identity()),
            None => Err(identity()),
        }
    }
}
fn identity() -> ToolkitError {
    fault(
        "data.domain.request_identity",
        4,
        "Domain request differs from its bound request, authenticated source tasks, or presented identity",
    )
}
fn resource() -> ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Domain metadata admission overflow",
    )
}
fn completeness(report: &Value, key: &str) -> Result<bool> {
    report.get(key).and_then(Value::as_bool).ok_or_else(|| {
        fault(
            "data.domain.completeness_missing",
            4,
            "Domain report must explicitly declare business completeness and truncation",
        )
    })
}
fn validate_input(request: &DomainRequest) -> Result<()> {
    let (mode, captures) = request.binding();
    if captures.len() > 16 || (matches!(mode, SourceMode::Static) != captures.is_empty()) {
        return Err(identity());
    }
    for (index, id) in captures.iter().enumerate() {
        if id.is_empty()
            || id.len() > 256
            || id.chars().any(char::is_control)
            || captures[..index].contains(id)
        {
            return Err(identity());
        }
    }
    let valid = match request {
        DomainRequest::ItemGet { item_id, .. }
        | DomainRequest::ItemModels { item_id, .. }
        | DomainRequest::ItemTextures { item_id, .. }
        | DomainRequest::ItemGeosets { item_id, .. } => *item_id != 0,
        DomainRequest::SpellInfo {
            spell_id,
            max_depth,
            ..
        } => *spell_id != 0 && *max_depth <= 32,
        DomainRequest::SpellAuras { spell_id, .. }
        | DomainRequest::SpellSummons { spell_id, .. } => *spell_id != 0,
        DomainRequest::EncounterGet {
            encounter_id,
            max_depth,
            ..
        } => *encounter_id != 0 && *max_depth <= 32,
        DomainRequest::DecorList { limit, .. } => (1..=5000).contains(limit),
        DomainRequest::DecorGet { wanted, .. } => *wanted != 0,
        DomainRequest::CreatureDisplay {
            display_id,
            file_data_id,
            ..
        } => (*display_id == 0) != (*file_data_id == 0),
        DomainRequest::CreatureModel { file_data_id, .. } => *file_data_id != 0,
    };
    if valid { Ok(()) } else { Err(identity()) }
}
fn is_id(value: &Value, expected: u32) -> bool {
    value.as_u64() == Some(u64::from(expected))
}
fn report_identity(request: &DomainRequest, report: &Value) -> bool {
    match request {
        DomainRequest::ItemGet { item_id, .. } => is_id(&report["item"]["id"], *item_id),
        DomainRequest::ItemModels {
            item_id,
            race_id,
            gender,
            ..
        } => {
            is_id(&report["itemID"], *item_id)
                && report["raceID"].as_i64() == Some(*race_id)
                && report["gender"].as_i64() == Some(*gender)
        }
        DomainRequest::ItemTextures { item_id, .. }
        | DomainRequest::ItemGeosets { item_id, .. } => is_id(&report["itemID"], *item_id),
        DomainRequest::SpellInfo { spell_id, .. } | DomainRequest::SpellAuras { spell_id, .. } => {
            is_id(&report["spellID"], *spell_id)
        }
        DomainRequest::SpellSummons {
            spell_id, npc_id, ..
        } => is_id(&report["spellID"], *spell_id) && is_id(&report["npcID"], *npc_id),
        DomainRequest::EncounterGet { encounter_id, .. } => {
            is_id(&report["journalEncounterID"], *encounter_id)
        }
        DomainRequest::DecorList { limit, .. } => report["items"].as_array().is_some_and(|items| {
            items.len() <= *limit as usize && report["count"].as_u64() == Some(items.len() as u64)
        }),
        DomainRequest::DecorGet { field, wanted, .. } => is_id(
            &report["item"][match field {
                DecorSelector::Id => "id",
                DecorSelector::ItemId => "itemID",
                DecorSelector::ModelFileDataId => "modelFileDataID",
            }],
            *wanted,
        ),
        DomainRequest::CreatureDisplay {
            display_id,
            file_data_id,
            ..
        } => {
            if *display_id != 0 {
                is_id(&report["displayID"], *display_id)
            } else {
                // The requested selector is distinct from fileDataID, which
                // the shared algorithm computes through a related physical-ID
                // lookup and may legitimately resolve to zero or another file.
                // The immutable owner and required root scan independently
                // bind this explicit selector field to the original request.
                is_id(&report["requestedFileDataID"], *file_data_id)
            }
        }
        DomainRequest::CreatureModel { file_data_id, .. } => {
            is_id(&report["fileDataID"], *file_data_id)
        }
    }
}

// Borrow exact internally-produced task shapes without cloning arbitrary JSON,
// accepting SQL, searching recursively for IDs, or reconstructing dependencies.
#[derive(Clone, Copy)]
enum Action<'a> {
    Lookup(u32),
    Scan(Option<(&'a str, u32)>, usize),
}
struct SourceTask<'a> {
    name: &'a str,
    table: &'a Value,
    action: Action<'a>,
}
fn keys(value: &Value, required: &[&str], optional: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        required.iter().all(|key| object.contains_key(*key))
            && object
                .keys()
                .all(|key| required.contains(&key.as_str()) || optional.contains(&key.as_str()))
    })
}
fn u32_value(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|n| u32::try_from(n).ok())
}
fn task(value: &Value) -> Result<SourceTask<'_>> {
    let request = &value["request"];
    let (body, action) = match request["kind"].as_str() {
        Some("domain_lookup") if keys(request, &["kind", "table", "ids"], &[]) => {
            (request, "lookup")
        }
        Some("domain_scan") if keys(request, &["kind", "table", "filter", "limit"], &[]) => {
            (request, "scan")
        }
        _ => return Err(identity()),
    };
    let table = &body["table"];
    if !keys(table, &["name", "mode"], &["hotfix_captures"]) {
        return Err(identity());
    }
    let name = table["name"].as_str().ok_or_else(identity)?;
    let action = match action {
        "lookup" => {
            let ids = body["ids"]
                .as_array()
                .filter(|ids| ids.len() == 1)
                .ok_or_else(identity)?;
            Action::Lookup(u32_value(&ids[0]).ok_or_else(identity)?)
        }
        _ => {
            let filter = if body["filter"].is_null() {
                None
            } else {
                let fields = body["filter"]
                    .as_array()
                    .filter(|fields| fields.len() == 2)
                    .ok_or_else(identity)?;
                Some((
                    fields[0].as_str().ok_or_else(identity)?,
                    u32_value(&fields[1]).ok_or_else(identity)?,
                ))
            };
            let limit = body["limit"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(identity)?;
            Action::Scan(filter, limit)
        }
    };
    Ok(SourceTask {
        name,
        table,
        action,
    })
}
impl SourceTask<'_> {
    fn lookup(&self, name: &str, wanted: Option<u32>) -> bool {
        self.name.eq_ignore_ascii_case(name)
            && matches!(self.action, Action::Lookup(id) if wanted.is_none_or(|wanted| id == wanted))
    }
    fn scan(&self, name: &str, field: Option<&str>, wanted: Option<u32>, limit: usize) -> bool {
        self.name.eq_ignore_ascii_case(name)
            && match self.action {
                Action::Scan(None, count) => field.is_none() && wanted.is_none() && count == limit,
                Action::Scan(Some((actual, id)), count) => {
                    field == Some(actual)
                        && wanted.is_none_or(|wanted| id == wanted)
                        && count == limit
                }
                _ => false,
            }
    }
}
fn same_history(value: Option<&Value>, captures: &[String]) -> bool {
    match value {
        None => captures.is_empty(),
        Some(value) => value.as_array().is_some_and(|ids| {
            ids.len() == captures.len()
                && ids
                    .iter()
                    .zip(captures)
                    .all(|(id, expected)| id.as_str() == Some(expected.as_str()))
        }),
    }
}
fn source_identity(
    source: &OwnedProvenance,
    task: &SourceTask<'_>,
    request: &DomainRequest,
    report_complete: bool,
) -> Result<()> {
    let (mode, captures) = request.binding();
    let mode = match mode {
        SourceMode::Static => "static",
        SourceMode::Effective => "effective",
    };
    let value = source.value();
    if task.table["mode"] != mode || !same_history(task.table.get("hotfix_captures"), captures) {
        return Err(identity());
    }
    // CAP dependencies remain the private Engine-owned set, never source labels.
    if source.dependencies().len() != captures.len()
        || captures.iter().any(|id| {
            source
                .dependencies()
                .iter()
                .filter(|actual| *actual == id)
                .count()
                != 1
        })
    {
        return Err(identity());
    }
    let receipts = value["sources"]
        .as_array()
        .filter(|sources| sources.len() == 1)
        .ok_or_else(identity)?;
    let receipt = &receipts[0];
    if receipt["sourceMode"] != mode
        || !receipt["sourceIdentity"]["table"]["name"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(task.name))
    {
        return Err(identity());
    }
    if value["physicalLocatorIdentity"] != "authenticated_source_index" {
        return Err(identity());
    }
    if mode == "static" {
        if !receipt["effective"].is_null() {
            return Err(identity());
        }
    } else {
        let effective = &receipt["effective"];
        let ids = effective["captures"].as_array().ok_or_else(identity)?;
        if !effective["table"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(task.name))
            || ids.len() != captures.len()
            || ids
                .iter()
                .zip(captures)
                .any(|(id, expected)| id["id"].as_str() != Some(expected.as_str()))
        {
            return Err(identity());
        }
    }
    let complete = value["complete"].as_bool().ok_or_else(identity)?;
    let source_complete = receipt["complete"].as_bool().ok_or_else(identity)?;
    if (complete && !source_complete) || (report_complete && (!complete || !source_complete)) {
        return Err(identity());
    }
    Ok(())
}

// A closed set of source roles. Seed roles must exist even when an optional row
// is missing; empty lookup results still own their authenticated provenance.
// Derived IDs are supplied by the already-run shared algorithms. Checking their
// full business lineage here would duplicate/re-execute that domain algorithm.
fn role(request: &DomainRequest, report: &Value, source: &SourceTask<'_>) -> Option<u16> {
    let lookup = |name, id| source.lookup(name, id);
    let scan = |name, field, id, limit| source.scan(name, field, id, limit);
    match request {
        DomainRequest::ItemGet { item_id, .. } => {
            if lookup("ItemSparse", Some(*item_id)) {
                Some(1)
            } else if lookup("Item", Some(*item_id)) {
                Some(2)
            } else {
                None
            }
        }
        DomainRequest::ItemModels { item_id, .. }
        | DomainRequest::ItemTextures { item_id, .. }
        | DomainRequest::ItemGeosets { item_id, .. } => {
            if lookup("ItemSparse", Some(*item_id)) {
                return Some(1);
            }
            if scan(
                "ItemModifiedAppearance",
                Some("ItemID"),
                Some(*item_id),
                65536,
            ) {
                return Some(2);
            }
            let common = lookup("ItemAppearance", None) || lookup("ItemDisplayInfo", None);
            let special = match request {
                DomainRequest::ItemModels { .. } => {
                    scan("ModelFileData", Some("ModelResourcesID"), None, 65536)
                        || scan("TextureFileData", Some("MaterialResourcesID"), None, 65536)
                        || lookup("ComponentModelFileData", None)
                }
                DomainRequest::ItemTextures { .. } => {
                    u32_value(&report["displayID"]).is_some_and(|id| {
                        scan(
                            "ItemDisplayInfoMaterialRes",
                            Some("ItemDisplayInfoID"),
                            Some(id),
                            65536,
                        )
                    }) || scan("TextureFileData", Some("MaterialResourcesID"), None, 65536)
                }
                DomainRequest::ItemGeosets { .. } => scan(
                    "HelmetGeosetData",
                    Some("HelmetGeosetVisDataID"),
                    None,
                    65536,
                ),
                _ => false,
            };
            (common || special).then_some(0)
        }
        DomainRequest::SpellInfo { spell_id, .. } => {
            if lookup("SpellName", Some(*spell_id)) {
                Some(1)
            } else if lookup("Spell", Some(*spell_id)) {
                Some(2)
            } else if scan("SpellEffect", Some("SpellID"), Some(*spell_id), 65536) {
                Some(4)
            } else if scan("SpellMisc", Some("SpellID"), Some(*spell_id), 1) {
                Some(8)
            } else {
                (lookup("SpellName", None)
                    || lookup("Spell", None)
                    || lookup("SpellCastTimes", None)
                    || lookup("SpellDuration", None)
                    || lookup("SpellRange", None)
                    || scan("SpellEffect", Some("SpellID"), None, 65536)
                    || scan("SpellMisc", Some("SpellID"), None, 1))
                .then_some(0)
            }
        }
        DomainRequest::SpellAuras { spell_id, .. }
        | DomainRequest::SpellSummons { spell_id, .. } => {
            scan("SpellEffect", Some("SpellID"), Some(*spell_id), 65536).then_some(1)
        }
        DomainRequest::EncounterGet { encounter_id, .. } => {
            if scan(
                "JournalEncounterSection",
                Some("JournalEncounterID"),
                Some(*encounter_id),
                10000,
            ) {
                Some(1)
            } else {
                (lookup("SpellName", None) || scan("SpellMisc", Some("SpellID"), None, 1))
                    .then_some(0)
            }
        }
        DomainRequest::DecorList { limit, .. } => {
            scan("HouseDecor", None, None, *limit as usize).then_some(1)
        }
        DomainRequest::DecorGet { field, wanted, .. } => scan(
            "HouseDecor",
            Some(match field {
                DecorSelector::Id => "ID",
                DecorSelector::ItemId => "ItemID",
                DecorSelector::ModelFileDataId => "ModelFileDataID",
            }),
            Some(*wanted),
            5000,
        )
        .then_some(1),
        DomainRequest::CreatureDisplay {
            display_id,
            file_data_id,
            ..
        } => {
            if *display_id != 0 && lookup("CreatureDisplayInfo", Some(*display_id)) {
                Some(1)
            } else if *display_id == 0
                && scan(
                    "CreatureModelData",
                    Some("FileDataID"),
                    Some(*file_data_id),
                    65536,
                )
            {
                Some(1)
            } else if u32_value(&report["displayID"]).is_some_and(|id| {
                scan(
                    "CreatureDisplayInfoGeosetData",
                    Some("CreatureDisplayInfoID"),
                    Some(id),
                    65536,
                )
            }) {
                Some(2)
            } else if *display_id == 0 && scan("CreatureDisplayInfo", Some("ModelID"), None, 65536)
            {
                Some(4)
            } else {
                lookup("CreatureModelData", None).then_some(0)
            }
        }
        DomainRequest::CreatureModel { file_data_id, .. } => {
            if scan(
                "CreatureModelData",
                Some("FileDataID"),
                Some(*file_data_id),
                65536,
            ) {
                Some(1)
            } else {
                (scan("CreatureDisplayInfo", Some("ModelID"), None, 65536)
                    || scan(
                        "CreatureDisplayInfoGeosetData",
                        Some("CreatureDisplayInfoID"),
                        None,
                        65536,
                    ))
                .then_some(0)
            }
        }
    }
}
fn validate_bound(
    request: &DomainRequest,
    report: &Value,
    sources: &[OwnedProvenance],
    budget: &Budget,
) -> Result<()> {
    validate_input(request)?;
    let complete = completeness(report, "complete")?;
    let truncated = completeness(report, "truncated")?;
    if !report_identity(request, report)
        || !report["relations"].is_array()
        || (complete
            && (truncated
                || report["partial"] == true
                || report["partialReasons"]
                    .as_array()
                    .is_some_and(|reasons| !reasons.is_empty())))
    {
        return Err(identity());
    }
    let mut observed = 0u16;
    for source in sources {
        budget.check()?;
        budget.charge(Cost::work(64 + request.binding().1.len() * 32))?;
        let source_task = task(source.value())?;
        source_identity(source, &source_task, request, complete)?;
        let bit = role(request, report, &source_task).ok_or_else(identity)?;
        if matches!(request, DomainRequest::ItemGet { .. }) && observed & bit != 0 {
            return Err(identity());
        }
        observed |= bit;
    }
    let required = match request {
        DomainRequest::ItemGet { .. }
        | DomainRequest::ItemModels { .. }
        | DomainRequest::ItemTextures { .. }
        | DomainRequest::ItemGeosets { .. } => 3,
        DomainRequest::SpellInfo { .. } => 15,
        DomainRequest::CreatureDisplay { display_id, .. } => {
            if *display_id == 0 {
                7
            } else {
                3
            }
        }
        _ => 1,
    };
    let exact = match request {
        DomainRequest::ItemGet { .. } => Some(2),
        DomainRequest::DecorList { .. }
        | DomainRequest::DecorGet { .. }
        | DomainRequest::SpellAuras { .. }
        | DomainRequest::SpellSummons { .. } => Some(1),
        _ => None,
    };
    if observed & required != required || exact.is_some_and(|count| sources.len() != count) {
        return Err(identity());
    }
    Ok(())
}

#[cfg(test)]
#[path = "domain_tests.rs"]
mod tests;
