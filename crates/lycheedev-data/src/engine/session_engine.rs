//! One session, one mature execution kernel. No legacy-executor fallback.
use super::{
    api::*,
    error,
    lifetime::{RequestLifetime, SessionMemory},
    plan, provider,
    result::ResultOwner,
    source_demand::{self as source, PreparedSnapshot, SourceUse},
};
use crate::fault;
use arrow_array::Array;
use super::{pull_provider::PullTableProvider, output_batches::OutputBatches};
use futures_util::StreamExt;
use crate::request::DataContext;
use lycheedev_core::{Result, control::OperationControl};
use std::sync::Arc;

struct Cached {
    snapshot: Arc<PreparedSnapshot>,
    lifetime: Arc<RequestLifetime>,
}
pub struct Engine {
    context: DataContext,
    limits: EngineLimits,
    memory: SessionMemory,
    cache: Vec<Cached>,
    pinned: Option<(String, Option<lycheedev_core::selection::DataPin>)>,
}
impl Engine {
    pub fn new(context: DataContext, limits: EngineLimits) -> Result<Self> {
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
            context,
            limits,
            memory: SessionMemory::new(limits.session_retained_bytes)?,
            cache: Vec::new(),
            pinned: None,
        })
    }
    // Discard cold cache ownership before request admission, never live results.
    // Reservations actually refunded by final buffer owners remain authoritative.
    fn admit_request(&mut self, control: &OperationControl) -> Result<Arc<RequestLifetime>> {
        control.check()?;
        let required = self.limits.result_bytes.checked_add(self.limits.operator_bytes)
            .and_then(|n| n.checked_add(8 << 20)).ok_or_else(bound)?
            .min(self.limits.session_retained_bytes);
        loop {
            let used = usize::try_from(self.memory.budget.snapshot()?.retained_bytes).map_err(|_|bound())?;
            let available = self.limits.session_retained_bytes.checked_sub(used).ok_or_else(bound)?;
            if available >= required || self.cache.is_empty() { break; }
            let Cached { snapshot, lifetime } = self.cache.remove(0);
            drop(snapshot);
            lifetime.settle()?;
            control.check()?;
        }
        self.memory.begin(self.limits.query_work, control.clone())
    }
    async fn source(
        &mut self,
        request: &TableRequest,
        whole_source_consumed: bool,
        source_use: &SourceUse,
        lifetime: &Arc<RequestLifetime>,
        control: &OperationControl,
    ) -> Result<(Arc<PreparedSnapshot>, Arc<RequestLifetime>, bool)> {
        for cached in &self.cache {
            control.check()?;
            if cached.snapshot.table() == request
                && cached.snapshot.reusable()
                && cached.snapshot.covers(source_use)
            {
                return Ok((cached.snapshot.clone(), cached.lifetime.clone(), false));
            }
        }
        let mut context = self.context.clone();
        context.control = control.clone();
        let prepared = source::prepare_selected(
            &context,
            request,
            whole_source_consumed,
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
            self.context.source.snapshot = Some(prepared.pin().id.clone());
            self.pinned = Some((prepared.pin().id.clone(), prepared.pin().data.clone()));
        }
        Ok((prepared, lifetime.clone(), true))
    }
    fn install_source(
        &mut self,
        snapshot: Arc<PreparedSnapshot>,
        lifetime: Arc<RequestLifetime>,
    ) -> Result<()> {
        if !snapshot.reusable() { return Ok(()); }
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
        let lifetime = self.admit_request(&control)?;
        let _execution = super::lifetime::RequestExecution(lifetime.clone());
        let mut context = self.context.clone();
        context.control = control;
        let (mut catalog, pin) = super::source_io::run(|| super::catalog::read(&context, &lifetime.budget))?;
        if let Some((id, data)) = &self.pinned {
            if id != &pin.id || data != &pin.data {
                return Err(fault(
                    "records.source_identity",
                    3,
                    "Catalog differs from the session immutable snapshot",
                ));
            }
        } else {
            self.context.source.snapshot = Some(pin.id.clone());
            self.pinned = Some((pin.id, pin.spec.data));
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
        let lifetime = self.admit_request(&control)?;
        let _execution = super::lifetime::RequestExecution(lifetime.clone());
        let (snapshot, _, new) = self.source(&table, false, &SourceUse::Whole, &lifetime, &control).await?;
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
        let _execution = super::lifetime::RequestExecution(lifetime.clone());
        let mut context = self.context.clone();
        context.control = control.clone();
        super::evidence::capture(&context, result, lifetime)
    }
}
fn bound() -> lycheedev_core::ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Unified query exceeds its admitted bound",
    )
}

fn consumes_all_source_rows(plan:&QueryPlan)->bool {
    let mut node=plan;
    loop {match node { QueryPlan::Scan{..}=>return true,QueryPlan::Project{input,..}|QueryPlan::Sort{input,..}=>node=input,
        _=>return false }}
}
fn source_batch_rows(plan: &QueryPlan) -> usize {
    let mut node = plan;
    loop {
        node = match node {
            QueryPlan::Scan { .. } => return 1024,
            QueryPlan::Project { input, columns }
                if columns.iter().all(|column| matches!(column.expression, Expression::Column { .. })) => input,
            QueryPlan::Sort { input, keys }
                if keys.iter().all(|key| matches!(key.expression, Expression::Column { .. })) => input,
            // Limits, predicates and computed expressions retain the original
            // one-row demand/error boundary. Plain full-source columns and
            // ordering can share a bounded importer batch.
            _ => return 1,
        };
    }
}
impl Engine {
    pub async fn execute(&mut self,request:QueryRequest,control:OperationControl)->Result<ResultOwner>{
        self.execute_mode(request,None,control).await
    }
    pub async fn explain(&mut self,request:QueryRequest,verbose:bool,analyze:bool,control:OperationControl)->Result<ResultOwner>{
        self.execute_mode(request,Some((verbose,analyze)),control).await
    }
    async fn execute_mode(&mut self,request:QueryRequest,explanation:Option<(bool,bool)>,control:OperationControl)->Result<ResultOwner>{
        let entered=std::time::Instant::now();control.check()?;
        let QueryRequest::Plan{plan:query_plan}=&request;
        let lifetime=self.admit_request(&control)?;
        let _execution=super::lifetime::RequestExecution(lifetime.clone());
        let allowance=lifetime.budget.remaining_retained_bytes()?;
        let batch_rows=source_batch_rows(query_plan);
        let (context,owner)=provider::runtime::context(lifetime.budget.clone(),self.limits.operator_bytes,batch_rows).map_err(error)?;
        let requirements=plan::requirements(query_plan,&owner).map_err(error)?;
        let whole=explanation.is_none_or(|(_,analyze)|analyze) && consumes_all_source_rows(query_plan);
        // Output admission precedes optional eager source representations.
        let mut output=lifetime.budget.reserve_retained(self.limits.result_bytes)?.into_owned(&lifetime.budget)?;
        let mut sources=Vec::new();let mut source_provenance=Vec::new();let mut providers=Vec::new();let mut lifetimes=vec![lifetime.clone()];
        let mut source_uses=Vec::new();
        let mut aliases=std::collections::BTreeSet::new();
        let mut lookup_reports=Vec::new();
        for requirement in &requirements {
            let alias=plan::table_name(&requirement.table).map_err(error)?;
            if !aliases.insert(alias.clone()){return Err(fault("records.source_identity",3,"Duplicate source alias"));}
            owner.retain(requirement.ids.as_ref().map_or(0,|ids|ids.len()).checked_mul(8).and_then(|n|n.checked_add(512)).ok_or_else(bound)?).map_err(error)?;
            let source_use=match &requirement.ids {Some(ids)=>SourceUse::PublicIds(ids.clone()),None=>SourceUse::Whole};
            let(snapshot,retained,new)=self.source(&requirement.table,whole,&source_use,&lifetime,&control).await?;
            owner.retain(crate::capture::json_size(snapshot.provenance(),&lifetime.budget)?.checked_mul(8).and_then(|n|n.checked_add(8192)).ok_or_else(bound)?).map_err(error)?;
            source_provenance.push(snapshot.current_provenance(!new,&lifetime.budget)?);
            let selected=if snapshot.public_equals_physical(){requirement.ids.as_deref()}else{None};
            let mut provider=PullTableProvider::new(snapshot.demand(lifetime.budget.clone())?,owner.clone(),selected,None,None,whole||selected.is_some(),batch_rows).map_err(error)?;
            if let Some(ids)=requirement.ids.as_deref() && !snapshot.public_equals_physical(){
                provider=provider.track_public_lookup(ids).map_err(error)?;
            }
            let provider=Arc::new(provider);
            context.register_table(datafusion::common::TableReference::bare(alias),provider.clone()).map_err(error)?;
            if let Some(ids)=requirement.ids.as_deref(){
                owner.retain(8192usize.checked_add(ids.len().checked_mul(256).ok_or_else(bound)?).ok_or_else(bound)?).map_err(error)?;
                let mut missing=Vec::new();let mut unresolved=Vec::new();
                if snapshot.public_equals_physical(){
                    for &id in ids {control.check()?;if snapshot.physical_ids().binary_search(&id).is_err(){
                        if snapshot.absent_is_unresolved(id,&lifetime.budget)?{unresolved.push(id)}else{missing.push(id)}
                    }}
                }
                lookup_reports.push(serde_json::json!({"table":requirement.table,"requestedIds":ids,
                    "scope":"current_query_source_requirement","classification":if snapshot.public_equals_physical(){"authenticated_physical_locator"}else{"public_id_filter_not_physical_proof"},
                    "missingIds":missing,"unresolvedIds":unresolved,"coverageComplete":snapshot.complete()}));
            }
            sources.push(snapshot);providers.push(provider);lifetimes.push(retained);source_uses.push(source_use);
        }
        let prerequisite_start=std::time::Instant::now();
        let prerequisite_work=lifetime.budget.snapshot()?.decode_work;
        let prepared=plan::prepare(&context,query_plan,owner.clone()).await.map_err(error)?;
        let prerequisites=prepared.has_prerequisites();
        let nonanalyzing=explanation.is_some_and(|(_,analyze)|!analyze);
        let frame=if nonanalyzing {let frame=prepared.explain_frame(&context).map_err(error)?;drop(prepared);frame}
            else {prepared.complete(&context,owner.clone(),self.limits.result_bytes).await.map_err(error)?};
        let prerequisite_ms=prerequisite_start.elapsed().as_secs_f64()*1000.0;
        let prerequisite_work=lifetime.budget.snapshot()?.decode_work.saturating_sub(prerequisite_work);
        // Keep prerequisite MemTable owners alive until root output is detached.
        let held=if prerequisites && !nonanalyzing {
            let bytes=crate::capture::json_size(&request,&lifetime.budget)?.checked_mul(16).and_then(|n|n.checked_add(64<<10)).ok_or_else(bound)?;
            let admission=lifetime.budget.reserve_retained(bytes)?.into_owned(&lifetime.budget)?;
            Some((frame.logical_plan().clone(),admission))
        }else{None};
        let frame=if let Some((verbose,analyze))=explanation {frame.explain(verbose,analyze).map_err(error)?}else{frame};
        let request_value=if let Some((verbose,analyze))=explanation {serde_json::json!({"kind":"explain","query":request,"verbose":verbose,"analyze":analyze})}
            else {serde_json::to_value(&request).map_err(|_|bound())?};
        let mut metadata_bytes=4096usize+(16<<10)+if explanation.is_some(){16<<10}else{0};
        for field in frame.schema().fields(){metadata_bytes=metadata_bytes.checked_add(field.name().len()+256).ok_or_else(bound)?;}
        for source in &source_provenance {metadata_bytes=metadata_bytes.checked_add(crate::capture::json_size(source,&lifetime.budget)?.checked_mul(8).ok_or_else(bound)?).ok_or_else(bound)?;}
        metadata_bytes=metadata_bytes.checked_add(crate::capture::json_size(&(&request_value,&lookup_reports),&lifetime.budget)?.checked_mul(16).ok_or_else(bound)?).ok_or_else(bound)?;
        if metadata_bytes>self.limits.result_bytes{return Err(bound());}
        let root_start=std::time::Instant::now();let root_work=lifetime.budget.snapshot()?.decode_work;
        let mut stream=frame.execute_stream().await.map_err(error)?;
        // Optimizer-refined nullability belongs to the executing stream. Keep
        // result metadata and strict output admission on that same schema.
        let schema=stream.schema();
        let columns=schema.fields().iter().map(|f|f.name().clone()).collect::<Vec<_>>();
        let mut collector=OutputBatches::new(schema.clone(),self.limits.result_bytes-metadata_bytes);
        while let Some(next)=stream.next().await {control.check()?;owner.check().map_err(error)?;collector.push(next.map_err(error)?,&lifetime.budget)?;}
        let (batches,body_bytes,rows)=collector.finish(&lifetime.budget)?;
        let bytes=body_bytes.checked_add(metadata_bytes).ok_or_else(bound)?;
        let root_ms=root_start.elapsed().as_secs_f64()*1000.0;
        let root_work=lifetime.budget.snapshot()?.decode_work.saturating_sub(root_work);
        drop(stream);drop(held);
        // Promote only full authenticated physical-ID coverage; filtered output
        // is never mistaken for source coverage. No reader is published.
        let mut reports=lookup_reports.iter_mut();
        for ((snapshot,provider),source_use) in sources.iter().zip(&providers).zip(&source_uses){
            if let SourceUse::PublicIds(_)=source_use {
                let report=reports.next().ok_or_else(bound)?;
                if !snapshot.public_equals_physical(){
                    // Only the rows consumed by this plan establish public-ID
                    // matches. A physical tombstone never proves public absence;
                    // an unexhausted or incomplete source leaves it unresolved.
                    report["identityDomain"]=serde_json::json!("public_id_not_physical_locator");
                    match provider.public_lookup_absence(snapshot.complete()).map_err(error)? {
                        Some((missing,unresolved))=>{
                            report["classification"]=serde_json::json!("exact_visible_integer_id");
                            report["missingIds"]=serde_json::json!(missing);
                            report["unresolvedIds"]=serde_json::json!(unresolved);
                        }
                        None=>{
                            report["classification"]=serde_json::json!(if snapshot.schema().index_of("ID").is_ok(){"unavailable_non_integer_id"}else{"unavailable_missing_id_column"});
                            report["missingIds"]=serde_json::Value::Null;
                            report["unresolvedIds"]=serde_json::Value::Null;
                        }
                    }
                }
            }
            let observed=provider.take_observed().map_err(error)?;
            if snapshot.reusable(){self.install_source(snapshot.clone(),lifetimes[sources.iter().position(|s|Arc::ptr_eq(s,snapshot)).ok_or_else(bound)?+1].clone())?;}
            else if !nonanalyzing && provider.collection_complete() && let Some(cached)=snapshot.promote(observed,source_use,&lifetime.budget)?{self.install_source(cached,lifetime.clone())?;}
        }
        drop(providers);drop(context);drop(source_uses);drop(aliases);drop(requirements);drop(owner);
        let complete=sources.iter().all(|s|s.complete());
        let mut dependencies=sources.iter().flat_map(|s|s.table().hotfix_captures.iter().cloned()).collect::<Vec<_>>();dependencies.sort();dependencies.dedup();
        drop(sources);
        output.shrink_to(bytes)?;lifetime.settle()?;
        let resources=serde_json::json!({"scope":"result_return_before_transport_and_later_handle_operations",
            "limits":{"sessionRetainedBytes":self.limits.session_retained_bytes,"requestRetainedBytes":allowance,"queryWork":self.limits.query_work,"operatorPoolBytes":self.limits.operator_bytes,"resultBytes":self.limits.result_bytes,"pageBytes":self.limits.page_bytes},
            "requestSafetyLedger":lifetime.budget.snapshot()?,"requestLogicalLedger":lifetime.budget.logical_snapshot()?,"sessionRetainedBytes":self.memory.budget.snapshot()?.retained_bytes,
            "retainedSemantics":"outstanding_admitted_bytes_not_rss_or_os_peak","workSemantics":"admitted_work_not_cpu_or_operator_cost",
            "resourceBoundaryScope":"in_process_logical_and_operator_pool","requiredExternalProtection":"formal_session_worker_supervisor","externalSupervisorVerifiedByEngine":false,"wholeNativeAllocatorAccountingComplete":false});
        let request_work_ledger=lifetime.budget.snapshot()?;
        let analysis=explanation.map(|(_,analyze)|serde_json::json!({"analysisScope":if !analyze{"expanded_native_plan_without_execution"}else if prerequisites{"root_after_completed_prerequisites"}else{"complete_native_root"},
            "requestElapsedMs":entered.elapsed().as_secs_f64()*1000.0,"requestTimingScope":"execute_entry_to_result_return",
            "requestWorkLedger":request_work_ledger,"wholeNativeWorkAccountingComplete":false,
            "excluded":["transport_serialization","subsequent_handle_operations"],
            "workLedgerSemantics":"admitted_work_not_cpu_or_operator_cost",
            "sourcesScope":"authenticated_request_sources_listed_in_provenance",
            "nativePreflightAndPrerequisites":{"executed":!nonanalyzing,"hasPrerequisites":prerequisites,"elapsedMs":prerequisite_ms,"decodeWork":prerequisite_work},
            "rootExecutionAndOutputCompaction":{"elapsedMs":root_ms,"decodeWork":root_work},"prerequisiteNativeMetricsIncluded":!prerequisites}));
        let provenance=serde_json::json!({"engine":"datafusion","request":request_value,"sources":source_provenance,"lookupRequirements":lookup_reports,"complete":complete,"resources":resources,"analysis":analysis});
        Ok(ResultOwner{schema:Some(schema),dependencies,batches,columns,provenance,provenance_bytes:metadata_bytes,complete,rows,bytes,session:self.memory.budget.clone(),page_bytes:self.limits.page_bytes,_lifetimes:lifetimes,_output:Some(output)})
    }
}

impl Engine {
    pub(crate) async fn lookup_domain_rows(&mut self,table:TableRequest,mut ids:Vec<u32>,control:OperationControl)->Result<super::OwnedDomainRows>{
        ids.sort_unstable();ids.dedup();
        self.domain_rows(table,Some(ids),None,None,control).await
    }
    pub(crate) async fn scan_domain_records(&mut self,table:TableRequest,filter:Option<(String,u32)>,limit:usize,control:OperationControl)->Result<super::OwnedDomainRows>{
        self.domain_rows(table,None,filter,Some(limit),control).await
    }
    async fn domain_rows(&mut self,table:TableRequest,ids:Option<Vec<u32>>,filter:Option<(String,u32)>,limit:Option<usize>,control:OperationControl)->Result<super::OwnedDomainRows>{
        control.check()?;
        let cap=match limit{Some(n)=>n.checked_add(1).ok_or_else(bound)?,None=>ids.as_ref().map_or(0,Vec::len)};
        if cap>1_000_001{return Err(bound());}
        let lifetime=self.admit_request(&control)?;
        let _execution=super::lifetime::RequestExecution(lifetime.clone());
        let mut output=lifetime.budget.reserve_retained(self.limits.result_bytes)?.into_owned(&lifetime.budget)?;
        let locator_scratch=lifetime.budget.reserve_retained(cap.checked_mul(32).and_then(|n|n.checked_add(8192)).ok_or_else(bound)?)?.into_owned(&lifetime.budget)?;
        let source_use=match &ids {Some(ids)=>SourceUse::PhysicalIds(ids.clone()),None=>SourceUse::Scan{filter:filter.clone(),cap}};
        let(snapshot,retained,new)=self.source(&table,false,&source_use,&lifetime,&control).await?;
        let(context,owner)=provider::runtime::context(lifetime.budget.clone(),self.limits.operator_bytes,1).map_err(error)?;
        let provider=Arc::new(PullTableProvider::new(snapshot.demand(lifetime.budget.clone())?,owner.clone(),ids.as_deref(),filter.as_ref().map(|(f,id)|(f.as_str(),*id)),Some(cap),true,1).map_err(error)?);
        let frame=context.read_table(provider.clone()).map_err(error)?;
        let schema=snapshot.schema();
        let request=match &ids{Some(ids)=>serde_json::json!({"kind":"domain_lookup","table":table,"ids":ids}),None=>serde_json::json!({"kind":"domain_scan","table":table,"filter":filter,"limit":limit.ok_or_else(bound)?})};
        let metadata_bytes=crate::capture::json_size(&(&request,snapshot.provenance()),&lifetime.budget)?.checked_mul(16).and_then(|n|n.checked_add(16384)).ok_or_else(bound)?;
        if metadata_bytes>self.limits.result_bytes{return Err(bound());}
        let mut stream=frame.execute_stream().await.map_err(error)?;
        let mut collector=OutputBatches::new(schema.clone(),self.limits.result_bytes-metadata_bytes);
        let mut emitted=0usize;let mut extra=false;
        while let Some(batch)=stream.next().await{
            control.check()?;let batch=batch.map_err(error)?;
            if limit.is_some_and(|maximum|emitted==maximum){extra=true;break;}
            emitted=emitted.checked_add(batch.num_rows()).ok_or_else(bound)?;
            collector.push(batch,&lifetime.budget)?;
        }
        let(batches,body_bytes,rows)=collector.finish(&lifetime.budget)?;
        drop(stream);
        let mut physical_ids=provider.take_accepted_ids().map_err(error)?;
        if extra {physical_ids.truncate(rows);}
        if physical_ids.len()!=rows{return Err(fault("records.internal",8,"Physical locator result length differs"));}
        // Output compaction can produce one small batch (or many large-cell
        // batches). Admit actual native locator container count before cloning.
        let locator_peak=rows.checked_mul(16).and_then(|n|n.checked_add(batches.len().checked_mul(1024)?))
            .and_then(|n|n.checked_add(cap.checked_mul(8)?)).and_then(|n|n.checked_add(16384)).ok_or_else(bound)?;
        let mut locator_owner=lifetime.budget.reserve_retained(locator_peak)?.into_owned(&lifetime.budget)?;
        let mut native_ids=Vec::with_capacity(batches.len());let mut offset=0usize;
        for batch in &batches {control.check()?;let end=offset.checked_add(batch.num_rows()).ok_or_else(bound)?;
            native_ids.push(arrow_array::UInt32Array::from(physical_ids.get(offset..end).ok_or_else(bound)?.to_vec()));offset=end;}
        let mut unresolved=Vec::new();
        if let Some(requested)=&ids {for &id in requested {control.check()?;if physical_ids.binary_search(&id).is_err() && snapshot.absent_is_unresolved(id,&lifetime.budget)?{unresolved.push(id);}}}
        let locator_bytes=native_ids.iter().try_fold(8192usize,|n,a|n.checked_add(a.get_array_memory_size()).and_then(|n|n.checked_add(256)).ok_or_else(bound))?
            .checked_add(unresolved.capacity().checked_mul(4).ok_or_else(bound)?).ok_or_else(bound)?;
        drop(physical_ids);locator_owner.shrink_to(locator_bytes)?;
        let observed=provider.take_observed().map_err(error)?;
        if snapshot.reusable(){self.install_source(snapshot.clone(),retained.clone())?;}
        else if provider.collection_complete() && let Some(cached)=snapshot.promote(observed,&source_use,&lifetime.budget)? {self.install_source(cached,lifetime.clone())?;}
        let current_provenance=snapshot.current_provenance(!new,&lifetime.budget)?;
        drop(provider);drop(context);drop(owner);
        let complete=snapshot.complete();let mut dependencies=table.hotfix_captures.clone();dependencies.sort();dependencies.dedup();
        let provenance=serde_json::json!({"engine":"datafusion","request":request,"sources":[current_provenance],"physicalLocatorIdentity":"authenticated_source_index",
            "complete":complete,"truncated":extra,"unresolvedIds":unresolved});
        drop(ids);drop(source_use);drop(locator_scratch);
        let columns=schema.fields().iter().map(|f|f.name().clone()).collect();let bytes=body_bytes.checked_add(metadata_bytes).ok_or_else(bound)?;
        output.shrink_to(bytes)?;lifetime.settle()?;
        let result=ResultOwner{schema:Some(schema.clone()),dependencies,batches,columns,provenance,provenance_bytes:metadata_bytes,complete,rows,bytes,
            session:self.memory.budget.clone(),page_bytes:self.limits.page_bytes,_lifetimes:vec![lifetime,retained],_output:Some(output)};
        Ok(super::OwnedDomainRows{result,schema:Some(schema),ids:native_ids,unresolved,extra,locator_owner:Some(locator_owner)})
    }
}
impl Engine {
    pub async fn semantics(&mut self,table:TableRequest,control:OperationControl)->Result<super::OwnedSemantics>{
        let lifetime=self.admit_request(&control)?;
        let _execution=super::lifetime::RequestExecution(lifetime.clone());
        let mut context=self.context.clone();context.control=control;
        let(result,pin)=super::source_io::run(|| super::semantics::read(&context,table,lifetime.clone(),self.limits.result_bytes))?;
        if let Some((id,data))=&self.pinned {if id!=&pin.id||data!=&pin.data{return Err(fault("records.source_identity",3,"Semantic metadata differs from the session pin"));}}
        else {self.context.source.snapshot=Some(pin.id.clone());self.pinned=Some((pin.id,pin.spec.data));}
        lifetime.settle()?;Ok(result)
    }
}

// Integrate inside impl Engine in engine/session_engine.rs. The engine module
// also declares mod domain, pub use domain::DomainResult and
// pub(crate) use domain::DomainOwner. This fragment does not edit active core.
impl Engine {
    pub(crate) fn begin_domain_request(
        &self,
        request: &DomainRequest,
        control: OperationControl,
    ) -> Result<super::DomainOwner> {
        self.begin_domain(control)?.bind_request(request)
    }
    pub(crate) fn begin_domain(&self, control: OperationControl) -> Result<super::DomainOwner> {
        let lifetime = self.memory.begin_bounded(
            self.limits.query_work,
            control,
            self.limits.result_bytes,
        )?;
        Ok(super::DomainOwner::new(lifetime, self.memory.budget.clone()))
    }
    pub(crate) fn capture_domain(
        &self,
        result: &super::DomainResult,
        request: &DomainRequest,
        control: OperationControl,
    ) -> Result<super::OwnedCapture> {
        if !Arc::ptr_eq(result.session(), &self.memory.budget) {
            return Err(fault("records.source_identity", 3, "Domain result belongs to a different session"));
        }
        let lifetime = self.memory.begin(self.limits.query_work, control.clone())?;
        let _execution = super::lifetime::RequestExecution(lifetime.clone());
        result.validate_request(request, &lifetime.budget)?;
        let mut context = self.context.clone();
        context.control = control.clone();
        super::evidence::capture_domain(&context, result, request, lifetime)
    }
}

#[cfg(test)]
#[path="session_recovery_tests.rs"]
mod recovery_tests;
