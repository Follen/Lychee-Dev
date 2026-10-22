//! Domain presentation over current Engine-owned rows.
pub(crate) mod catalog;
use crate::{
    capture, fault,
    resource::{Budget, Cost},
    table,
};
use lycheedev_core::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;
const MAX_RELATED: usize = 65536;
pub(crate) type Row = BTreeMap<String, table::Value>;

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

/// Shared aura/summon presentation; reads are exclusively supplied by the caller.
pub(crate) async fn effects_domain(
    access: &mut impl catalog::async_access::DomainAccess,
    spell: u32,
    npc: u32,
    summon: bool,
) -> Result<Value> {
    let (rows, extra) = access
        .scan("SpellEffect", Some(("SpellID", spell)), MAX_RELATED)
        .await?;
    let mut effects = SpellEffects::new(spell, npc, summon);
    for (id, row) in rows {
        if !effects.accept(id, row, access.budget())? {
            break;
        }
    }
    if extra || effects.truncated {
        access.mark_truncated("row_budget");
    }
    for relation in &effects.relations {
        access.link(
            "SpellEffect",
            relation["fromField"].as_str().unwrap_or(""),
            relation["fromID"].as_u64().unwrap_or(0) as u32,
            relation["toKind"].as_str().unwrap_or(""),
            relation["toID"].as_u64().unwrap_or(0),
            0,
        )?;
    }
    Ok(effects.result())
}
