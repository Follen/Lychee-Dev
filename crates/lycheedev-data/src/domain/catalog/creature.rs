use super::async_access::DomainAccess;
// Frozen Go display/model navigation over the shared authenticated navigator.
use super::{Row, integer, not_found, unsigned};
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
pub(crate) trait CreatureAccess: DomainAccess {
    fn creature_relation(
        &mut self,
        from: &str,
        field: &str,
        id: u32,
        kind: &str,
        target: u32,
        depth: u32,
    ) -> Result<()> {
        self.link(from, field, id, kind, u64::from(target), depth)
    }
    async fn creature_rows(
        &mut self,
        table: &str,
        field: &str,
        wanted: u32,
    ) -> Result<Vec<(u32, Row)>> {
        let (rows, extra) = self.scan(table, Some((field, wanted)), MAX_RELATED).await?;
        if extra {
            self.mark_truncated("row_budget");
        }
        Ok(rows)
    }
    async fn creature_variations(&mut self, display: u32) -> Result<Vec<i64>> {
        let rows = self
            .creature_rows(
                "CreatureDisplayInfoGeosetData",
                "CreatureDisplayInfoID",
                display,
            )
            .await?;
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
    async fn creature_display_detail(&mut self, display: u32, row: &Row) -> Result<Value> {
        let model = unsigned(row.get("ModelID"));
        let file = if model == 0 {
            self.mark_partial("relation_unresolved");
            0
        } else if let Some(row) = self.lookup("CreatureModelData", model).await? {
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
            self.mark_partial("relation_unresolved");
            0
        };
        let variations = self.creature_variations(display).await?;
        self.budget()
            .charge(Cost::retained(1024 + variations.len() * 32))?;
        Ok(
            json!({"displayID":display,"modelID":model,"fileDataID":file,"modelFileDataID":file,"textures":textures(row),"variations":variations}),
        )
    }
    async fn creature_models(&mut self, file: u32) -> Result<Vec<(u32, Row)>> {
        let rows = self
            .creature_rows("CreatureModelData", "FileDataID", file)
            .await?;
        if rows.len() > 1 {
            self.mark_partial("relation_ambiguous");
        }
        Ok(rows)
    }
    async fn creature_displays(&mut self, model: u32) -> Result<Vec<(u32, Row)>> {
        let mut rows = self
            .creature_rows("CreatureDisplayInfo", "ModelID", model)
            .await?;
        rows.sort_by_key(|(_, row)| unsigned(row.get("ID")));
        Ok(rows)
    }
    async fn creature_display(&mut self, display: u32, file: u32) -> Result<Value> {
        if display != 0 {
            let row = self
                .lookup("CreatureDisplayInfo", display)
                .await?
                .ok_or_else(not_found)?;
            return self.creature_display_detail(display, &row).await;
        }
        let models = self.creature_models(file).await?;
        let (_, model) = models.first().ok_or_else(not_found)?;
        let displays = self.creature_displays(unsigned(model.get("ID"))).await?;
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
            .await
    }
    async fn creature_model(&mut self, file: u32) -> Result<Value> {
        let models = self.creature_models(file).await?;
        let mut seen = BTreeSet::new();
        let mut displays = Vec::new();
        for (_, model) in &models {
            self.budget().check()?;
            let model_id = unsigned(model.get("ID"));
            for (id, row) in self.creature_displays(model_id).await? {
                let display = unsigned(row.get("ID"));
                if seen.contains(&display) {
                    continue;
                }
                self.budget().charge(Cost::retained(1152))?;
                seen.insert(display);
                let variations = self.creature_variations(display).await?;
                self.budget()
                    .charge(Cost::retained(variations.len() * 32))?;
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

impl<T: DomainAccess> CreatureAccess for T {}

#[cfg(test)]
mod tests {
    use super::variation;
    #[test]
    fn geoset_arithmetic_matches_windows_retained_int_wrapping() {
        assert_eq!(variation(i64::MAX, 0), 0);
        assert_eq!(variation(i64::MAX - 1, 0), -100);
        assert_eq!(variation(i64::MIN, 0), 100);
        assert_eq!(variation(0, i64::MAX), i64::MIN + 99);
    }
}
