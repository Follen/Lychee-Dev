//! Shared item and decor business presentation, with Engine-owned async reads.
use super::{
    MAX_DECOR, async_access::DomainAccess, decor, integer, not_found, slot_name, text, unsigned,
};
use lycheedev_core::Result;
use serde_json::{Value, json};
pub(crate) trait BasicAccess: DomainAccess {
    async fn item_get(&mut self, id: u32) -> Result<Value> {
        let sparse = self.lookup("ItemSparse", id).await?.ok_or_else(not_found)?;
        let mut item = json!({"id":id,"name":text(&sparse,"Display_lang"),"inventoryType":integer(sparse.get("InventoryType")),"quality":integer(sparse.get("OverallQualityID")),"slotName":slot_name(integer(sparse.get("InventoryType"))),"classID":0,"subclassID":0});
        if let Some(class) = self.lookup("Item", id).await? {
            item["classID"] = json!(integer(class.get("ClassID")));
            item["subclassID"] = json!(integer(class.get("SubclassID")));
        } else {
            self.mark_partial("row_missing");
        }
        Ok(json!({"item":item}))
    }
    async fn decor_list(&mut self, limit: usize) -> Result<Value> {
        let (rows, extra) = self.scan("HouseDecor", None, limit).await?;
        if extra {
            self.mark_truncated("limit_reached");
        }
        let items: Vec<_> = rows.iter().map(|(_, row)| decor(row)).collect();
        Ok(json!({"count":items.len(),"items":items}))
    }
    async fn decor_get(&mut self, field: &str, wanted: u32) -> Result<Value> {
        let (rows, extra) = self
            .scan("HouseDecor", Some((field, wanted)), MAX_DECOR)
            .await?;
        if extra {
            self.mark_truncated("row_budget");
        }
        let (_, first) = rows.first().ok_or_else(not_found)?;
        if rows.len() > 1 {
            self.mark_partial("relation_ambiguous");
        }
        for (id, row) in &rows {
            self.link(
                "HouseDecor",
                field,
                *id,
                "decor",
                u64::from(unsigned(row.get("ID"))),
                0,
            )?;
        }
        Ok(json!({"item":decor(first)}))
    }
}
impl<T: DomainAccess> BasicAccess for T {}
