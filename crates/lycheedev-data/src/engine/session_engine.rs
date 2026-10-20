//! One session, one mature execution kernel. No legacy-executor fallback.
use super::{
    api::*,
    error,
    lifetime::{RequestLifetime, SessionMemory},
    plan, provider,
    result::ResultOwner,
    source::{self, PreparedSnapshot},
};
use crate::fault;
use arrow_array::RecordBatch;
use datafusion::datasource::MemTable;
use futures_util::StreamExt;
use lycheedev_core::{Invocation, Result, control::OperationControl};
use std::sync::Arc;

struct Cached {
    snapshot: Arc<PreparedSnapshot>,
    lifetime: Arc<RequestLifetime>,
}
pub struct Engine {
    invocation: Invocation,
    limits: EngineLimits,
    memory: SessionMemory,
    cache: Vec<Cached>,
    pinned: Option<(String, Option<lycheedev_core::selection::DataPin>)>,
}
impl Engine {
    pub fn new(invocation: Invocation, limits: EngineLimits) -> Result<Self> {
        if limits.query_work == 0
            || limits.operator_bytes == 0
            || limits.operator_bytes > 128 << 20
            || limits.result_bytes == 0
            || limits.page_bytes == 0
            || limits.page_bytes > limits.session_retained_bytes
        {
            return Err(bound());
        }
        Ok(Self {
            invocation,
            limits,
            memory: SessionMemory::new(limits.session_retained_bytes)?,
            cache: Vec::new(),
            pinned: None,
        })
    }
    async fn source(
        &mut self,
        request: &TableRequest,
        lifetime: &Arc<RequestLifetime>,
        control: &OperationControl,
    ) -> Result<(Arc<PreparedSnapshot>, Arc<RequestLifetime>, bool)> {
        if let Some(cached) = self
            .cache
            .iter()
            .find(|entry| entry.snapshot.table() == request)
        {
            control.check()?;
            return Ok((cached.snapshot.clone(), cached.lifetime.clone(), false));
        }
        let mut invocation = self.invocation.clone();
        invocation.control = control.clone();
        let prepared = source::prepare(
            &invocation,
            request,
            lifetime.budget.clone(),
            control.clone(),
        )
        .await?;
        if let Some((id, data)) = &self.pinned {
            if id != &prepared.pin().id || data != &prepared.pin().data {
                return Err(fault(
                    "records.source_identity",
                    3,
                    "Source differs from the session immutable snapshot",
                ));
            }
        } else {
            self.invocation
                .flags
                .insert("--snapshot".into(), prepared.pin().id.clone());
            self.pinned = Some((prepared.pin().id.clone(), prepared.pin().data.clone()));
        }
        Ok((prepared, lifetime.clone(), true))
    }
    pub fn close(&mut self) -> Result<()> {
        self.cache.clear();
        Ok(())
    }
    pub async fn discover(
        &mut self,
        table: TableRequest,
        control: OperationControl,
    ) -> Result<DatasetInfo> {
        let lifetime = self.memory.begin(self.limits.query_work, control.clone())?;
        let (snapshot, _, new) = self.source(&table, &lifetime, &control).await?;
        let mut info = source::describe(&snapshot, &lifetime.budget)?;
        info._lifetime = Some(lifetime.clone());
        lifetime.settle()?;
        if new {
            self.cache.push(Cached { snapshot, lifetime });
        }
        Ok(info)
    }
    pub async fn execute(
        &mut self,
        request: QueryRequest,
        control: OperationControl,
    ) -> Result<ResultOwner> {
        control.check()?;
        let QueryRequest::Plan { plan: query_plan } = &request;
        let tables = plan::tables(query_plan).map_err(error)?;
        let lifetime = self.memory.begin(self.limits.query_work, control.clone())?;
        let (context, owner) =
            provider::runtime::context(lifetime.budget.clone(), self.limits.operator_bytes, 1024)
                .map_err(error)?;
        let mut pending = Vec::new();
        let mut sources = Vec::new();
        let mut lifetimes = vec![lifetime.clone()];
        let mut aliases = std::collections::BTreeSet::new();
        for table in &tables {
            let alias = plan::table_name(table).map_err(error)?;
            if !aliases.insert(alias.clone()) {
                return Err(fault(
                    "records.source_identity",
                    3,
                    "Duplicate source alias",
                ));
            }
            let (snapshot, retained, new) = self.source(table, &lifetime, &control).await?;
            let provider = Arc::new(
                MemTable::try_new(snapshot.schema(), vec![snapshot.batches().to_vec()])
                    .map_err(error)?,
            );
            context
                .register_table(
                    datafusion::common::TableReference::bare(alias),
                    provider.clone(),
                )
                .map_err(error)?;
            lifetimes.push(retained);
            if new {
                pending.push(snapshot.clone());
            }
            sources.push(snapshot);
        }
        let frame = plan::lower(&context, query_plan, owner.clone())
            .await
            .map_err(error)?;
        // Admit result arrays, vector growth, names and provenance before any
        // owned output copy. Native operator temporaries are host-cap bounded.
        let mut output = lifetime
            .budget
            .reserve_retained(self.limits.result_bytes)?
            .into_owned(&lifetime.budget)?;
        let mut metadata_bytes = 4096usize;
        for field in frame.schema().fields() {
            metadata_bytes = metadata_bytes
                .checked_add(field.name().len())
                .and_then(|n| n.checked_add(256))
                .ok_or_else(bound)?;
        }
        for source in &sources {
            let size = crate::capture::json_size(source.provenance(), &lifetime.budget)?;
            metadata_bytes = metadata_bytes
                .checked_add(size.checked_mul(8).ok_or_else(bound)?)
                .ok_or_else(bound)?;
        }
        if metadata_bytes > self.limits.result_bytes {
            return Err(bound());
        }
        let columns: Vec<String> = frame
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect();
        let mut stream = frame.execute_stream().await.map_err(error)?;
        let mut batches: Vec<RecordBatch> = Vec::new();
        let (mut bytes, mut rows) = (metadata_bytes, 0usize);
        while let Some(next) = stream.next().await {
            control.check()?;
            owner.check().map_err(error)?;
            let batch = next.map_err(error)?;
            bytes = bytes
                .checked_add(batch.get_array_memory_size())
                .and_then(|v| v.checked_add(256))
                .ok_or_else(bound)?;
            rows = rows.checked_add(batch.num_rows()).ok_or_else(bound)?;
            if bytes > self.limits.result_bytes {
                return Err(bound());
            }
            batches.push(batch);
        }
        control.check()?;
        drop(stream);
        drop(context);
        drop(owner);
        let provenance = serde_json::json!({"engine":"datafusion", "sources":sources.iter().map(|s|s.provenance()).collect::<Vec<_>>(), "complete":sources.iter().all(|s|s.complete())});
        let complete = sources.iter().all(|s| s.complete());
        output.shrink_to(bytes)?;
        lifetime.settle()?;
        for snapshot in pending {
            self.cache.push(Cached {
                snapshot,
                lifetime: lifetime.clone(),
            });
        }
        Ok(ResultOwner {
            batches,
            columns,
            provenance,
            provenance_bytes: metadata_bytes,
            complete,
            rows,
            bytes,
            session: self.memory.budget.clone(),
            page_bytes: self.limits.page_bytes,
            _lifetimes: lifetimes,
            _output: Some(output),
        })
    }
}
fn bound() -> lycheedev_core::ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Unified query exceeds its admitted bound",
    )
}
