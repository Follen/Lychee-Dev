//! Current Wago page receipt decoding within the original operation ledger.
use super::*;
pub(super) fn decode(raw: &[u8], ctx: &DataContext, budget: &Budget) -> Result<Receipt> {
    check_scope(ctx, budget)?;
    let _scratch = budget.reserve_retained(raw.len().checked_mul(3).ok_or_else(limit)?)?;
    let result = (|| {
        let receipt: Receipt = serde_json::from_slice(raw)
            .map_err(|error| super::error("cache_corrupt", 4, error.to_string()))?;
        chrono::DateTime::parse_from_rfc3339(&receipt.captured_at)
            .map_err(|_| error("cache_corrupt", 4, "invalid receipt timestamp"))?;
        let _: evidence::BlobRef = serde_json::from_value(receipt.response.clone())
            .map_err(|_| error("cache_corrupt", 4, "invalid response blob"))?;
        Ok(receipt)
    })();
    finish_scope(result, ctx, budget)
}
