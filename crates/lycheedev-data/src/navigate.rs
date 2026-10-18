//! Domain navigation consumes the authenticated typed table provider. Matching
//! is exact integer equality; presentation coercions preserve the Go contracts.
mod catalog;
use crate::{
    capture, db2, fault,
    resource::{Budget, Cost},
    table,
};
use lycheedev_core::{Invocation, Outcome, Result, evidence};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_RELATED: usize = 65536;
type Row = BTreeMap<String, table::Value>;

fn identifier(inv: &Invocation, flag: &str, optional: bool) -> Result<u32> {
    let Some(text) = inv.flag(flag) else {
        if optional {
            return Ok(0);
        }
        return Err(fault(
            "command.invalid_arguments",
            2,
            format!("{} requires {flag}", inv.command),
        ));
    };
    let value = text.parse::<u32>().ok().filter(|n| optional || *n != 0);
    value.ok_or_else(|| fault("command.invalid_arguments", 2, format!("invalid {flag}")))
}
fn matches(value: Option<&table::Value>, wanted: u32) -> bool {
    match value {
        Some(table::Value::Int(v)) => *v >= 0 && *v as u64 == u64::from(wanted),
        Some(table::Value::UInt(v)) => *v == u64::from(wanted),
        _ => false,
    }
}
fn integer(value: Option<&table::Value>) -> i64 {
    match value {
        Some(table::Value::Int(v)) => *v,
        Some(table::Value::UInt(v)) => i64::try_from(*v).unwrap_or(0),
        Some(table::Value::Float32(v)) => *v as i64,
        _ => 0,
    }
}
fn unsigned(value: Option<&table::Value>) -> u32 {
    match value {
        Some(table::Value::Int(v)) if *v > 0 => u32::try_from(*v).unwrap_or(0),
        Some(table::Value::UInt(v)) => u32::try_from(*v).unwrap_or(0),
        _ => 0,
    }
}
fn first_unsigned(value: Option<&table::Value>) -> u32 {
    match value {
        Some(table::Value::Array(values)) => unsigned(values.first()),
        value => unsigned(value),
    }
}
fn link(id: u32, field: &str, kind: &str, target: u32) -> Value {
    json!({"fromTable":"SpellEffect","fromField":field,"fromID":id,"toKind":kind,"toID":target,"depth":0})
}

struct SpellEffects {
    spell: u32,
    npc: u32,
    summon: bool,
    count: usize,
    truncated: bool,
    has_aura: bool,
    summons: Vec<Value>,
    relations: Vec<Value>,
}
impl SpellEffects {
    fn new(spell: u32, npc: u32, summon: bool) -> Self {
        Self {
            spell,
            npc,
            summon,
            count: 0,
            truncated: false,
            has_aura: false,
            summons: vec![],
            relations: vec![],
        }
    }
    // Return false only after the single extra matching row proves truncation.
    fn accept(&mut self, id: u32, row: Row, budget: &Budget) -> Result<bool> {
        if !matches(row.get("SpellID"), self.spell) {
            return Ok(true);
        }
        if self.count == MAX_RELATED {
            self.truncated = true;
            return Ok(false);
        }
        self.count += 1;
        // Retained result/relationship containers are charged before ownership
        // transfer. WDC has already charged row decoding; do not re-charge that.
        if !self.summon {
            self.has_aura |= integer(row.get("Effect")) == 6;
            budget.charge(Cost::retained(256))?;
            self.relations
                .push(link(id, "SpellID", "spell", self.spell));
        } else if integer(row.get("Effect")) == 28 {
            let npc = first_unsigned(row.get("EffectMiscValue"));
            if self.npc != 0 && self.npc != npc {
                return Ok(true);
            }
            let encoded = capture::json_size(&row, budget)?;
            budget.charge(Cost::retained(
                encoded
                    .checked_mul(4)
                    .and_then(|n| n.checked_add(512))
                    .ok_or_else(|| {
                        fault(
                            "records.query_resource_budget",
                            3,
                            "Domain row admission overflow",
                        )
                    })?,
            ))?;
            if npc != 0 {
                self.relations.push(link(id, "EffectMiscValue", "npc", npc));
            }
            self.summons.push(json!({"spellID":self.spell,"npcID":npc,"effectIndex":integer(row.get("EffectIndex")),"row":row}));
        }
        Ok(true)
    }
    fn result(&self) -> Value {
        if self.summon {
            json!({"spellID":self.spell,"npcID":self.npc,"count":self.summons.len(),"summons":self.summons})
        } else {
            json!({"spellID":self.spell,"hasAura":if self.has_aura{vec![self.spell]}else{vec![]},"noAura":if self.has_aura{vec![]}else{vec![self.spell]}})
        }
    }
}

pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    dispatch_after_read(inv, &mut || Ok(()))
}
pub(crate) fn dispatch_after_read(
    inv: &Invocation,
    after_read: &mut impl FnMut() -> Result<()>,
) -> Result<Outcome> {
    dispatch_finishing(inv, after_read, |scope, result| scope.finish(result))
}
// Tests may observe the real consuming finish without replacing IO or capture.
pub(crate) fn dispatch_finishing(
    inv: &Invocation,
    after_read: &mut impl FnMut() -> Result<()>,
    finish: impl for<'a> FnOnce(crate::remote::provider::Scope<'a>, Result<Outcome>) -> Result<Outcome>,
) -> Result<Outcome> {
    if matches!(
        inv.command.as_str(),
        "data item get"
            | "data item models"
            | "data item geosets"
            | "data item textures"
            | "data spell info"
            | "data decor list"
            | "data decor get"
            | "data creature display"
            | "data creature model"
            | "data encounter get"
    ) {
        return catalog::dispatch(inv, after_read, finish);
    }
    inv.control.check()?;
    let summon = match inv.command.as_str() {
        "data spell auras" => false,
        "data spell summons" => true,
        _ => {
            return Err(fault(
                "command.unsupported",
                3,
                "Domain profile not implemented",
            ));
        }
    };
    let spell = identifier(inv, "--spell-id", false)?;
    let npc = if summon {
        identifier(inv, "--npc-id", true)?
    } else {
        0
    };
    let timeout = inv.usize_flag("--timeout-seconds", 300, 1, 3600)?;
    let budget = Arc::new(Budget::query_controlled(
        Instant::now() + Duration::from_secs(timeout as u64),
        inv.control.clone(),
    ));
    // Navigation admits both sources before resolving the pin. Keep this owner
    // through decoding, source revalidation, capture, and checked teardown.
    let mut scope = crate::remote::provider::OpenedScope::open(inv, &budget)?.admit()?;
    let mut committed = Vec::new();
    let result = (|| {
        let pin = scope.resolve_pin()?;
        let data = pin
            .data
            .as_ref()
            .ok_or_else(|| fault("command.failed", 5, "records.data_pin_required"))?;
        scope.check_pin(data)?;
        let mut execute = |reader: &mut dyn crate::remote::provider::FileScope| {
            let prepared = db2::prepare_scoped(
                inv,
                "SpellEffect",
                &budget,
                Some(&mut *reader),
                Some(pin.clone()),
            )?;
            let view = table::open_budgeted(
                &prepared.file.bytes,
                &prepared.definition,
                Default::default(),
                &prepared.availability.skipped,
                &prepared.availability.ranges,
                budget.clone(),
            )?;
            let field = prepared
                .definition
                .fields
                .iter()
                .find(|f| f.name == "SpellID")
                .ok_or_else(|| {
                    fault("records.field_unknown", 4, "records.field_unknown: SpellID")
                })?;
            if field.kind != "int" || field.array {
                return Err(fault(
                    "records.field_type_mismatch",
                    2,
                    format!("records.field_type_mismatch: SpellID is {}", field.kind),
                ));
            }
            let mut effects = SpellEffects::new(spell, npc, summon);
            for id in view.ids() {
                budget.charge(Cost::work(1))?;
                // Full decoding preserves corrupt unused values and schema coverage.
                let row = view.row(id, 1 << 20)?;
                if !effects.accept(id, row, &budget)? {
                    break;
                }
            }
            let partial = view.coverage()["complete"] != true;
            let mut result = effects.result();
            let data = prepared
                .pin
                .data
                .as_ref()
                .expect("prepare checked fixed DataPin");
            let mut proof = json!({"contentVerified":prepared.file.reading["contentVerified"],"keySource":prepared.file.reading["keySource"],"name":prepared.bundle.identity.name,"identity":prepared.bundle.identity,"layoutHash":format!("{:08X}",prepared.layout.layout_hash),"definitionSHA256":prepared.definition.sha256,"fileDataID":prepared.bundle.identity.db2_file_data_id,"contentKey":prepared.file.reading["entry"]["contentKey"],"logicalRows":view.logical_count()});
            if partial {
                proof["partialContent"] = prepared.file.reading["partialContent"].clone();
                proof["missing"] = prepared.file.reading["missing"].clone();
                proof["unavailablePartitions"] = json!(prepared.availability.partitions);
            }
            let context = json!({"resources":budget.logical_snapshot()?,"memoryBudget":budget.memory_snapshot()?,"snapshot":prepared.pin.id,"pin":data,"source":prepared.file.reading["source"],"tables":[proof],"relations":effects.relations,"complete":!partial&&!effects.truncated,"truncated":effects.truncated,"truncationReason":if effects.truncated{"row_budget"}else{"none"},"partial":partial,"partialReasons":if partial{json!(["encrypted_sections_unavailable"])}else{Value::Null}});
            result
                .as_object_mut()
                .unwrap()
                .extend(context.as_object().unwrap().clone());
            result["tableVerification"] = json!([prepared.file.reading["verification"]]);
            result["resourceAccounting"] = json!({"wholeQueryAccountingComplete":false,"strategy":"conservative cumulative admission","wdcMetadataAdmittedBytes":view.admitted_metadata_bytes(),"remainingGaps":["uninterruptible single codec call"]});
            budget.check()?;
            after_read()?;
            catalog::check_source(inv, &budget, &*reader)?;
            let raw = capture::json_bytes(&result, &budget, 16 << 20, || {
                fault(
                    "records.query_resource_budget",
                    3,
                    "Domain evidence byte budget exceeded",
                )
            })?;
            let verb = if summon { "summons" } else { "auras" };
            // Admit the small retained identity before publication.
            capture::admit_committed_identity(&budget)?;
            let capture = evidence::commit_capture_budgeted(
                &inv.home,
                evidence::CaptureInput {
                    bytes: &raw,
                    maximum_bytes: 16 << 20,
                    media_type: "application/json",
                    provenance: json!({"kind":format!("spell-{verb}"),"locator":format!("spell:{verb}:{spell}"),"snapshot":prepared.pin.id,"dataBuild":data.full_build}),
                    complete: !partial && !effects.truncated,
                    truncated: effects.truncated,
                },
                &inv.control,
                budget.as_ref(),
            )?;
            committed.push(
                capture["id"]
                    .as_str()
                    .expect("committed query capture identity")
                    .to_owned(),
            );
            let mut outcome = Outcome::new(result);
            outcome.complete = !partial && !effects.truncated;
            outcome.selection = serde_json::to_value(prepared.pin)?;
            outcome.provenance = vec![capture];
            if effects.truncated {
                outcome.warnings.push(json!("This result is truncated; preserve the snapshot and the stated truncation reason when continuing."));
            }
            Ok(outcome)
        };
        if inv.flag_bool("--cdn") {
            scope.with_session(data, |reader| execute(reader))
        } else {
            scope.with_local_session(data, |reader| execute(reader))
        }
    })();
    finish(scope, result).map_err(|error| capture::query_committed_error(error, &committed, false))
}

#[cfg(test)]
mod admission_tests;

#[cfg(test)]
mod tests {
    use super::*;
    fn row(spell: table::Value, effect: i64, npc: table::Value) -> Row {
        BTreeMap::from([
            ("SpellID".into(), spell),
            ("Effect".into(), table::Value::Int(effect)),
            ("EffectIndex".into(), table::Value::Int(2)),
            ("EffectMiscValue".into(), npc),
        ])
    }
    fn budget() -> Budget {
        Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(10),
            None,
        )
    }
    #[test]
    fn aura_presence_is_effect_six_and_exact_integer_identity() {
        let budget = budget();
        let mut effects = SpellEffects::new(200, 0, false);
        effects
            .accept(
                1,
                row(table::Value::Float32(200.), 6, table::Value::Null),
                &budget,
            )
            .unwrap();
        effects
            .accept(
                2,
                row(table::Value::UInt(200), 7, table::Value::Null),
                &budget,
            )
            .unwrap();
        assert_eq!(
            effects.result(),
            json!({"spellID":200,"hasAura":[],"noAura":[200]})
        );
        effects
            .accept(
                3,
                row(table::Value::Int(200), 6, table::Value::Null),
                &budget,
            )
            .unwrap();
        assert_eq!(
            effects.result(),
            json!({"spellID":200,"hasAura":[200],"noAura":[]})
        );
        assert_eq!(
            effects.relations,
            vec![
                link(2, "SpellID", "spell", 200),
                link(3, "SpellID", "spell", 200)
            ]
        );
    }
    #[test]
    fn summons_filter_first_array_value_preserve_full_row_and_probe_limit() {
        let budget = budget();
        let mut effects = SpellEffects::new(300, 5000, true);
        let expected = row(
            table::Value::UInt(300),
            28,
            table::Value::Array(vec![table::Value::Int(5000), table::Value::UInt(9)]),
        );
        effects.accept(3, expected.clone(), &budget).unwrap();
        effects
            .accept(
                4,
                row(table::Value::UInt(300), 28, table::Value::UInt(9)),
                &budget,
            )
            .unwrap();
        assert_eq!(
            effects.result(),
            json!({"spellID":300,"npcID":5000,"count":1,"summons":[{"spellID":300,"npcID":5000,"effectIndex":2,"row":expected}]})
        );
        assert_eq!(
            effects.relations,
            vec![link(3, "EffectMiscValue", "npc", 5000)]
        );
        effects.count = MAX_RELATED;
        assert!(
            effects
                .accept(
                    5,
                    row(table::Value::UInt(9), 28, table::Value::UInt(9)),
                    &budget
                )
                .unwrap()
        );
        assert!(!effects.truncated);
        assert!(
            !effects
                .accept(
                    6,
                    row(table::Value::UInt(300), 1, table::Value::Null),
                    &budget
                )
                .unwrap()
        );
        assert!(effects.truncated);
    }
}

#[cfg(test)]
mod partial_tests;
