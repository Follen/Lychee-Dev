use super::async_access::DomainAccess;
// Frozen Go item appearance, model variant and material resolution.
use super::{Row, integer, not_found, unsigned};
use crate::{resource::Cost, table};
use lycheedev_core::Result;
use serde_json::{Value, json};

const MAX_RELATED: usize = 65536;

fn resources(row: &Row, field: &str) -> Vec<u32> {
    match row.get(field) {
        Some(table::Value::Array(values)) => values
            .iter()
            .map(|value| super::super::unsigned(Some(value)))
            .filter(|id| *id != 0)
            .collect(),
        _ => vec![],
    }
}
pub(super) fn int_list(row: &Row, field: &str) -> Vec<i64> {
    match row.get(field) {
        Some(table::Value::Array(values)) => values
            .iter()
            .map(|value| match value {
                table::Value::UInt(value) => *value as i64,
                value => integer(Some(value)),
            })
            .collect(),
        _ => vec![],
    }
}
// Domain presentation uses asynchronous reads from the current typed Engine.
// Row providers retain responsibility for physical IDs, ordering, early-stop,
// field validation, source evidence and admitted ownership.
pub(crate) trait ItemModelAccess: DomainAccess {
    fn item_link(
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
    async fn item_rows(&mut self, name: &str, field: &str, wanted: u32) -> Result<Vec<(u32, Row)>> {
        let (rows, extra) = self.scan(name, Some((field, wanted)), MAX_RELATED).await?;
        if extra {
            self.mark_truncated("row_budget");
        }
        Ok(rows)
    }
    async fn item_appearance(&mut self, item: u32) -> Result<Option<(u32, Row)>> {
        let appearances = self
            .item_rows("ItemModifiedAppearance", "ItemID", item)
            .await?;
        let Some((selected, row)) = appearances.first() else {
            return Ok(None);
        };
        if appearances.len() > 1 {
            self.mark_partial("relation_ambiguous");
        }
        // Go emits the nonselected candidates first, then the selected lowest ID.
        for (id, candidate) in &appearances[1..] {
            self.item_link(
                "ItemModifiedAppearance",
                "ItemAppearanceID",
                *id,
                "item_appearance",
                unsigned(candidate.get("ItemAppearanceID")),
                0,
            )?;
        }
        let appearance = unsigned(row.get("ItemAppearanceID"));
        self.item_link(
            "ItemModifiedAppearance",
            "ItemAppearanceID",
            *selected,
            "item_appearance",
            appearance,
            0,
        )?;
        let Some(row) = self.lookup("ItemAppearance", appearance).await? else {
            self.mark_partial("relation_unresolved");
            return Ok(None);
        };
        let display = unsigned(row.get("ItemDisplayInfoID"));
        self.item_link(
            "ItemAppearance",
            "ItemDisplayInfoID",
            appearance,
            "item_display",
            display,
            1,
        )?;
        let Some(row) = self.lookup("ItemDisplayInfo", display).await? else {
            self.mark_partial("relation_unresolved");
            return Ok(None);
        };
        Ok(Some((display, row)))
    }
    async fn item_model_file(
        &mut self,
        display: u32,
        resource: u32,
        race: i64,
        gender: i64,
    ) -> Result<u32> {
        let candidates = self
            .item_rows("ModelFileData", "ModelResourcesID", resource)
            .await?;
        self.item_link(
            "ItemDisplayInfo",
            "ModelResourcesID",
            display,
            "model_resource",
            resource,
            2,
        )?;
        let mut files = Vec::new();
        for (id, row) in candidates {
            let mut file = unsigned(row.get("FileDataID"));
            if file == 0 {
                file = unsigned(row.get("ID"));
            }
            if file != 0 {
                self.item_link("ModelFileData", "FileDataID", id, "model_file", file, 3)?;
                self.budget().charge(Cost::retained(8))?;
                files.push(file);
            }
        }
        let Some(first) = files.first().copied() else {
            return Ok(0);
        };
        if race == 0 && gender == 0 {
            return Ok(first);
        }
        for file in files {
            if let Some(component) = self.lookup("ComponentModelFileData", file).await?
                && integer(component.get("RaceID")) == race
                && integer(component.get("GenderIndex")) == gender
            {
                return Ok(file);
            }
        }
        self.mark_partial("variant_unavailable");
        Ok(first)
    }
    async fn item_texture_file(&mut self, material: u32) -> Result<u32> {
        let candidates = self
            .item_rows("TextureFileData", "MaterialResourcesID", material)
            .await?;
        for (id, row) in candidates {
            if unsigned(row.get("UsageType")) != 0 {
                continue;
            }
            let mut file = unsigned(row.get("FileDataID"));
            if file == 0 {
                file = unsigned(row.get("ID"));
            }
            if file != 0 {
                self.item_link(
                    "TextureFileData",
                    "MaterialResourcesID",
                    id,
                    "texture_file",
                    file,
                    2,
                )?;
                return Ok(file);
            }
        }
        Ok(0)
    }
    async fn item_models(&mut self, item: u32, race: i64, gender: i64) -> Result<Value> {
        self.lookup("ItemSparse", item)
            .await?
            .ok_or_else(not_found)?;
        let mut result = json!({"itemID":item,"raceID":race,"gender":gender,"displayID":0,"models":[],"textures":[],"geosetGroup":[]});
        let Some((display, row)) = self.item_appearance(item).await? else {
            return Ok(result);
        };
        let mut models = Vec::new();
        let mut textures = Vec::new();
        for resource in resources(&row, "ModelResourcesID") {
            let file = self
                .item_model_file(display, resource, race, gender)
                .await?;
            if file != 0 {
                models.push(file);
            }
        }
        for material in resources(&row, "ModelMaterialResourcesID") {
            let file = self.item_texture_file(material).await?;
            if file != 0 {
                textures.push(file);
            }
        }
        let geosets = int_list(&row, "GeosetGroup");
        self.budget().charge(Cost::retained(
            1024 + (models.len() + textures.len() + geosets.len()) * 32,
        ))?;
        result["displayID"] = json!(display);
        result["models"] = json!(models);
        result["textures"] = json!(textures);
        result["geosetGroup"] = json!(geosets);
        Ok(result)
    }
}

impl<T: DomainAccess> ItemModelAccess for T {}
