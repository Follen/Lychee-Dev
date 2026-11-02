use super::async_access::DomainAccess;
// Frozen Go encounter tree semantics. Spell tables are demanded only by
// reachable, depth-admitted spell references, in the same source session.
use super::{Row, admit_row, integer, text, unsigned};
use crate::{
    fault,
    resource::Cost,
};
use lycheedev_core::Result;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
};

const MAX_SECTIONS: usize = 10000;
const SECTION: &str = "JournalEncounterSection";

fn tree_work(size: usize) -> usize {
    // Account for lookup plus insertion/comparison over bounded ordered sets.
    2 * (usize::BITS - size.leading_zeros()).max(1) as usize
}

struct Tree {
    nodes: BTreeMap<u32, Row>,
    parents: BTreeMap<u32, Vec<u32>>,
    active: BTreeSet<u32>,
    reachable: BTreeSet<u32>,
    unresolved: Vec<u32>,
    unresolved_set: BTreeSet<u32>,
    spells: Vec<u32>,
    spell_set: BTreeSet<u32>,
    max_depth: u32,
}
impl Tree {
    fn unresolved(&mut self, nav: &mut impl EncounterAccess, id: u32) -> Result<()> {
        nav.budget()
            .charge(Cost::work(tree_work(self.unresolved_set.len())))?;
        if id != 0 && !self.unresolved_set.contains(&id) {
            nav.budget().charge(Cost::retained(128))?;
            self.unresolved_set.insert(id);
            self.unresolved.push(id);
            nav.mark_partial("relation_unresolved");
        }
        Ok(())
    }
    fn children(&self, parent: u32, nav: &impl EncounterAccess) -> Result<Vec<u32>> {
        nav.budget().check()?;
        nav.budget()
            .charge(Cost::work(tree_work(self.parents.len())))?;
        let ids = self.parents.get(&parent).map(Vec::as_slice).unwrap_or(&[]);
        nav.budget().charge(Cost::work(ids.len()))?;
        nav.budget().charge(Cost::retained(ids.len() * 4))?;
        Ok(ids.to_vec())
    }
    fn build(&mut self, nav: &mut impl EncounterAccess, id: u32, depth: u32) -> Result<Value> {
        nav.budget().check()?;
        nav.budget()
            .charge(Cost::work(tree_work(self.active.len())))?;
        nav.budget().charge(Cost::retained(128))?;
        if !self.active.insert(id) {
            return Err(fault("records.relation_cycle", 4, "records.relation_cycle"));
        }
        let row = &self.nodes[&id];
        // A DAG may repeat a section in separate branches. Admit each emitted
        // copy, so the depth bound alone cannot hide exponential allocation.
        admit_row(&nav.budget(), row)?;
        nav.budget().charge(Cost::retained(1024))?;
        let spell = unsigned(row.get("SpellID"));
        let display = unsigned(row.get("IconCreatureDisplayInfoID"));
        let first = unsigned(row.get("FirstChildSectionID"));
        let mut value = json!({"id":id,"title":text(row,"Title_lang"),"bodyText":text(row,"BodyText_lang"),"spellID":spell,"iconFlags":integer(row.get("IconFlags")),"type":integer(row.get("Type")),"difficultyMask":integer(row.get("DifficultyMask")),"iconCreatureDisplayInfoID":display,"orderIndex":integer(row.get("OrderIndex")) as u32,"parentSectionID":unsigned(row.get("ParentSectionID")),"firstChildSectionID":first,"nextSiblingSectionID":unsigned(row.get("NextSiblingSectionID"))});
        nav.budget()
            .charge(Cost::work(tree_work(self.reachable.len())))?;
        self.reachable.insert(id);
        if spell != 0 {
            value["spellIDs"] = json!([spell]);
            nav.budget()
                .charge(Cost::work(tree_work(self.spell_set.len())))?;
            if !self.spell_set.contains(&spell) {
                nav.budget().charge(Cost::retained(128))?;
                self.spell_set.insert(spell);
                self.spells.push(spell);
            }
            nav.encounter_link(SECTION, "SpellID", id, "spell", spell, depth)?;
        }
        if display != 0 {
            nav.encounter_link(
                SECTION,
                "IconCreatureDisplayInfoID",
                id,
                "creature_display",
                display,
                depth,
            )?;
        }
        let mut children = Vec::new();
        if first != 0 {
            let mut chain = Vec::new();
            let mut seen = BTreeSet::new();
            let mut cursor = first;
            let mut field = "FirstChildSectionID";
            while cursor != 0 {
                nav.budget().charge(Cost::work(tree_work(seen.len())))?;
                nav.budget().charge(Cost::retained(128))?;
                if !seen.insert(cursor) {
                    return Err(fault("records.relation_cycle", 4, "records.relation_cycle"));
                }
                chain.push(cursor);
                // Go attributes every sibling-chain link to the owning parent.
                nav.encounter_link(SECTION, field, id, "section", cursor, depth + 1)?;
                let Some(child) = self.nodes.get(&cursor) else {
                    self.unresolved(nav, cursor)?;
                    break;
                };
                cursor = unsigned(child.get("NextSiblingSectionID"));
                field = "NextSiblingSectionID";
            }
            for child in chain {
                if !self.nodes.contains_key(&child) {
                    continue;
                }
                if depth + 1 > self.max_depth {
                    nav.mark_truncated("max_depth");
                    self.unresolved(nav, child)?;
                } else {
                    children.push(self.build(nav, child, depth + 1)?);
                }
            }
        } else {
            for child in self.children(id, nav)? {
                if depth + 1 > self.max_depth {
                    nav.mark_truncated("max_depth");
                    self.unresolved(nav, child)?;
                    continue;
                }
                nav.encounter_link(SECTION, "ParentSectionID", child, "section", id, depth + 1)?;
                children.push(self.build(nav, child, depth + 1)?);
            }
        }
        if !children.is_empty() {
            value["children"] = Value::Array(children);
        }
        self.active.remove(&id);
        Ok(value)
    }
}

pub(crate) trait EncounterAccess: DomainAccess + Sized {
    fn encounter_link(
        &mut self,
        table: &str,
        field: &str,
        id: u32,
        kind: &str,
        target: u32,
        depth: u32,
    ) -> Result<()> {
        self.link(table, field, id, kind, u64::from(target), depth)
    }
    async fn encounter_tree(&mut self, encounter: u32, max_depth: u32) -> Result<Value> {
        let (rows, extra) = self
            .scan(
                SECTION,
                Some(("JournalEncounterID", encounter)),
                MAX_SECTIONS,
            )
            .await?;
        self.encounter_tree_rows(rows, extra, encounter, max_depth)
    }
    fn encounter_tree_rows(
        &mut self,
        rows: Vec<(u32, Row)>,
        extra: bool,
        encounter: u32,
        max_depth: u32,
    ) -> Result<Value> {
        if extra {
            self.mark_truncated("row_budget");
        }
        self.budget().charge(Cost::retained(rows.len() * 256))?;
        let mut tree = Tree {
            nodes: BTreeMap::new(),
            parents: BTreeMap::new(),
            active: BTreeSet::new(),
            reachable: BTreeSet::new(),
            unresolved: Vec::new(),
            unresolved_set: BTreeSet::new(),
            spells: Vec::new(),
            spell_set: BTreeSet::new(),
            max_depth,
        };
        for (_, row) in rows {
            self.budget().check()?;
            let id = unsigned(row.get("ID"));
            if id == 0 {
                self.mark_partial("row_missing");
                continue;
            }
            tree.nodes.entry(id).or_insert(row);
        }
        for (id, row) in &tree.nodes {
            self.budget()
                .charge(Cost::work(tree_work(tree.parents.len())))?;
            self.budget().charge(Cost::retained(128))?;
            tree.parents
                .entry(unsigned(row.get("ParentSectionID")))
                .or_default()
                .push(*id);
        }
        for ids in tree.parents.values_mut() {
            // Unique (order, ID) keys preserve the same stable ordering using
            // an in-place sort. Admit comparator work before starting it.
            self.budget().charge(Cost::work(
                ids.len() * tree_work(ids.len()) * tree_work(tree.nodes.len()),
            ))?;
            ids.sort_unstable_by_key(|id| (integer(tree.nodes[id].get("OrderIndex")) as u32, *id));
        }
        let mut sections = Vec::new();
        for id in tree.children(0, self)? {
            sections.push(tree.build(self, id, 0)?);
        }
        self.budget().charge(Cost::work(
            tree.nodes.len() * tree_work(tree.reachable.len()),
        ))?;
        self.budget().charge(Cost::retained(tree.nodes.len() * 4))?;
        let orphaned: Vec<_> = tree
            .nodes
            .keys()
            .filter(|id| !tree.reachable.contains(*id))
            .copied()
            .collect();
        for id in orphaned {
            self.budget().check()?;
            tree.unresolved(self, id)?;
        }
        let mut result = json!({"journalEncounterID":encounter,"sectionCount":tree.nodes.len(),"spellCount":tree.spells.len(),"spellIDs":tree.spells,"sections":[],"relatedSpells":[],"relatedFiles":[],"unresolvedSectionIDs":tree.unresolved});
        result["sections"] = Value::Array(sections);
        Ok(result)
    }
    async fn encounter_spells(&mut self, result: &mut Value) -> Result<()> {
        let spells = result["spellIDs"].as_array().expect("spell array");
        let mut related = Vec::new();
        let mut files = Vec::new();
        for spell in spells {
            self.budget().check()?;
            let spell = spell.as_u64().expect("spell ID") as u32;
            let name = self.lookup("SpellName", spell).await?;
            let name = name
                .as_ref()
                .map(|r| text(r, "Name_lang"))
                .filter(|s| !s.is_empty());
            let (misc, _) = self.scan("SpellMisc", Some(("SpellID", spell)), 1).await?;
            let mut icon = 0;
            if let Some((id, row)) = misc.first() {
                let signed_icon = integer(row.get("SpellIconFileDataID"));
                if signed_icon > 0 {
                    icon = signed_icon as u32;
                    self.encounter_link(
                        "SpellMisc",
                        "SpellIconFileDataID",
                        *id,
                        "icon_file",
                        icon,
                        1,
                    )?;
                    files.push(json!({"kind":"icon_file","fileDataID":icon,"ownerKind":"spell","ownerID":spell}));
                }
            }
            self.budget().charge(Cost::retained(1024))?;
            related.push(json!({"spellID":spell,"name":name,"iconFileDataID":icon}));
        }
        result["relatedSpells"] = Value::Array(related);
        result["relatedFiles"] = Value::Array(files);
        Ok(())
    }
    async fn encounter_get(&mut self, encounter: u32, max_depth: u32) -> Result<Value> {
        let mut result = self.encounter_tree(encounter, max_depth).await?;
        if result["spellCount"].as_u64().unwrap_or(0) != 0 {
            self.encounter_spells(&mut result).await?;
        }
        Ok(result)
    }
}

impl<T: DomainAccess> EncounterAccess for T {}
