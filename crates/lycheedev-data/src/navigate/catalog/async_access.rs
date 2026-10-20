//! Shared mature algorithms consume owned Engine rows through this narrow seam.
use super::{Navigator, Row};
use crate::resource::{Budget, Cost};
use lycheedev_core::Result;
use std::sync::Arc;
pub(crate) trait DomainAccess {
    async fn lookup(&mut self, name: &str, id: u32) -> Result<Option<Row>>;
    async fn scan(&mut self, name: &str, filter: Option<(&str,u32)>, limit: usize) -> Result<(Vec<(u32,Row)>,bool)>;
    fn budget(&self)->&Budget;
    fn budget_owner(&self)->Arc<Budget>;
    fn mark_partial(&mut self, reason:&'static str);
    fn mark_truncated(&mut self, reason:&'static str);
    fn link(&mut self, table:&str, field:&str,id:u32,kind:&str,target:u64,depth:u32)->Result<()>;
}
impl DomainAccess for Navigator<'_> {
    async fn lookup(&mut self,name:&str,id:u32)->Result<Option<Row>> { self.row(name,id) }
    async fn scan(&mut self,name:&str,filter:Option<(&str,u32)>,limit:usize)->Result<(Vec<(u32,Row)>,bool)> { self.rows(name,filter,limit) }
    fn budget(&self)->&Budget { &self.budget }
    fn budget_owner(&self)->Arc<Budget> { self.budget.clone() }
    fn mark_partial(&mut self,r:&'static str) { self.partial(r); }
    fn mark_truncated(&mut self,r:&'static str) { self.truncate(r); }
    fn link(&mut self,table:&str,field:&str,id:u32,kind:&str,target:u64,depth:u32)->Result<()> {
        self.budget.check()?;
        if self.relations.len()==super::MAX_RELATIONS { self.truncate("relation_budget");return Ok(()); }
        self.budget.charge(Cost::retained(512))?;
        self.relations.push(serde_json::json!({"fromTable":table,"fromField":field,"fromID":id,"toKind":kind,"toID":target,"depth":depth}));Ok(())
    }
}
pub(super) fn immediately<T>(future:impl std::future::Future<Output=Result<T>>)->Result<T> {
    let mut future=std::pin::pin!(future);
    match future.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(result)=>result,
        std::task::Poll::Pending=>Err(crate::fault("data.domain.unexpected_async_legacy",8,"Synchronous domain adapter unexpectedly suspended"))
    }
}
