//! Frozen Go display/model navigation over the shared authenticated navigator.
use super::{MAX_RELATIONS, Navigator, Row, integer, not_found, unsigned};
use crate::{resource::Cost, table};
use lycheedev_core::Result;
use serde_json::{Value, json};
use std::collections::BTreeSet;

const MAX_RELATED: usize = 65536;
fn variation(index: i64, value: i64) -> i64 {
    // Frozen Windows Go uses 64-bit int and defined wraparound on overflow.
    index.wrapping_add(1).wrapping_mul(100).wrapping_add(value)
}
fn textures(row: &Row) -> Vec<u32> {
    match row.get("TextureVariationFileDataID") {
        Some(table::Value::Array(values)) => values
            .iter()
            .map(|value| super::super::unsigned(Some(value)))
            .filter(|id| *id != 0)
            .collect(),
        _ => vec![],
    }
}
impl Navigator<'_> {
    fn creature_relation(
        &mut self,
        from: &str,
        field: &str,
        id: u32,
        kind: &str,
        target: u32,
        depth: u32,
    ) -> Result<()> {
        if self.relations.len() == MAX_RELATIONS {
            self.truncate("relation_budget");
            return Ok(());
        }
        self.budget.charge(Cost::retained(512))?;
        self.relations.push(json!({"fromTable":from,"fromField":field,"fromID":id,"toKind":kind,"toID":target,"depth":depth}));
        Ok(())
    }
    fn creature_rows(&mut self, table: &str, field: &str, wanted: u32) -> Result<Vec<(u32, Row)>> {
        let (rows, extra) = self.rows(table, Some((field, wanted)), MAX_RELATED)?;
        if extra {
            self.truncate("row_budget");
        }
        Ok(rows)
    }
    fn creature_variations(&mut self, display: u32) -> Result<Vec<i64>> {
        let rows = self.creature_rows(
            "CreatureDisplayInfoGeosetData",
            "CreatureDisplayInfoID",
            display,
        )?;
        Ok(rows
            .iter()
            .map(|(_, row)| {
                variation(
                    integer(row.get("GeosetIndex")),
                    integer(row.get("GeosetValue")),
                )
            })
            .collect())
    }
    fn creature_display_detail(&mut self, display: u32, row: &Row) -> Result<Value> {
        let model = unsigned(row.get("ModelID"));
        let file = if model == 0 {
            self.partial("relation_unresolved");
            0
        } else if let Some(row) = self.row("CreatureModelData", model)? {
            let file = unsigned(row.get("FileDataID"));
            if file != 0 {
                self.creature_relation(
                    "CreatureModelData",
                    "FileDataID",
                    model,
                    "model_file",
                    file,
                    1,
                )?;
            }
            file
        } else {
            self.partial("relation_unresolved");
            0
        };
        let variations = self.creature_variations(display)?;
        self.budget
            .charge(Cost::retained(1024 + variations.len() * 32))?;
        Ok(
            json!({"displayID":display,"modelID":model,"fileDataID":file,"modelFileDataID":file,"textures":textures(row),"variations":variations}),
        )
    }
    fn creature_models(&mut self, file: u32) -> Result<Vec<(u32, Row)>> {
        let rows = self.creature_rows("CreatureModelData", "FileDataID", file)?;
        if rows.len() > 1 {
            self.partial("relation_ambiguous");
        }
        Ok(rows)
    }
    fn creature_displays(&mut self, model: u32) -> Result<Vec<(u32, Row)>> {
        let mut rows = self.creature_rows("CreatureDisplayInfo", "ModelID", model)?;
        rows.sort_by_key(|(_, row)| unsigned(row.get("ID")));
        Ok(rows)
    }
    pub(super) fn creature_display(&mut self, display: u32, file: u32) -> Result<Value> {
        if display != 0 {
            let row = self
                .row("CreatureDisplayInfo", display)?
                .ok_or_else(not_found)?;
            return self.creature_display_detail(display, &row);
        }
        let models = self.creature_models(file)?;
        let (_, model) = models.first().ok_or_else(not_found)?;
        let displays = self.creature_displays(unsigned(model.get("ID")))?;
        let (_, selected) = displays.first().ok_or_else(not_found)?;
        for (id, row) in &displays {
            self.creature_relation(
                "CreatureDisplayInfo",
                "ModelID",
                *id,
                "creature_display",
                unsigned(row.get("ID")),
                0,
            )?;
        }
        self.creature_display_detail(unsigned(selected.get("ID")), selected)
    }
    pub(super) fn creature_model(&mut self, file: u32) -> Result<Value> {
        let models = self.creature_models(file)?;
        let mut seen = BTreeSet::new();
        let mut displays = Vec::new();
        for (_, model) in &models {
            self.budget.check()?;
            let model_id = unsigned(model.get("ID"));
            for (id, row) in self.creature_displays(model_id)? {
                let display = unsigned(row.get("ID"));
                if seen.contains(&display) {
                    continue;
                }
                self.budget.charge(Cost::retained(1152))?;
                seen.insert(display);
                let variations = self.creature_variations(display)?;
                self.budget.charge(Cost::retained(variations.len() * 32))?;
                self.creature_relation(
                    "CreatureDisplayInfo",
                    "ModelID",
                    id,
                    "creature_display",
                    display,
                    0,
                )?;
                displays.push(json!({"displayID":display,"modelID":model_id,"fileDataID":file,"textures":textures(&row),"variations":variations}));
            }
        }
        Ok(json!({"fileDataID":file,"displays":displays}))
    }
}

#[cfg(test)]
mod tests {
    use super::variation;
    #[test]
    fn geoset_arithmetic_matches_windows_go_int_wrapping() {
        assert_eq!(variation(i64::MAX, 0), 0);
        assert_eq!(variation(i64::MAX - 1, 0), -100);
        assert_eq!(variation(i64::MIN, 0), 100);
        assert_eq!(variation(0, i64::MAX), i64::MIN + 99);
    }
}
