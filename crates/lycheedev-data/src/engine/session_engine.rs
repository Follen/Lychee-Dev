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
        ids: Option<&[u32]>,
        lifetime: &Arc<RequestLifetime>,
        control: &OperationControl,
    ) -> Result<(Arc<PreparedSnapshot>, Arc<RequestLifetime>, bool)> {
        for cached in &self.cache {
            control.check()?;
            if cached.snapshot.table() == request
                && cached.snapshot.covers(ids, &lifetime.budget)?
            {
                return Ok((cached.snapshot.clone(), cached.lifetime.clone(), false));
            }
        }
        let mut invocation = self.invocation.clone();
        invocation.control = control.clone();
        let prepared = source::prepare_selected(
            &invocation,
            request,
            ids,
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
    fn install_source(
        &mut self,
        snapshot: Arc<PreparedSnapshot>,
        lifetime: Arc<RequestLifetime>,
    ) -> Result<()> {
        // At most one cached preparation per exact table. Live results retain
        // their Arrow admission owners independently of this cache entry.
        let mut index = 0;
        while index < self.cache.len() {
            if self.cache[index].snapshot.table() == snapshot.table() {
                let Cached {
                    snapshot: old,
                    lifetime: old_lifetime,
                } = self.cache.remove(index);
                drop(old);
                old_lifetime.settle()?;
            } else {
                index += 1;
            }
        }
        self.cache.push(Cached { snapshot, lifetime });
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn cached_source_count(&self) -> usize {
        self.cache.len()
    }
    pub fn close(&mut self) -> Result<()> {
        let mut first_error = None;
        while let Some(Cached { snapshot, lifetime }) = self.cache.pop() {
            drop(snapshot);
            if let Err(error) = lifetime.settle()
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    pub async fn catalog(&mut self, control: OperationControl) -> Result<DatasetCatalog> {
        let lifetime = self.memory.begin(self.limits.query_work, control.clone())?;
        let mut invocation = self.invocation.clone();
        invocation.control = control;
        let (mut catalog, pin) = super::catalog::read(&invocation, &lifetime.budget)?;
        if let Some((id, data)) = &self.pinned {
            if id != &pin.id || data != &pin.data {
                return Err(fault(
                    "records.source_identity",
                    3,
                    "Catalog differs from the session immutable snapshot",
                ));
            }
        } else {
            self.invocation
                .flags
                .insert("--snapshot".into(), pin.id.clone());
            self.pinned = Some((pin.id, pin.data));
        }
        catalog._lifetime = Some(lifetime.clone());
        lifetime.settle()?;
        Ok(catalog)
    }
    pub async fn discover(
        &mut self,
        table: TableRequest,
        control: OperationControl,
    ) -> Result<DatasetInfo> {
        let lifetime = self.memory.begin(self.limits.query_work, control.clone())?;
        let (snapshot, _, new) = self.source(&table, None, &lifetime, &control).await?;
        let mut info = source::describe(&snapshot, &lifetime.budget)?;
        info._lifetime = Some(lifetime.clone());
        lifetime.settle()?;
        if new {
            self.install_source(snapshot, lifetime)?;
        }
        Ok(info)
    }
    pub fn capture(
        &self,
        result: &ResultOwner,
        control: OperationControl,
    ) -> Result<super::OwnedCapture> {
        if !Arc::ptr_eq(&result.session, &self.memory.budget) {
            return Err(fault(
                "records.source_identity",
                3,
                "Result belongs to a different session",
            ));
        }
        let lifetime = self.memory.begin(self.limits.query_work, control.clone())?;
        super::evidence::capture(&self.invocation, result, lifetime, control)
    }
    pub async fn execute(
        &mut self,
        request: QueryRequest,
        control: OperationControl,
    ) -> Result<ResultOwner> {
        control.check()?;
        let QueryRequest::Plan { plan: query_plan } = &request;
        // Whole-plan admission remains before source I/O.
        let lifetime = self.memory.begin(self.limits.query_work, control.clone())?;
        let (context, owner) =
            provider::runtime::context(lifetime.budget.clone(), self.limits.operator_bytes, 1024)
                .map_err(error)?;
        let requirements = plan::requirements(query_plan, &owner).map_err(error)?;
        let mut pending = Vec::new();
        let mut sources = Vec::new();
        let mut lifetimes = vec![lifetime.clone()];
        let mut aliases = std::collections::BTreeSet::new();
        for requirement in &requirements {
            let table = &requirement.table;
            let alias = plan::table_name(table).map_err(error)?;
            if !aliases.insert(alias.clone()) {
                return Err(fault(
                    "records.source_identity",
                    3,
                    "Duplicate source alias",
                ));
            }
            let (snapshot, retained, new) = self
                .source(table, requirement.ids.as_deref(), &lifetime, &control)
                .await?;
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
        let mut lookup_bytes = 4096usize;
        for requirement in &requirements {
            if let Some(ids) = &requirement.ids {
                let table_bytes = crate::capture::json_size(&requirement.table, &lifetime.budget)?;
                lookup_bytes = lookup_bytes
                    .checked_add(ids.len().checked_mul(256).ok_or_else(bound)?)
                    .and_then(|n| n.checked_add(table_bytes.checked_mul(8)?))
                    .and_then(|n| n.checked_add(8192))
                    .ok_or_else(bound)?;
            }
        }
        owner.retain(lookup_bytes).map_err(error)?;
        let mut lookup_reports = Vec::new();
        for (requirement, snapshot) in requirements.iter().zip(&sources) {
            if let Some(ids) = &requirement.ids {
                lookup_reports.push(snapshot.lookup_report(ids, &lifetime.budget)?);
            }
        }
        drop(requirements);
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
        metadata_bytes = metadata_bytes
            .checked_add(
                crate::capture::json_size(&lookup_reports, &lifetime.budget)?
                    .checked_mul(8)
                    .ok_or_else(bound)?,
            )
            .ok_or_else(bound)?;
        metadata_bytes = metadata_bytes
            .checked_add(
                crate::capture::json_size(&request, &lifetime.budget)?
                    .checked_mul(16)
                    .ok_or_else(bound)?,
            )
            .ok_or_else(bound)?;
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
        let provenance = serde_json::json!({"engine":"datafusion", "request":request, "lookupRequirements":lookup_reports, "sources":sources.iter().map(|s|s.provenance()).collect::<Vec<_>>(), "complete":sources.iter().all(|s|s.complete())});
        let complete = sources.iter().all(|s| s.complete());
        output.shrink_to(bytes)?;
        lifetime.settle()?;
        for snapshot in pending {
            self.install_source(snapshot, lifetime.clone())?;
        }
        let mut dependencies = sources
            .iter()
            .flat_map(|source| source.table().hotfix_captures.iter().cloned())
            .collect::<Vec<_>>();
        dependencies.sort();
        dependencies.dedup();
        Ok(ResultOwner {
            dependencies,
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
