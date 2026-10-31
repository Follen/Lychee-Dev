//! Shared mature algorithms consume owned Engine rows through this narrow seam.
use super::Row;
use crate::resource::Budget;
use lycheedev_core::Result;
use std::sync::Arc;
pub(crate) trait DomainAccess {
    async fn lookup(&mut self, name: &str, id: u32) -> Result<Option<Row>>;
    async fn scan(
        &mut self,
        name: &str,
        filter: Option<(&str, u32)>,
        limit: usize,
    ) -> Result<(Vec<(u32, Row)>, bool)>;
    fn budget(&self) -> &Budget;
    fn budget_owner(&self) -> Arc<Budget>;
    fn complete(&self) -> bool;
    fn mark_partial(&mut self, reason: &'static str);
    fn mark_truncated(&mut self, reason: &'static str);
    fn link(
        &mut self,
        table: &str,
        field: &str,
        id: u32,
        kind: &str,
        target: u64,
        depth: u32,
    ) -> Result<()>;
}
