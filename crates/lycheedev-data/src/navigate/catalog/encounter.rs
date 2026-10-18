//! Frozen Go encounter tree semantics. Spell tables are demanded only by
//! reachable, depth-admitted spell references, in the same source session.
use super::{
    MAX_RELATIONS, Navigator, Row, admit_row, capture_bytes, check_source, integer, text, unsigned,
};
use crate::{
    capture, db2, fault,
    resource::{Budget, Cost},
};
use lycheedev_core::{
    Invocation, Outcome, Result,
    evidence::{self, CaptureInput},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_DEPTH: u32 = 32;
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
    fn unresolved(&mut self, nav: &mut Navigator<'_>, id: u32) -> Result<()> {
        nav.budget
            .charge(Cost::work(tree_work(self.unresolved_set.len())))?;
        if id != 0 && !self.unresolved_set.contains(&id) {
            nav.budget.charge(Cost::retained(128))?;
            self.unresolved_set.insert(id);
            self.unresolved.push(id);
            nav.partial("relation_unresolved");
        }
        Ok(())
    }
    fn children(&self, parent: u32, nav: &Navigator<'_>) -> Result<Vec<u32>> {
        nav.budget.check()?;
        nav.budget
            .charge(Cost::work(tree_work(self.parents.len())))?;
        let ids = self.parents.get(&parent).map(Vec::as_slice).unwrap_or(&[]);
        nav.budget.charge(Cost::work(ids.len()))?;
        nav.budget.charge(Cost::retained(ids.len() * 4))?;
        Ok(ids.to_vec())
    }
    fn build(&mut self, nav: &mut Navigator<'_>, id: u32, depth: u32) -> Result<Value> {
        nav.budget.check()?;
        nav.budget
            .charge(Cost::work(tree_work(self.active.len())))?;
        nav.budget.charge(Cost::retained(128))?;
        if !self.active.insert(id) {
            return Err(fault("records.relation_cycle", 4, "records.relation_cycle"));
        }
        let row = &self.nodes[&id];
        // A DAG may repeat a section in separate branches. Admit each emitted
        // copy, so the depth bound alone cannot hide exponential allocation.
        admit_row(&nav.budget, row)?;
        nav.budget.charge(Cost::retained(1024))?;
        let spell = unsigned(row.get("SpellID"));
        let display = unsigned(row.get("IconCreatureDisplayInfoID"));
        let first = unsigned(row.get("FirstChildSectionID"));
        let mut value = json!({"id":id,"title":text(row,"Title_lang"),"bodyText":text(row,"BodyText_lang"),"spellID":spell,"iconFlags":integer(row.get("IconFlags")),"type":integer(row.get("Type")),"difficultyMask":integer(row.get("DifficultyMask")),"iconCreatureDisplayInfoID":display,"orderIndex":integer(row.get("OrderIndex")) as u32,"parentSectionID":unsigned(row.get("ParentSectionID")),"firstChildSectionID":first,"nextSiblingSectionID":unsigned(row.get("NextSiblingSectionID"))});
        nav.budget
            .charge(Cost::work(tree_work(self.reachable.len())))?;
        self.reachable.insert(id);
        if spell != 0 {
            value["spellIDs"] = json!([spell]);
            nav.budget
                .charge(Cost::work(tree_work(self.spell_set.len())))?;
            if !self.spell_set.contains(&spell) {
                nav.budget.charge(Cost::retained(128))?;
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
                nav.budget.charge(Cost::work(tree_work(seen.len())))?;
                nav.budget.charge(Cost::retained(128))?;
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
                    nav.truncate("max_depth");
                    self.unresolved(nav, child)?;
                } else {
                    children.push(self.build(nav, child, depth + 1)?);
                }
            }
        } else {
            for child in self.children(id, nav)? {
                if depth + 1 > self.max_depth {
                    nav.truncate("max_depth");
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

impl Navigator<'_> {
    fn encounter_link(
        &mut self,
        table: &str,
        field: &str,
        id: u32,
        kind: &str,
        target: u32,
        depth: u32,
    ) -> Result<()> {
        self.budget.check()?;
        if self.relations.len() == MAX_RELATIONS {
            self.truncate("relation_budget");
            return Ok(());
        }
        self.budget.charge(Cost::retained(512))?;
        self.relations.push(json!({"fromTable":table,"fromField":field,"fromID":id,"toKind":kind,"toID":target,"depth":depth}));
        Ok(())
    }
    fn encounter_tree(&mut self, encounter: u32, max_depth: u32) -> Result<Value> {
        let (rows, extra) = self.rows(
            SECTION,
            Some(("JournalEncounterID", encounter)),
            MAX_SECTIONS,
        )?;
        if extra {
            self.truncate("row_budget");
        }
        self.budget.charge(Cost::retained(rows.len() * 256))?;
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
            self.budget.check()?;
            let id = unsigned(row.get("ID"));
            if id == 0 {
                self.partial("row_missing");
                continue;
            }
            tree.nodes.entry(id).or_insert(row);
        }
        for (id, row) in &tree.nodes {
            self.budget
                .charge(Cost::work(tree_work(tree.parents.len())))?;
            self.budget.charge(Cost::retained(128))?;
            tree.parents
                .entry(unsigned(row.get("ParentSectionID")))
                .or_default()
                .push(*id);
        }
        for ids in tree.parents.values_mut() {
            // Unique (order, ID) keys preserve the same stable ordering using
            // an in-place sort. Admit comparator work before starting it.
            self.budget.charge(Cost::work(
                ids.len() * tree_work(ids.len()) * tree_work(tree.nodes.len()),
            ))?;
            ids.sort_unstable_by_key(|id| (integer(tree.nodes[id].get("OrderIndex")) as u32, *id));
        }
        let mut sections = Vec::new();
        for id in tree.children(0, self)? {
            sections.push(tree.build(self, id, 0)?);
        }
        self.budget.charge(Cost::work(
            tree.nodes.len() * tree_work(tree.reachable.len()),
        ))?;
        self.budget.charge(Cost::retained(tree.nodes.len() * 4))?;
        let orphaned: Vec<_> = tree
            .nodes
            .keys()
            .filter(|id| !tree.reachable.contains(*id))
            .copied()
            .collect();
        for id in orphaned {
            self.budget.check()?;
            tree.unresolved(self, id)?;
        }
        let mut result = json!({"journalEncounterID":encounter,"sectionCount":tree.nodes.len(),"spellCount":tree.spells.len(),"spellIDs":tree.spells,"sections":[],"relatedSpells":[],"relatedFiles":[],"unresolvedSectionIDs":tree.unresolved});
        result["sections"] = Value::Array(sections);
        Ok(result)
    }
    fn encounter_spells(&mut self, result: &mut Value) -> Result<()> {
        let spells = result["spellIDs"].as_array().expect("spell array");
        let mut related = Vec::new();
        let mut files = Vec::new();
        for spell in spells {
            self.budget.check()?;
            let spell = spell.as_u64().expect("spell ID") as u32;
            let name = self.row("SpellName", spell)?;
            let name = name
                .as_ref()
                .map(|r| text(r, "Name_lang"))
                .filter(|s| !s.is_empty());
            let (misc, _) = self.rows("SpellMisc", Some(("SpellID", spell)), 1)?;
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
            self.budget.charge(Cost::retained(1024))?;
            related.push(json!({"spellID":spell,"name":name,"iconFileDataID":icon}));
        }
        result["relatedSpells"] = Value::Array(related);
        result["relatedFiles"] = Value::Array(files);
        Ok(())
    }
}

pub(super) fn dispatch(
    inv: &Invocation,
    after_read: &mut impl FnMut() -> Result<()>,
    finish: impl for<'a> FnOnce(crate::remote::provider::Scope<'a>, Result<Outcome>) -> Result<Outcome>,
) -> Result<Outcome> {
    dispatch_capturing(inv, after_read, finish, &mut |_| Ok(()))
}
fn dispatch_capturing(
    inv: &Invocation,
    after_read: &mut impl FnMut() -> Result<()>,
    finish: impl for<'a> FnOnce(crate::remote::provider::Scope<'a>, Result<Outcome>) -> Result<Outcome>,
    after_capture: &mut impl FnMut(&Value) -> Result<()>,
) -> Result<Outcome> {
    let encounter = super::identifier(inv, "--journal-encounter-id", false)?;
    let depth = inv
        .flag("--max-depth")
        .unwrap_or("0")
        .parse::<i64>()
        .map_err(|_| fault("command.invalid_arguments", 2, "invalid --max-depth"))?;
    if depth < 0 {
        return Err(fault("command.invalid_arguments", 2, "invalid --max-depth"));
    }
    if depth > i64::from(MAX_DEPTH) {
        return Err(fault("records.query_limit", 2, "records.query_limit"));
    }
    let depth = if depth == 0 { MAX_DEPTH } else { depth as u32 };
    let timeout = inv.usize_flag("--timeout-seconds", 300, 1, 3600)?;
    let budget = Arc::new(Budget::query_controlled(
        Instant::now() + Duration::from_secs(timeout as u64),
        inv.control.clone(),
    ));
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
            let mut state = None;
            let prepared = db2::prepare_following_scoped(
                inv,
                &[SECTION],
                &budget,
                Some(&mut *reader),
                Some(pin.clone()),
                &mut |batch| {
                    // Go completes every table's schema/WDC open before asking for the
                    // next table. Preserve error precedence as well as the success order.
                    let mut nav = Navigator::open(batch, budget.clone())?;
                    if state.is_some() {
                        return Ok(if batch[0].bundle.identity.name == "SpellName" {
                            vec!["SpellMisc"]
                        } else {
                            Vec::new()
                        });
                    }
                    let result = nav.encounter_tree(encounter, depth)?;
                    let spells = result["spellCount"] != 0;
                    state = Some((result, nav.relations, nav.partial, nav.truncation));
                    Ok(if spells {
                        vec!["SpellName"]
                    } else {
                        Vec::new()
                    })
                },
            )?;
            let (mut result, relations, partial, truncation) = state.expect("initial table batch");
            let mut nav = Navigator::open(&prepared, budget.clone())?;
            nav.relations = relations;
            for reason in partial {
                nav.partial(reason);
            }
            nav.truncation = truncation;
            if result["spellCount"] != 0 {
                nav.encounter_spells(&mut result)?;
            }
            result
                .as_object_mut()
                .expect("encounter object")
                .extend(nav.context()?.as_object().expect("context").clone());
            let pin = &prepared[0].pin;
            let complete = result["complete"] == true;
            let truncated = result["truncated"] == true;
            let manifest = json!({"operation":"encounter_get","root":format!("journal_encounter:{encounter}"),"snapshot":pin.id,"dataBuild":pin.data.as_ref().expect("DataPin").full_build,"relations":result["relations"],"relatedFiles":result["relatedFiles"],"complete":complete,"truncated":truncated,"truncationReason":result["truncationReason"]});
            after_read()?;
            let mut captures = Vec::new();
            for (value, kind, locator) in [
                (
                    &result,
                    "encounter-get",
                    format!("encounter:get:{encounter}"),
                ),
                (
                    &manifest,
                    "relation-manifest",
                    format!("encounter:relations:{encounter}"),
                ),
            ] {
                budget.check()?;
                check_source(inv, &budget, &*reader)?;
                let raw = capture_bytes(value, &budget)?;
                capture::admit_committed_identity(&budget)?;
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
                committed.push(
                    capture["id"]
                        .as_str()
                        .expect("committed encounter capture identity")
                        .to_owned(),
                );
                after_capture(&capture)?;
                captures.push(capture);
            }
            let mut outcome = Outcome::new(result);
            outcome.complete = complete;
            outcome.selection = serde_json::to_value(pin)?;
            outcome.provenance = captures;
            if truncated {
                outcome.warnings.push(json!("This result is truncated; preserve the snapshot and the stated truncation reason when continuing."));
            }
            if !nav.partial.is_empty() {
                outcome.warnings.push(json!("This result is only partially covered; inspect partialReasons for the unavailable tables and unresolved relations."));
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
mod tests {
    use super::*;

    #[test]
    fn encounter_rehashes_original_index_before_each_capture_and_keeps_first_commit() {
        use crate::capture::committed_tests::{fixture::Fixture, tables};
        use lycheedev_core::metadata::Metadata;
        use std::fs;

        let fx = Fixture::new(&tables());
        let inv = fx.invocation("data encounter get", &[("--journal-encounter-id", "132")]);
        let path = fx.client.parent().unwrap().join("Data/data/0000000001.idx");
        let original = fs::read(&path).unwrap();
        let mut reads = 0;
        let mut committed = Vec::new();
        let result = dispatch_capturing(
            &inv,
            &mut || {
                reads += 1;
                Ok(())
            },
            |scope, result| scope.finish(result),
            &mut |capture| {
                committed.push(capture.clone());
                assert_eq!(
                    committed.len(),
                    1,
                    "must not commit or replay a second capture"
                );
                let mut changed = original.clone();
                changed[40] ^= 1;
                assert_eq!(changed.len(), original.len());
                fs::write(&path, changed)?;
                Ok(())
            },
        );
        fs::write(&path, original).unwrap();
        let error = result.unwrap_err();
        assert_eq!(reads, 1);
        assert_eq!(committed.len(), 1);
        assert_eq!(error.code, "records.pinned_build_changed");
        let saved = &committed[0];
        let id = saved["id"].as_str().unwrap();
        assert_eq!(saved["provenance"]["kind"], "encounter-get");
        assert_eq!(error.details["completedCaptures"], json!([id]));
        assert_eq!(error.details["outputPublished"], false);
        assert_eq!(evidence::read_capture(&fx.home, id).unwrap(), *saved);
        let metadata = Metadata::open(&fx.home, false).unwrap();
        assert_eq!(metadata.list::<Value>("capture/", 100).unwrap(), committed);
        metadata.close().unwrap();
        let raw = evidence::read_blob(&fx.home, &saved["blob"], 16 << 20).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&raw).unwrap()["journalEncounterID"],
            132
        );
        assert_eq!(
            dispatch(&inv, &mut || Ok(()), |scope, result| scope.finish(result))
                .unwrap()
                .provenance
                .len(),
            2
        );
        fs::remove_dir_all(fx.root).unwrap();
    }

    #[test]
    fn encounter_keeps_first_capture_when_second_is_cancelled_and_final_close_fails() {
        use crate::capture::committed_tests::{fixture::Fixture, tables};
        use lycheedev_core::{
            control::{OperationControl, TrackedCancellation},
            metadata::Metadata,
            storage,
        };
        let fx = Fixture::new(&tables());
        let mut inv = fx.invocation("data encounter get", &[("--journal-encounter-id", "132")]);
        let cancel = TrackedCancellation::new();
        inv.control =
            OperationControl::tracked(Instant::now() + Duration::from_secs(60), cancel.clone());
        let close_error = fault(
            "vault.sqlite",
            4,
            "fixture close after first encounter capture",
        );
        let mut observed = None;
        let mut owner_error = None;
        let mut calls = 0;
        let error = dispatch_capturing(
            &inv,
            &mut || Ok(()),
            |scope, result| {
                assert_eq!(result.as_ref().unwrap_err().code, "command.cancelled");
                let result =
                    scope.finish_after_metadata_close(result, &mut || Err(close_error.clone()));
                owner_error = Some(result.as_ref().unwrap_err().clone());
                result
            },
            &mut |capture| {
                calls += 1;
                assert_eq!(
                    calls, 1,
                    "the second commit must not execute after cancellation"
                );
                observed = Some(capture.clone());
                cancel.cancel();
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        let saved = observed.unwrap();
        assert_eq!(saved["provenance"]["kind"], "encounter-get");
        let id = saved["id"].as_str().unwrap();
        assert_eq!(error.details["completedCaptures"], json!([id]));
        assert_eq!(error.details["outputPublished"], false);
        assert_eq!(
            error.details["primary"],
            serde_json::to_value(owner_error.unwrap()).unwrap()
        );
        assert_eq!(error.code, "command.cancelled");
        let primary = &error.details["primary"];
        assert_eq!(
            primary["details"]["schema"],
            "lycheedev.admitted-workspace-errors.v1"
        );
        assert_eq!(
            primary["details"]["causes"][0],
            serde_json::to_value(inv.control.check().unwrap_err()).unwrap()
        );
        assert_eq!(
            primary["details"]["causes"][1],
            serde_json::to_value(close_error).unwrap()
        );
        assert_eq!(primary["details"]["causes"].as_array().unwrap().len(), 2);
        assert_eq!(evidence::read_capture(&fx.home, id).unwrap(), saved);
        let raw = evidence::read_blob(&fx.home, &saved["blob"], 16 << 20).unwrap();
        assert_eq!(storage::sha256(&raw), saved["blob"]["sha256"]);
        assert_eq!(
            serde_json::from_slice::<Value>(&raw).unwrap()["journalEncounterID"],
            132
        );
        let metadata = Metadata::open(&fx.home, false).unwrap();
        assert_eq!(
            metadata.list::<Value>("capture/", 100).unwrap(),
            vec![saved]
        );
        metadata.close().unwrap();
        inv.control = OperationControl::default();
        let recovered = crate::navigate::dispatch(&inv).unwrap();
        assert_eq!(recovered.provenance.len(), 2);
        std::fs::remove_dir_all(fx.root).unwrap();
    }

    #[test]
    fn relation_limit_preserves_first_truncation_and_partial_is_independent() {
        let budget = Arc::new(Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(10),
            None,
        ));
        let mut nav = Navigator::open(&[], budget).unwrap();
        nav.relations = vec![Value::Null; MAX_RELATIONS - 1];
        nav.encounter_link(SECTION, "SpellID", 1, "spell", 7, 0)
            .unwrap();
        assert_eq!(nav.relations.len(), MAX_RELATIONS);
        assert!(nav.truncation.is_none());
        nav.encounter_link(SECTION, "SpellID", 2, "spell", 8, 0)
            .unwrap();
        assert_eq!(nav.relations.len(), MAX_RELATIONS);
        assert_eq!(nav.truncation, Some("relation_budget"));
        assert!(nav.partial.is_empty());
        nav.truncation = Some("row_budget");
        nav.encounter_link(SECTION, "SpellID", 3, "spell", 9, 0)
            .unwrap();
        assert_eq!(nav.truncation, Some("row_budget"));
    }

    #[test]
    fn relation_allocation_is_admitted_before_output_and_cancellation_is_checked_at_cap() {
        let limits = Cost {
            retained_bytes: 128,
            ..Cost::defaults()
        };
        let mut nav = Navigator::open(
            &[],
            Arc::new(Budget::new(
                limits,
                Instant::now() + Duration::from_secs(10),
                None,
            )),
        )
        .unwrap();
        assert_eq!(
            nav.encounter_link(SECTION, "SpellID", 1, "spell", 7, 0)
                .unwrap_err()
                .code,
            "records.query_resource_budget"
        );
        assert!(nav.relations.is_empty());
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut nav = Navigator::open(
            &[],
            Arc::new(Budget::new(
                Cost::defaults(),
                Instant::now() + Duration::from_secs(10),
                Some(cancel.clone()),
            )),
        )
        .unwrap();
        nav.relations = vec![Value::Null; MAX_RELATIONS];
        cancel.store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(
            nav.encounter_link(SECTION, "SpellID", 1, "spell", 7, 0)
                .unwrap_err()
                .exit_code,
            7
        );
        assert!(nav.truncation.is_none());
    }
}
