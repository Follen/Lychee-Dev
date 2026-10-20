//! Demand-only adapter over the same WDC/Hotfix decoders and Arrow builders.
//! It owns source/control state, never a borrowed iterator or another executor.
use super::*;
mod arrow_snapshot;
use arrow_snapshot::{ArrowSnapshot, ArrowCursor};
use crate::local_query::provider::{OwnedEquality, OwnedPhysicalCursor};
use crate::relational::RowCursor;
use arrow_array::UInt32Array;

pub(crate) enum DemandKind {
    Arrow(Arc<ArrowSnapshot>),
    Static(Arc<table::Table<'static>>),
    Physical(Arc<Physical<'static>>),
    Effective {
        base: Arc<Physical<'static>>,
        plan: Arc<effective::OverlayPlan>,
    },
}
pub(crate) struct DemandSource {
    pub(super) kind: DemandKind,
    pub(super) original: Option<Arc<db2::Prepared>>,
    pub(super) schema: Arc<SchemaOwner>,
    pub(super) budget: Arc<Budget>,
    pub(super) generation: Option<Arc<super::super::Generation>>,
    absence_plan: Option<Arc<effective::OverlayPlan>>,
}
impl DemandSource {
    pub(crate) fn absent_is_unresolved(&self, id: u32) -> Result<bool> {
        if let Some(plan) = &self.absence_plan {
            if plan.deleted_ids().any(|deleted| deleted == id) {
                return Ok(false);
            }
        }
        self.original
            .as_ref()
            .ok_or_else(shape)?
            .availability
            .record_unavailable(id, &self.budget)
    }
    pub(crate) fn uses_budget(&self, budget: &Arc<Budget>) -> bool {
        Arc::ptr_eq(&self.budget, budget)
    }
    #[cfg(test)]
    pub(crate) fn test_static(
        table: Arc<table::Table<'static>>,
        budget: Arc<Budget>,
    ) -> Result<Arc<Self>> {
        let original = import::schema_for_table(&table, &budget)?;
        let schema = Arc::new(SchemaOwner {
            schema: original.schema().clone(),
            _original: original,
            _extra: None,
        });
        Ok(Arc::new(Self {
            kind: DemandKind::Static(table),
            schema,
            budget,
            generation: None,
            original: None,
            absence_plan: None,
        }))
    }
    pub(crate) fn schema(&self) -> SchemaRef {
        self.schema.schema.clone()
    }
    pub(crate) fn close(&self) -> Result<()> {
        self.generation
            .as_ref()
            .map_or(Ok(()), |generation| generation.close())
    }
    pub(crate) fn columns(&self) -> &[crate::schema::Field] {
        match &self.kind {
            DemandKind::Arrow(_) => self.original.as_ref().expect("authenticated source").definition.fields.as_slice(),
            DemandKind::Static(table) => table.columns(),
            DemandKind::Physical(base) | DemandKind::Effective { base, .. } => base.table.columns(),
        }
    }
    pub(crate) fn cursor(self: &Arc<Self>, ids: Option<&[u32]>) -> Result<DemandCursor> {
        self.budget.check()?;
        let selection = self
            .budget
            .reserve_retained(add(256, mul(ids.map_or(0, <[u32]>::len), 4)?)?)?
            .into_owned(&self.budget)?;
        let kind = match &self.kind {
            DemandKind::Arrow(snapshot) => DemandCursorKind::Arrow(snapshot.new_cursor(ids, self.budget.clone())?),
            DemandKind::Static(table) => {
                let mut cursor = TableImporter::new_owned(
                    table.clone(),
                    self.budget.clone(),
                    ImportLimits {
                        rows: 1,
                        value_bytes: BATCH_VALUES,
                        text_bytes: 1 << 20,
                    },
                )?;
                if let Some(ids) = ids {
                    cursor = cursor.select_ids_owned(ids.to_vec())?;
                }
                DemandCursorKind::Static(cursor)
            }
            _ => {
                let current = if ids.is_none() {
                    self.full_cursor()?
                } else {
                    None
                };
                DemandCursorKind::Rows {
                    ids: ids.map(<[u32]>::to_vec),
                    next: 0,
                    current,
                }
            }
        };
        Ok(DemandCursor {
            source: self.clone(),
            kind,
            done: false,
            _selection: selection,
        })
    }
    fn full_cursor(&self) -> Result<Option<LocatedCursor>> {
        match &self.kind {
            DemandKind::Physical(base) => Ok(Some(LocatedCursor::Physical(base.owned_cursor()?))),
            DemandKind::Effective { base, plan } => Ok(Some(LocatedCursor::Effective(
                plan.owned_cursor(base.clone(), self.budget.clone())?,
            ))),
            DemandKind::Static(_) | DemandKind::Arrow(_) => Err(shape()),
        }
    }
    fn id_cursor(&self, id: u32) -> Result<Option<LocatedCursor>> {
        match &self.kind {
            DemandKind::Physical(base) => {
                Ok(Some(LocatedCursor::Physical(base.owned_id_cursor(id)?)))
            }
            DemandKind::Effective { base, plan } => {
                match plan.owned_id_cursor(base.clone(), self.budget.clone(), id)? {
                    OwnedEquality::Rows(cursor) => Ok(Some(LocatedCursor::Effective(cursor))),
                    OwnedEquality::Empty => Ok(None),
                    OwnedEquality::Declined => Err(shape()),
                }
            }
            DemandKind::Static(_) | DemandKind::Arrow(_) => Err(shape()),
        }
    }
}
enum LocatedCursor {
    Physical(OwnedPhysicalCursor),
    Effective(effective::OwnedOverlayCursor),
}
impl LocatedCursor {
    fn advance(&mut self) -> Result<bool> {
        match self {
            Self::Physical(c) => c.advance(),
            Self::Effective(c) => c.advance(),
        }
    }
    fn row(&self) -> &[Scalar] {
        match self {
            Self::Physical(c) => c.row(),
            Self::Effective(c) => c.row(),
        }
    }
    fn id(&self) -> Option<u32> {
        match self {
            Self::Physical(c) => c.current_id(),
            Self::Effective(c) => c.current_id(),
        }
    }
}
enum DemandCursorKind {
    Arrow(ArrowCursor),
    Static(TableImporter<'static, 'static>),
    Rows {
        ids: Option<Vec<u32>>,
        next: usize,
        current: Option<LocatedCursor>,
    },
}
pub(crate) struct DemandCursor {
    source: Arc<DemandSource>,
    kind: DemandCursorKind,
    done: bool,
    _selection: OwnedRetainedReservation,
}
pub(crate) struct DemandBatch {
    pub(crate) records: RecordBatch,
    pub(crate) ids: UInt32Array,
    pub(crate) _view_allocation: Option<OwnedRetainedReservation>,
}
impl DemandCursor {
    pub(crate) fn next_batch(&mut self) -> Result<Option<DemandBatch>> {
        if self.done {
            return Ok(None);
        }
        let result = self.next_inner();
        if result.is_err() || matches!(&result, Ok(None)) {
            self.done = true;
        }
        result
    }
    fn next_inner(&mut self) -> Result<Option<DemandBatch>> {
        self.source.budget.check()?;
        match &mut self.kind {
            DemandCursorKind::Arrow(cursor) => cursor.next_batch(),
            DemandCursorKind::Static(cursor) => {
                let Some(mut batch) = cursor.next_batch()? else {
                    return Ok(None);
                };
                batch.settle_retained()?;
                let (records, ids, _, allocation) = batch.into_parts();
                keep_demand(
                    records,
                    ids,
                    Admission::Imported(allocation),
                    &self.source.schema,
                    &self.source.budget,
                )
                .map(Some)
            }
            DemandCursorKind::Rows { ids, next, current } => loop {
                self.source.budget.check()?;
                if let Some(cursor) = current {
                    if cursor.advance()? {
                        let id = cursor.id().ok_or_else(shape)?;
                        return row_batch(
                            cursor.row(),
                            id,
                            &self.source.schema,
                            &self.source.budget,
                        )
                        .map(Some);
                    }
                }
                *current = None;
                let Some(ids) = ids else { return Ok(None) };
                let Some(id) = ids.get(*next) else {
                    return Ok(None);
                };
                *next += 1;
                *current = self.source.id_cursor(*id)?;
            },
        }
    }
}
fn keep_demand(
    records: RecordBatch,
    ids: UInt32Array,
    admission: Admission,
    schema: &Arc<SchemaOwner>,
    budget: &Arc<Budget>,
) -> Result<DemandBatch> {
    let hooks = budget
        .reserve_retained(add(
            2048,
            mul(add(mul(records.num_columns(), 4)?, 2)?, 256)?,
        )?)?
        .into_owned(budget)?;
    let owner = Arc::new(BufferOwner {
        _admission: admission,
        _schema: schema.clone(),
        _hooks: hooks,
    });
    let pool = InputPool(owner);
    records.claim(&pool);
    ids.claim(&pool);
    Ok(DemandBatch { records, ids, _view_allocation: None })
}
fn row_batch(
    values: &[Scalar],
    id: u32,
    schema: &Arc<SchemaOwner>,
    budget: &Arc<Budget>,
) -> Result<DemandBatch> {
    let _columns = budget.reserve_retained(add(1024, mul(schema.schema.fields().len(), 128)?)?)?;
    let columns = schema
        .schema
        .fields()
        .iter()
        .map(|f| Column::from_type(f.data_type()))
        .collect::<Result<Vec<_>>>()?;
    let cells = columns
        .iter()
        .try_fold(0usize, |n, c| add(n, c.width.unwrap_or(1)))?;
    if values.len() != cells {
        return Err(shape());
    }
    let mut position = 0;
    let mut bytes = 0;
    for column in &columns {
        for value in &values[position..position + column.width.unwrap_or(1)] {
            bytes = add(bytes, column.kind.inspect(value)?)?;
        }
        position += column.width.unwrap_or(1);
    }
    if bytes > BATCH_VALUES {
        return Err(limit());
    }
    // The same peak formula as the mature effective importer, now admitted
    // for one demanded row only. No next-row advance occurs after append.
    let peak = add(
        add(mul(add(bytes, cells)?, 2)?, mul(BATCH_VALUES, 3)?)?,
        add(mul(columns.len(), 2048)?, 4096)?,
    )?;
    let mut allocation = budget.reserve_retained(peak)?.into_owned(budget)?;
    budget.charge(Cost::work(bytes))?;
    let mut builders = columns
        .iter()
        .map(|c| c.builder(1))
        .collect::<Result<Vec<_>>>()?;
    position = 0;
    for (column, builder) in columns.iter().zip(&mut builders) {
        match builder {
            Builder::Scalar(builder) => column.kind.append(builder.as_mut(), &values[position]),
            Builder::List(builder) => {
                let width = column.width.ok_or_else(shape)?;
                for value in &values[position..position + width] {
                    column.kind.append(builder.values().as_mut(), value);
                }
                builder.append(true);
            }
        }
        position += column.width.unwrap_or(1);
    }
    let records = RecordBatch::try_new(
        schema.schema.clone(),
        builders.into_iter().map(Builder::finish).collect(),
    )
    .map_err(|_| shape())?;
    let ids = UInt32Array::from(vec![id]);
    allocation.shrink_to(add(
        add(records.get_array_memory_size(), ids.get_array_memory_size())?,
        add(4096, mul(columns.len(), 2048)?)?,
    )?)?;
    keep_demand(
        records,
        ids,
        Admission::Effective(allocation),
        schema,
        budget,
    )
}

pub(crate) struct PreparedDemand {
    pub(crate) source: Arc<DemandSource>,
    pub(crate) provenance: Value,
    pub(crate) complete: bool,
    pub(crate) rows: usize,
    pub(crate) physical_ids: Vec<u32>,
    _metadata: OwnedRetainedReservation,
}
struct PreparedBody(Arc<db2::Prepared>);
impl AsRef<[u8]> for PreparedBody {
    fn as_ref(&self) -> &[u8] { self.0.file.bytes.as_ref() }
}
impl DemandSource {
    /// Only immutable Arrow data is reused. A new request gets its own control
    /// and budget; no expired executable WDC cursor enters the session cache.
    pub(crate) fn rebind_arrow(&self, budget: Arc<Budget>) -> Result<Option<Arc<Self>>> {
        budget.check()?;
        let DemandKind::Arrow(snapshot) = &self.kind else { return Ok(None) };
        Ok(Some(Arc::new(Self { kind: DemandKind::Arrow(snapshot.clone()), original: self.original.clone(),
            schema: self.schema.clone(), budget, generation: None, absence_plan: self.absence_plan.clone() })))
    }
    pub(crate) fn is_arrow(&self) -> bool { matches!(self.kind, DemandKind::Arrow(_)) }
}

pub(crate) fn prepare_demand(invocation: &Invocation, prepared: db2::Prepared,
    scope: &mut dyn FileScope, effective_mode: bool, captures: &[String],
    whole_source_consumed: bool, budget: Arc<Budget>) -> Result<PreparedDemand> {
    invocation.control.check()?; budget.check()?; scope.check_consumer(invocation, &budget)?;
    let original = Arc::new(prepared);
    let identity = StaticIdentity::from_prepared(&original)?;
    let complete = identity.complete();
    let digest = identity.digest(&budget)?;
    let data = original.pin.data.as_ref().ok_or_else(shape)?;
    let table = table::open_owned_budgeted(PreparedBody(original.clone()), &original.definition,
        original.limits, &original.availability.skipped, &original.availability.ranges, budget.clone())?;
    let visible = import::schema_for_table(&table, &budget)?;
    let original_schema = visible.schema().clone();
    let (schema, extra) = if effective_mode {
        let owner = budget.reserve_retained(add(4096, mul(original_schema.fields().len(), 256)?)?)?.into_owned(&budget)?;
        let mut fields = original_schema.fields().iter().cloned().collect::<Vec<_>>();
        fields.extend([Arc::new(Field::new("__hotfix_source", DataType::Utf8, true)),
            Arc::new(Field::new("__hotfix_push", DataType::Int64, true)),
            Arc::new(Field::new("__hotfix_index", DataType::UInt64, true))]);
        (Arc::new(Schema::new(fields)), Some(owner))
    } else { (original_schema, None) };
    let schema = Arc::new(SchemaOwner { schema, _original: visible, _extra: extra });
    let reuse = super::super::open_existing(scope, StaticSource { data, bundle:&original.bundle,
        definition:&original.definition, layout:&original.layout, reading:&original.file.reading,
        missing:original.file.bytes.missing(), unavailable_partitions:&original.availability.skipped,
        bytes:original.file.bytes.len() }, &table, original.limits.file_bytes, &budget)?;
    let (generation, mut cache) = match reuse {
        Reuse::Hit(g) => { let v=json!({"status":"hit","generation":g.key,"object":g.object,
            "reused":true,"publicationThisCommand":false}); (Some(g),v) },
        Reuse::Absent => (None,json!({"status":"miss","reason":"not_prepared","reused":false,"publicationThisCommand":false})),
        Reuse::UnsupportedSchema => (None,json!({"status":"unavailable","reason":"schema_representation_limit","reused":false,"publicationThisCommand":false})),
        Reuse::MissingBody {generation,object} => (None,json!({"status":"unavailable","reason":"materialized_body_missing","generation":generation,"object":object,"reused":false,"publicationThisCommand":false})),
    };
    let eager_bound = generation.as_ref().and_then(|g| g.object.bytes.checked_mul(8))
        .and_then(|n| n.checked_add((table.logical_count() as u64).checked_mul(1024)?))
        .and_then(|n| n.checked_add(8 << 20));
    let remaining = budget.remaining_retained_bytes()?;
    let eager = whole_source_consumed && eager_bound.is_some_and(|bytes| bytes <= remaining / 2);
    let result = (|| {
        let mut base = Physical::new(table, budget.clone())?;
        // Only full-consumption plans may eagerly decode an entire generation.
        // The directory/object were authenticated above in either case.
        if eager { base.generation = generation.clone(); }
        let base = Arc::new(base);
        let (plan, report) = if effective_mode {
            let mut snapshots=effective::Snapshots::new(invocation,data,&budget);
            let (plan,report)=snapshots.prepare(&base.schema().columns,base.schema().complete,
                &original.definition,&original.bundle,captures)?;
            (Some(Arc::new(plan)),Some(report))
        } else {(None,None)};
        let id_bound = add(base.table.logical_count(), plan.as_ref().map_or(0, |p| p.patch_count()))?;
        let id_guard=budget.reserve_retained(add(1024,mul(id_bound,128)?)?)?;
        let mut physical_ids=base.table.ids().collect::<std::collections::BTreeSet<_>>();
        if let Some(plan)=&plan { plan.update_physical_ids(&mut physical_ids,&budget)?; }
        let physical_ids=physical_ids.into_iter().collect::<Vec<_>>();
        let arrow = if eager {
            generation.as_ref().map(|generation| match &plan {
                Some(plan)=>ArrowSnapshot::from_effective(&base,plan,&schema,&budget),
                None=>ArrowSnapshot::from_generation(generation,&schema,&budget),
            }).transpose()?
        } else {None};
        let selection=if arrow.is_some(){"authenticated_arrow_session_snapshot"}else{"authenticated_wdc_demand_stream"};
        let reason=if !whole_source_consumed {"demand_work_boundary"}else if generation.is_some() && !eager {"memory_admission"}else if arrow.is_none(){"no_admitted_generation"}else{"complete_native_ipc"};
        cache["representationSelection"] = json!(selection);
        cache["representationReason"] = json!(reason);
        let metadata_size=capture::json_size(&(&identity,&cache,&report,&original.file.reading,&original.definition,&original.layout),&budget)?;
        let metadata=budget.reserve_retained(add(add(mul(metadata_size,8)?,16384)?,mul(physical_ids.len(),16)?)?)?.into_owned(&budget)?;
        let provenance=json!({"sourceMode":if effective_mode{"effective"}else{"static"},
            "sourceIdentity":identity,"sourceSha256":digest,"sourceReading":original.file.reading,
            "sourceVerification":original.file.reading.get("verification"),
            "sourceDefinition":original.definition,"sourceLayout":original.layout,
            "typedGenerationCache":cache,"effective":report,"complete":complete,"truncated":false,"availableRows":physical_ids.len()});
        let absence_plan=plan.clone();
        let kind=match arrow {Some(arrow)=>DemandKind::Arrow(Arc::new(arrow)),None=>match plan {
            Some(plan)=>DemandKind::Effective{base,plan},None=>DemandKind::Static(base.table.clone())}};
        drop(id_guard);
        Ok(PreparedDemand { rows:physical_ids.len(),physical_ids,complete,provenance,_metadata:metadata,
            source:Arc::new(DemandSource{kind,original:Some(original.clone()),schema,budget:budget.clone(),generation:None,absence_plan}) })
    })();
    // Checked-close the exact authenticated reader before any snapshot escapes.
    let closed=generation.as_ref().map_or(Ok(()),|g|g.close());
    match (result,closed) { (Ok(value),Ok(()))=>Ok(value),(Err(error),Ok(()))=>Err(error),
        (Ok(_),Err(error))=>Err(error),(Err(mut error),Err(close))=>{error.details=json!({"primary":error.details,"closeError":close});Err(error)} }
}
impl DemandSource {
    pub(crate) fn promote_visited(&self,batches:Vec<DemandBatch>,expected_ids:&[u32])->Result<Option<Arc<Self>>>{
        let Some(snapshot)=ArrowSnapshot::from_visited(batches,expected_ids,&self.schema,&self.budget)? else{return Ok(None)};
        Ok(Some(Arc::new(Self{kind:DemandKind::Arrow(Arc::new(snapshot)),original:self.original.clone(),schema:self.schema.clone(),budget:self.budget.clone(),generation:None,absence_plan:self.absence_plan.clone()})))
    }
}
