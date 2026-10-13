//! Item display geosets and helmet-hide rows, preserving frozen Go ordering.
use super::{MAX_RELATIONS, Navigator, integer, item_models::int_list, not_found};
use crate::resource::Cost;
use lycheedev_core::Result;
use serde_json::{Value, json};

pub(super) const TABLES: &[&str] = &[
    "ItemSparse", "ItemModifiedAppearance", "ItemAppearance", "ItemDisplayInfo", "HelmetGeosetData",
];

impl Navigator<'_> {
    pub(super) fn item_geosets(&mut self, item: u32) -> Result<Value> {
        self.row("ItemSparse", item)?.ok_or_else(not_found)?;
        let mut result = json!({"itemID":item,"displayID":0,"geosetGroup":[],"helmetGeosetVis":[],"helmetHide":[]});
        let Some((display, row)) = self.item_appearance(item)? else { return Ok(result); };
        let geosets = int_list(&row, "GeosetGroup");
        let visibility = int_list(&row, "HelmetGeosetVis");
        self.budget.charge(Cost::retained(1024 + (geosets.len() + visibility.len()) * 32))?;
        let mut hide = Vec::new();
        for vis in &visibility {
            if *vis <= 0 { continue; }
            // The Go query casts its signed int selector to uint32, while the
            // relation keeps the original positive visibility value.
            for (id, row) in self.item_rows("HelmetGeosetData", "HelmetGeosetVisDataID", *vis as u32)? {
                if self.relations.len() == MAX_RELATIONS {
                    self.truncate("relation_budget");
                } else {
                    self.budget.charge(Cost::retained(512))?;
                    self.relations.push(json!({"fromTable":"HelmetGeosetData","fromField":"HelmetGeosetVisDataID","fromID":id,"toKind":"helmet_geoset_vis","toID":vis,"depth":2}));
                }
                self.budget.charge(Cost::retained(32))?;
                hide.push(integer(row.get("HideGeosetGroup")));
            }
        }
        result["displayID"] = json!(display);
        result["geosetGroup"] = json!(geosets);
        result["helmetGeosetVis"] = json!(visibility);
        result["helmetHide"] = json!(hide);
        Ok(result)
    }
}
