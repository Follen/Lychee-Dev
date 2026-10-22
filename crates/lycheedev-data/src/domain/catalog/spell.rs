use super::async_access::DomainAccess;
// SPDX-License-Identifier: MIT
// Frozen Go spell-info closure, including detail on depth-limited leaves.
use super::{Row, integer, text, unsigned};
use crate::{
    resource::{Budget, Cost},
    table,
};
use lycheedev_core::Result;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const MAX_EFFECTS: usize = 65536;

fn int_list(row: &Row, field: &str) -> Vec<i64> {
    match row.get(field) {
        Some(table::Value::Array(values)) => values
            .iter()
            .map(|value| match value {
                // Go scalarInt (array presentation) differs from rowInt: uint64
                // elements wrap to Windows' 64-bit int rather than becoming zero.
                table::Value::UInt(value) => *value as i64,
                value => integer(Some(value)),
            })
            .collect(),
        _ => Vec::new(),
    }
}
fn nullable_id(row: &Row, field: &str) -> Option<u32> {
    let id = unsigned(row.get(field));
    (id != 0).then_some(id)
}
fn first_id(row: &Row, field: &str) -> u32 {
    match row.get(field) {
        Some(table::Value::Array(values)) => unsigned(values.first()),
        value => unsigned(value),
    }
}
fn nullable_text<'a>(row: &'a Option<Row>, field: &str) -> Option<&'a str> {
    row.as_ref()
        .map(|row| text(row, field))
        .filter(|value| !value.is_empty())
}
fn effect(row: &Row) -> Value {
    json!({"effectIndex":integer(row.get("EffectIndex")),"effect":integer(row.get("Effect")),
        "effectAura":integer(row.get("EffectAura")),"effectTriggerSpell":nullable_id(row,"EffectTriggerSpell"),
        "effectAuraPeriod":nullable_id(row,"EffectAuraPeriod"),"effectBasePoints":integer(row.get("EffectBasePointsF")),
        "effectMechanic":integer(row.get("EffectMechanic")),"implicitTarget":int_list(row,"ImplicitTarget"),
        "effectRadiusIndex":int_list(row,"EffectRadiusIndex"),"effectMiscValue":int_list(row,"EffectMiscValue")})
}

/// The exact ASCII regexp `$@spellname(\d+)|\$(\d{5,})[a-zA-Z]`, scanned
/// without allocating all matches. Duplicate references still emit links.
fn references(text: &str, budget: &Budget, mut visit: impl FnMut(u32) -> Result<()>) -> Result<()> {
    let bytes = text.as_bytes();
    budget.charge(Cost::work(bytes.len().saturating_mul(2)))?;
    let mut at = 0;
    let mut next_check = 65536;
    while at < bytes.len() {
        // Bound cancellation latency even in a long non-matching description.
        if at >= next_check {
            budget.check()?;
            next_check = at + 65536;
        }
        if bytes[at] != b'$' {
            at += 1;
            continue;
        }
        let named = bytes[at + 1..].starts_with(b"@spellname");
        let start = at + if named { 11 } else { 1 };
        let mut end = start;
        let mut parsed = Some(0u32);
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            if end >= next_check {
                budget.check()?;
                next_check = end + 65536;
            }
            parsed = parsed.and_then(|value| {
                value
                    .checked_mul(10)?
                    .checked_add(u32::from(bytes[end] - b'0'))
            });
            end += 1;
        }
        let matched = if named {
            end > start
        } else {
            end - start >= 5 && bytes.get(end).is_some_and(u8::is_ascii_alphabetic)
        };
        if matched {
            if let Some(id) = parsed.filter(|id| *id != 0) {
                visit(id)?;
            }
            at = end + usize::from(!named);
        } else {
            at += 1;
        }
    }
    budget.check()
}

struct Closure {
    ids: BTreeSet<u32>,
    triggers: BTreeMap<u32, Vec<u32>>,
    descriptions: BTreeMap<u32, Vec<u32>>,
    effects: BTreeMap<u32, Vec<(u32, Row)>>,
}
impl Closure {
    fn add(
        &mut self,
        nav: &mut impl DomainAccess,
        parent: u32,
        child: u32,
        description: bool,
        next: &mut Vec<u32>,
    ) -> Result<()> {
        let edges = if description {
            &mut self.descriptions
        } else {
            &mut self.triggers
        };
        let previous = edges.get(&parent);
        nav.budget()
            .charge(Cost::work(previous.map_or(1, |ids| ids.len() + 1)))?;
        if previous.is_none_or(|ids| !ids.contains(&child)) {
            nav.budget().charge(Cost::retained(128))?;
            edges.entry(parent).or_default().push(child);
        }
        if !self.ids.contains(&child) {
            nav.budget().charge(Cost::retained(128))?;
            self.ids.insert(child);
            next.push(child);
        }
        Ok(())
    }
}
pub(crate) trait SpellAccess: DomainAccess + Sized {
    fn spell_link(
        &mut self,
        table: &str,
        field: &str,
        id: u32,
        kind: &str,
        target: u64,
        depth: u32,
    ) -> Result<()> {
        self.link(table, field, id, kind, target, depth)
    }
    async fn spell_effects(&mut self, spell: u32) -> Result<Vec<(u32, Row)>> {
        let (rows, extra) = self
            .scan("SpellEffect", Some(("SpellID", spell)), MAX_EFFECTS)
            .await?;
        if extra {
            self.mark_truncated("row_budget");
        }
        Ok(rows)
    }
    async fn spell_misc(&mut self, spell: u32) -> Result<Value> {
        // The first ascending record wins. Extra matches do not mean partial
        // or truncated in this Go domain contract.
        let (rows, _) = self.scan("SpellMisc", Some(("SpellID", spell)), 1).await?;
        let Some((id, row)) = rows.first() else {
            return Ok(Value::Null);
        };
        let icon = integer(row.get("SpellIconFileDataID"));
        if icon > 0 {
            self.spell_link(
                "SpellMisc",
                "SpellIconFileDataID",
                *id,
                "icon_file",
                icon as u64,
                1,
            )?;
        }
        let cast = self.lookup("SpellCastTimes", unsigned(row.get("CastingTimeIndex"))).await?
            .map(|row| json!({"base":integer(row.get("Base")),"minimum":integer(row.get("Minimum"))}));
        let duration = self.lookup("SpellDuration", unsigned(row.get("DurationIndex"))).await?
            .map(|row| json!({"duration":integer(row.get("Duration")),"maxDuration":integer(row.get("MaxDuration"))}));
        let range = self.lookup("SpellRange", unsigned(row.get("RangeIndex"))).await?
            .map(|row| json!({"displayName":text(&row,"DisplayName_lang"),"rangeMin":int_list(&row,"RangeMin"),"rangeMax":int_list(&row,"RangeMax")}));
        Ok(
            json!({"attributes":int_list(row,"Attributes"),"schoolMask":integer(row.get("SchoolMask")),
            "speed":integer(row.get("Speed")),"spellIconFileDataID":icon,"castTime":cast,"duration":duration,"range":range}),
        )
    }
    async fn spell_info(&mut self, seed: u32, max_depth: u32) -> Result<Value> {
        self.budget().charge(Cost::retained(256))?;
        let mut closure = Closure {
            ids: BTreeSet::from([seed]),
            triggers: BTreeMap::new(),
            descriptions: BTreeMap::new(),
            effects: BTreeMap::new(),
        };
        let mut frontier = vec![seed];
        let mut depth = 0;
        while !frontier.is_empty() && depth < max_depth {
            let mut next = Vec::new();
            for sid in frontier {
                self.budget().check()?;
                let effects = self.spell_effects(sid).await?;
                for (id, row) in &effects {
                    let trigger = unsigned(row.get("EffectTriggerSpell"));
                    let misc = if integer(row.get("Effect")) == 64 {
                        first_id(row, "EffectMiscValue")
                    } else {
                        0
                    };
                    for (child, field) in
                        [(trigger, "EffectTriggerSpell"), (misc, "EffectMiscValue")]
                    {
                        if child == 0
                            || child == sid
                            || (field == "EffectMiscValue" && child == trigger)
                        {
                            continue;
                        }
                        self.spell_link(
                            "SpellEffect",
                            field,
                            *id,
                            "spell",
                            u64::from(child),
                            depth,
                        )?;
                        closure.add(self, sid, child, false, &mut next)?;
                    }
                }
                self.budget().charge(Cost::retained(128))?;
                closure.effects.insert(sid, effects);
                if let Some(row) = self.lookup("Spell", sid).await? {
                    for field in ["Description_lang", "AuraDescription_lang"] {
                        let budget = self.budget_owner();
                        references(text(&row, field), &budget, |child| {
                            if child != sid {
                                self.spell_link(
                                    "Spell",
                                    field,
                                    sid,
                                    "spell",
                                    u64::from(child),
                                    depth,
                                )?;
                                closure.add(self, sid, child, true, &mut next)?;
                            }
                            Ok(())
                        })?;
                    }
                }
            }
            frontier = next;
            depth += 1;
        }
        if !frontier.is_empty() {
            self.mark_truncated("max_depth");
        }
        let mut parents: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for edges in [&closure.triggers, &closure.descriptions] {
            for (parent, children) in edges {
                for child in children {
                    self.budget().charge(Cost::work(1))?;
                    self.budget().charge(Cost::retained(128))?;
                    parents.entry(*child).or_default().insert(*parent);
                }
            }
        }
        let mut spells = serde_json::Map::new();
        for sid in &closure.ids {
            self.budget().check()?;
            let effects = match closure.effects.remove(sid) {
                Some(rows) => rows,
                None => self.spell_effects(*sid).await?,
            };
            self.budget().charge(Cost::retained(effects.len() * 1024))?;
            let mut payloads = Vec::with_capacity(effects.len());
            for (_, row) in &effects {
                self.budget().charge(Cost::work(1))?;
                payloads.push(effect(row));
            }
            let name = self.lookup("SpellName", *sid).await?;
            let description = self.lookup("Spell", *sid).await?;
            let misc = self.spell_misc(*sid).await?;
            self.budget().charge(Cost::retained(1024))?;
            let triggered_by: Vec<_> = parents
                .remove(sid)
                .unwrap_or_default()
                .into_iter()
                .collect();
            spells.insert(sid.to_string(),json!({"spellID":sid,"name":nullable_text(&name,"Name_lang"),
                "description":nullable_text(&description,"Description_lang"),"auraDescription":nullable_text(&description,"AuraDescription_lang"),
                "isSeed":*sid==seed,"triggeredBy":triggered_by,"misc":misc,"effects":payloads}));
        }
        Ok(
            json!({"spellID":seed,"seedCount":1,"totalCount":closure.ids.len(),"chainDepth":depth,
            "triggers":closure.triggers,"descRefs":closure.descriptions,"spells":spells}),
        )
    }
}

impl<T: DomainAccess> SpellAccess for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn reference_scanner_preserves_ascii_boundaries_duplicates_and_u32_range() {
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(10),
            None,
        );
        let mut found = Vec::new();
        references("$7922d $12345x $@spellname77 $@spellname77 $@spellname0 $@spellname4294967295 $4294967296z $@spellname4294967296 $00001Z $12345é $@SPELLNAME8 $@spellname١ $@spellname0002 $@spellname3tail",&budget,|id|{found.push(id);Ok(())}).unwrap();
        assert_eq!(found, vec![12345, 77, 77, u32::MAX, 1, 2, 3]);
    }
}
