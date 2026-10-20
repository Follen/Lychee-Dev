use super::{async_access::{DomainAccess, immediately}, item_models::ItemModelAccess};
// Frozen Go item character-texture sections over the shared appearance path.
use super::{Navigator, integer, not_found, unsigned};
use crate::resource::Cost;
use lycheedev_core::Result;
use serde_json::{Value, json};

pub(super) const TABLES: &[&str] = &[
    "ItemSparse", "ItemModifiedAppearance", "ItemAppearance", "ItemDisplayInfo",
    "ItemDisplayInfoMaterialRes", "TextureFileData",
];

pub(crate) trait ItemTextureAccess: ItemModelAccess {
    async fn item_textures(&mut self, item: u32) -> Result<Value> {
        self.lookup("ItemSparse", item).await?.ok_or_else(not_found)?;
        let mut result = json!({"itemID":item,"displayID":0,"textures":[]});
        let Some((display, _)) = self.item_appearance(item).await? else { return Ok(result); };
        // Unlike item models/geosets, frozen textures stops on a zero display
        // identity even if that display row exists in the source.
        if display == 0 { return Ok(result); }
        let mut textures = Vec::new();
        for (_, row) in self.item_rows("ItemDisplayInfoMaterialRes", "ItemDisplayInfoID", display).await? {
            let material = unsigned(row.get("MaterialResourcesID"));
            if material == 0 { continue; }
            let file = self.item_texture_file(material).await?;
            if file == 0 { continue; }
            self.budget().charge(Cost::retained(256))?;
            textures.push(json!({"section":integer(row.get("ComponentSection")),"fileDataID":file}));
        }
        result["displayID"] = json!(display);
        result["textures"] = json!(textures);
        Ok(result)
    }
}

impl<T: ItemModelAccess> ItemTextureAccess for T {}
impl Navigator<'_> {
    pub(super) fn item_textures(&mut self, item: u32) -> Result<Value> { immediately(ItemTextureAccess::item_textures(self,item)) }
}
