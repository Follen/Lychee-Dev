//! Demand-driven native DataFusion scan. WDC/overlay polls decode one demanded
//! row; authenticated Arrow snapshots expose bounded native batches.
use super::{QueryOwner, external, unsupported};
use crate::generation::engine_adapter::demand::{DemandSource,DemandCursor,DemandBatch};
use arrow_array::{Array,RecordBatch,RecordBatchOptions};
use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::{catalog::{Session,TableProvider},common::Result as DfResult,execution::TaskContext,
    logical_expr::{Expr,TableType},physical_expr::EquivalenceProperties,
    physical_plan::{DisplayAs,DisplayFormatType,ExecutionPlan,Partitioning,PlanProperties,
        RecordBatchStream,SendableRecordBatchStream,execution_plan::{Boundedness,EmissionType}}};
use futures_util::Stream;
use std::{sync::{Arc,Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}},fmt,pin::Pin,task::{Context,Poll}};

pub(super) struct PullTableProvider {
    source: Arc<DemandSource>, owner:Arc<QueryOwner>, ids:Option<Vec<u32>>,
    selection:Option<(usize,u32)>, cap:Option<usize>, batch_rows:usize,
    observed:Arc<Mutex<Vec<DemandBatch>>>, accepted:Arc<Mutex<Vec<u32>>>, collect:bool,track_ids:bool, collection_complete:Arc<AtomicBool>, collected:Arc<AtomicUsize>, collection_limit:usize,
    public_lookup: Option<Arc<PublicLookup>>,
}
struct PublicLookup {
    column: usize,
    ids: Vec<u32>,
    found: Mutex<Vec<bool>>,
    exhausted: AtomicBool,
}
impl PublicLookup {
    fn observe(&self, records: &RecordBatch) -> DfResult<()> {
        let values = records.column(self.column);
        let mut found = self.found.lock().map_err(|_| external(super::limit()))?;
        for row in 0..records.num_rows() {
            if values.is_null(row) { continue; }
            let id = if let Some(values) = values.as_any().downcast_ref::<arrow_array::UInt64Array>() {
                u32::try_from(values.value(row)).ok()
            } else {
                let values = values.as_any().downcast_ref::<arrow_array::Int64Array>()
                    .ok_or_else(|| unsupported("Public lookup integer representation changed"))?;
                u32::try_from(values.value(row)).ok()
            };
            if let Some(id) = id && let Ok(slot) = self.ids.binary_search(&id) { found[slot] = true; }
        }
        Ok(())
    }
}
impl fmt::Debug for PullTableProvider {fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str("AuthenticatedDemandTable")}}
impl PullTableProvider {
    pub(super) fn new(source:Arc<DemandSource>,owner:Arc<QueryOwner>,ids:Option<&[u32]>,
        filter:Option<(&str,u32)>,cap:Option<usize>,collect:bool,batch_rows:usize) -> DfResult<Self> {
        owner.check()?;
        if !source.uses_budget(&owner.budget) {return Err(unsupported("Source request budget differs"));}
        owner.retain(4096usize.checked_add(ids.map_or(0,|ids|ids.len()).checked_mul(8).ok_or_else(||external(super::limit()))?).ok_or_else(||external(super::limit()))?)?;
        let selection=filter.map(|(name,wanted)| {
            let index=source.schema().index_of(name)?;
            if !matches!(source.schema().field(index).data_type(),arrow_schema::DataType::Int64|arrow_schema::DataType::UInt64) {
                return Err(unsupported("Physical domain filter requires a non-array integer field"));
            }
            Ok((index,wanted))
        }).transpose()?;
        let available=usize::try_from(owner.budget.remaining_retained_bytes().map_err(external)?).map_err(|_|external(super::limit()))?;
        let collection_limit=if ids.is_none() && cap.is_none() && filter.is_none(){available/2}else{(available/8).min(8<<20)};
        Ok(Self{public_lookup:None,collection_complete:Arc::new(AtomicBool::new(collect)),collected:Arc::new(AtomicUsize::new(0)),collection_limit,source,owner,ids:ids.map(<[u32]>::to_vec),selection,cap,batch_rows,observed:Arc::new(Mutex::new(Vec::new())),accepted:Arc::new(Mutex::new(Vec::new())),collect,track_ids:cap.is_some()||filter.is_some()||ids.is_some()})
    }
    /// Observe public IDs in the rows already consumed by this plan. Physical
    /// locators and Hotfix tombstones cannot prove absence in this domain.
    pub(super) fn track_public_lookup(mut self, ids: &[u32]) -> DfResult<Self> {
        let schema = self.source.schema();
        if let Ok(column) = schema.index_of("ID")
            && matches!(schema.field(column).data_type(), arrow_schema::DataType::Int64 | arrow_schema::DataType::UInt64)
        {
            self.owner.retain(ids.len().checked_mul(8).and_then(|n| n.checked_add(512))
                .ok_or_else(|| external(super::limit()))?)?;
            self.public_lookup = Some(Arc::new(PublicLookup { column, ids: ids.to_vec(),
                found: Mutex::new(vec![false; ids.len()]), exhausted: AtomicBool::new(false) }));
        }
        Ok(self)
    }
    pub(super) fn public_lookup_absence(&self, source_complete: bool) -> DfResult<Option<(Vec<u32>, Vec<u32>)>> {
        let Some(lookup) = &self.public_lookup else { return Ok(None); };
        let found = lookup.found.lock().map_err(|_| external(super::limit()))?;
        let complete = source_complete && lookup.exhausted.load(Ordering::Relaxed);
        let (mut missing, mut unresolved) = (Vec::new(), Vec::new());
        for (&id, &found) in lookup.ids.iter().zip(found.iter()) {
            self.owner.work(1)?;
            if !found { if complete { missing.push(id); } else { unresolved.push(id); } }
        }
        Ok(Some((missing, unresolved)))
    }
    pub(super) fn collection_complete(&self)->bool {self.collection_complete.load(Ordering::Relaxed)}
    pub(super) fn take_accepted_ids(&self)->DfResult<Vec<u32>> {Ok(std::mem::take(&mut *self.accepted.lock().map_err(|_|external(super::limit()))?))}
    pub(super) fn take_observed(&self)->DfResult<Vec<DemandBatch>> {
        Ok(std::mem::take(&mut *self.observed.lock().map_err(|_|external(super::limit()))?))
    }
}
#[async_trait]
impl TableProvider for PullTableProvider {
    fn schema(&self)->SchemaRef{self.source.schema()}
    fn table_type(&self)->TableType{TableType::Base}
    async fn scan(&self,_state:&dyn Session,projection:Option<&Vec<usize>>,filters:&[Expr],limit:Option<usize>)->DfResult<Arc<dyn ExecutionPlan>>{
        self.owner.check()?;
        if !filters.is_empty(){return Err(unsupported("Unhandled pushed predicate"));}
        let schema=self.source.schema();
        let scan_bytes=schema.fields().len().checked_mul(1024)
            .and_then(|n|n.checked_add(self.ids.as_ref().map_or(0,Vec::len).checked_mul(8)?))
            .and_then(|n|n.checked_add(4096)).ok_or_else(||external(super::limit()))?;
        self.owner.retain(scan_bytes)?;
        let projection=projection.cloned().unwrap_or_else(||(0..schema.fields().len()).collect());
        let projected=Arc::new(schema.project(&projection)?);
        let properties=Arc::new(PlanProperties::new(EquivalenceProperties::new(projected),Partitioning::UnknownPartitioning(1),EmissionType::Incremental,Boundedness::Bounded));
        Ok(Arc::new(PullScan{source:self.source.clone(),owner:self.owner.clone(),ids:self.ids.clone(),projection,
            selection:self.selection,rowwise:self.cap.is_some()||self.selection.is_some(),batch_rows:self.batch_rows,limit:match(self.cap,limit){(Some(a),Some(b))=>Some(a.min(b)),(a,b)=>a.or(b)},observed:self.observed.clone(),accepted:self.accepted.clone(),collect:self.collect,track_ids:self.track_ids,collection_complete:self.collection_complete.clone(),collected:self.collected.clone(),collection_limit:self.collection_limit,public_lookup:self.public_lookup.clone(),properties}))
    }
}
struct PullScan {source:Arc<DemandSource>,owner:Arc<QueryOwner>,ids:Option<Vec<u32>>,projection:Vec<usize>,selection:Option<(usize,u32)>,rowwise:bool,batch_rows:usize,limit:Option<usize>,observed:Arc<Mutex<Vec<DemandBatch>>>, accepted:Arc<Mutex<Vec<u32>>>, collect:bool,track_ids:bool, collection_complete:Arc<AtomicBool>, collected:Arc<AtomicUsize>, collection_limit:usize,public_lookup:Option<Arc<PublicLookup>>,properties:Arc<PlanProperties>}
impl fmt::Debug for PullScan{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str("AuthenticatedDemandScan")}}
impl DisplayAs for PullScan{fn fmt_as(&self,_:DisplayFormatType,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str("AuthenticatedDemandScan")}}
impl ExecutionPlan for PullScan {
    fn name(&self)->&'static str{"AuthenticatedDemandScan"}
    fn properties(&self)->&Arc<PlanProperties>{&self.properties}
    fn children(&self)->Vec<&Arc<dyn ExecutionPlan>>{Vec::new()}
    fn with_new_children(self:Arc<Self>,children:Vec<Arc<dyn ExecutionPlan>>)->DfResult<Arc<dyn ExecutionPlan>>{
        if !children.is_empty(){return Err(unsupported("Demand scan cannot acquire children"));} Ok(self)
    }
    fn execute(&self,partition:usize,_:Arc<TaskContext>)->DfResult<SendableRecordBatchStream>{
        self.owner.check()?;if partition!=0{return Err(unsupported("Demand source has one ordered partition"));}
        self.owner.retain(4096+self.projection.len()*64)?;
        Ok(Box::pin(PullStream{cursor:self.source.cursor(self.ids.as_deref(),self.batch_rows.min(self.limit.unwrap_or(usize::MAX)).max(1)).map_err(external)?,owner:self.owner.clone(),
            schema:self.schema(),projection:self.projection.clone(),selection:self.selection,rowwise:self.rowwise,remaining:self.limit.unwrap_or(usize::MAX),
            observed:self.observed.clone(),accepted:self.accepted.clone(),collect:self.collect,track_ids:self.track_ids,collection_complete:self.collection_complete.clone(),collected:self.collected.clone(),collection_limit:self.collection_limit,public_lookup:self.public_lookup.clone(),done:false}))
    }
}
struct PullStream{cursor:DemandCursor,owner:Arc<QueryOwner>,schema:SchemaRef,projection:Vec<usize>,selection:Option<(usize,u32)>,rowwise:bool,remaining:usize,observed:Arc<Mutex<Vec<DemandBatch>>>, accepted:Arc<Mutex<Vec<u32>>>, collect:bool,track_ids:bool, collection_complete:Arc<AtomicBool>, collected:Arc<AtomicUsize>, collection_limit:usize,public_lookup:Option<Arc<PublicLookup>>,done:bool}
impl PullStream {
    fn next_batch(&mut self)->DfResult<Option<RecordBatch>> {
        self.owner.check()?;if self.remaining==0{return Ok(None);}
        loop {
            // Domain scans deliberately use one row: their caller detects the
            // limit+1 witness before accepting it. Plain full-source WDC scans
            // and authenticated Arrow query scans use bounded native batches.
            let demand=if self.rowwise {1}else{self.remaining};
            let Some(mut batch)=self.cursor.next_batch(demand).map_err(external)? else{
                if let Some(lookup)=&self.public_lookup {lookup.exhausted.store(true,Ordering::Relaxed);}
                return Ok(None)
            };
            let rows=batch.records.num_rows();
            if rows==0 || rows>demand{return Err(unsupported("Demand batch row contract changed"));}
            if let Some(lookup)=&self.public_lookup {
                self.owner.work(rows.checked_mul((usize::BITS-lookup.ids.len().leading_zeros()) as usize+1).ok_or_else(||external(super::limit()))?)?;
                lookup.observe(&batch.records)?;
            }
            self.owner.work(rows.checked_mul(batch.records.num_columns()+1).ok_or_else(||external(super::limit()))?)?;
            let selected=match self.selection {None=>true,Some((field,wanted))=>{
                let array=batch.records.column(field);
                if array.is_null(0){false}else if let Some(a)=array.as_any().downcast_ref::<arrow_array::UInt64Array>(){a.value(0)==u64::from(wanted)}
                else if let Some(a)=array.as_any().downcast_ref::<arrow_array::Int64Array>(){a.value(0)==i64::from(wanted)}else{return Err(unsupported("Domain integer representation changed"));}
            }};
            let overhead=1024usize.checked_add(batch.records.num_columns().checked_mul(256).ok_or_else(||external(super::limit()))?).ok_or_else(||external(super::limit()))?;
            // Include the original importer admission and buffer ownership
            // hooks, not only the small visible Arrow arrays.
            let cost=overhead.checked_add(128).and_then(|n|n.checked_add(batch.retained_bytes)).ok_or_else(||external(super::limit()))?;
            let previous=self.collected.load(Ordering::Relaxed);
            let keep=self.collect && self.collection_complete.load(Ordering::Relaxed) && previous.checked_add(cost).is_some_and(|n|n<=self.collection_limit);
            if keep {self.owner.retain(overhead)?;self.collected.store(previous+cost,Ordering::Relaxed);}
            else if self.collect {self.collection_complete.store(false,Ordering::Relaxed);self.observed.lock().map_err(|_|external(super::limit()))?.clear();}
            let records=if selected {Some(RecordBatch::try_new_with_options(self.schema.clone(),self.projection.iter().map(|&i|batch.records.column(i).clone()).collect(),&RecordBatchOptions::new().with_row_count(Some(rows)))?)}else{None};
            if selected {
                self.owner.adopt_view(&mut batch._view_allocation)?;
            }
            if selected && self.track_ids {self.owner.retain(rows.checked_mul(64).ok_or_else(||external(super::limit()))?)?;self.accepted.lock().map_err(|_|external(super::limit()))?.extend_from_slice(batch.ids.values());}
            if keep {self.observed.lock().map_err(|_|external(super::limit()))?.push(batch);}
            if let Some(records)=records {self.remaining-=rows;return Ok(Some(records));}
        }
    }
}
impl Stream for PullStream {type Item=DfResult<RecordBatch>;fn poll_next(mut self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Option<Self::Item>>{
    if self.done{return Poll::Ready(None);}match self.next_batch(){Ok(Some(batch))=>Poll::Ready(Some(Ok(batch))),Ok(None)=>{self.done=true;Poll::Ready(None)},Err(e)=>{self.done=true;Poll::Ready(Some(Err(e)))}}
}}
impl RecordBatchStream for PullStream{fn schema(&self)->SchemaRef{self.schema.clone()}}
