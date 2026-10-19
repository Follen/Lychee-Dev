//! Physical roots are prepared only when the real schema pass requests them.
//! Stable cells own bodies and metadata; execution borrows each cell directly.
use super::*;
use crate::hotfix::remote::identifier_lower;
use std::cell::{OnceCell, RefCell};

fn internal() -> lycheedev_core::ToolkitError {
    fault("records.internal", 8, "SQL resolver was reentered")
}
fn limit() -> lycheedev_core::ToolkitError {
    fault(
        "relational.resource_limit",
        3,
        "SQL source allocation exceeds its bound",
    )
}

enum Input {
    Semantic(relational::Source),
    Physical {
        input: provider::PreparedInput<'static>,
        file: Value,
        unavailable: Value,
        bytes: usize,
    },
}
impl Input {
    fn schema(&self) -> &relational::Source {
        match self {
            Self::Semantic(source) => source,
            Self::Physical { input, .. } => input.schema(),
        }
    }
}
struct Entry {
    catalog: String,
    name: String,
    input: Input,
}
// Moving a Box never moves its cell. Unlike a Vec of cells, growth cannot
// invalidate a Source borrowed by the current CTE/correlated execution frame.
// This is not a physical-table cap: meta roots use the same budgeted registry.
#[derive(Default)]
struct Slot {
    entry: OnceCell<Entry>,
    next: OnceCell<Box<Slot>>,
}
impl Slot {
    fn entries(&self) -> impl Iterator<Item = &Entry> {
        std::iter::successors(Some(self), |slot| slot.next.get().map(Box::as_ref))
            .filter_map(|slot| slot.entry.get())
    }
    fn insert(&self, entry: Entry, budget: &Budget) -> Result<&Entry> {
        let mut slot = self;
        while slot.entry.get().is_some() {
            if slot.next.get().is_none() {
                budget
                    .safety_only()
                    .charge(Cost::retained(size_of::<Slot>()))?;
                slot.next
                    .set(Box::new(Self::default()))
                    .map_err(|_| internal())?;
            }
            slot = slot.next.get().ok_or_else(internal)?;
        }
        slot.entry.set(entry).map_err(|_| internal())?;
        slot.entry.get().ok_or_else(internal)
    }
}
#[derive(Default)]
pub(super) struct Report {
    pub provenance: Vec<Value>,
    pub semantic: Vec<definitions::semantics::SemanticBundle>,
    pub effective: Vec<effective::Report>,
    pub decoded_bytes: usize,
    pub definitions_ms: f64,
    pub preparation_ms: f64,
}
struct Factory<'a> {
    inv: &'a Invocation,
    data: &'a selection::DataPin,
    budget: &'a Arc<Budget>,
    scope: &'a mut dyn crate::remote::provider::FileScope,
    maximum: usize,
    hotfix: &'a [String],
    snapshots: effective::Snapshots<'a>,
    local: Option<super::sql_reader::Reader<'a>>,
    report: Report,
}
pub(super) struct Resolver<'a> {
    slots: Slot,
    factory: RefCell<Factory<'a>>,
    budget: &'a Arc<Budget>,
}
impl<'a> Resolver<'a> {
    pub fn new(
        inv: &'a Invocation,
        data: &'a selection::DataPin,
        budget: &'a Arc<Budget>,
        scope: &'a mut dyn crate::remote::provider::FileScope,
        maximum: usize,
        hotfix: &'a [String],
    ) -> Self {
        Self {
            slots: Slot::default(),
            factory: RefCell::new(Factory {
                inv,
                data,
                budget,
                scope,
                maximum,
                hotfix,
                snapshots: effective::Snapshots::new(inv, data, budget),
                local: None,
                report: Report::default(),
            }),
            budget,
        }
    }
    pub fn check_source(&self) -> Result<()> {
        let factory = self.factory.try_borrow().map_err(|_| internal())?;
        factory.scope.check_consumer(factory.inv, factory.budget)?;
        if let Some(reader) = &factory.local {
            reader.verify_source()?;
        }
        Ok(())
    }
    pub fn take_report(&self) -> Result<Report> {
        let mut factory = self.factory.try_borrow_mut().map_err(|_| internal())?;
        Ok(std::mem::take(&mut factory.report))
    }
    pub fn close_after<T>(self, result: Result<T>) -> Result<T> {
        let factory = self.factory.into_inner();
        // All tables/views and cached shared bodies leave before their source
        // owner is closed. CDN Session remains owned by the enclosing Scope.
        drop(self.slots);
        match factory.local {
            Some(reader) => reader.close_after(result),
            None => result,
        }
    }
}
impl relational::RootResolver for Resolver<'_> {
    fn resolve(&self, use_: relational::SourceTableUse<'_>) -> Result<&relational::Source> {
        self.budget.check()?;
        // Go's root cache uses ToLower on each component, not EqualFold.
        self.budget.safety_only().charge(Cost::retained(
            (use_.catalog.len() + use_.name.len()).saturating_mul(3),
        ))?;
        let catalog = identifier_lower(use_.catalog);
        let name = identifier_lower(use_.name);
        if let Some(entry) = self
            .slots
            .entries()
            .find(|entry| entry.catalog == catalog && entry.name == name)
        {
            return Ok(entry.input.schema());
        }
        let input = self
            .factory
            .try_borrow_mut()
            .map_err(|_| internal())?
            .prepare(use_, &self.slots)?;
        Ok(self
            .slots
            .insert(
                Entry {
                    catalog,
                    name,
                    input,
                },
                self.budget,
            )?
            .input
            .schema())
    }
    fn provider(&self, source: &relational::Source) -> Option<&dyn relational::RowProvider> {
        self.slots.entries().find_map(|entry| match &entry.input {
            Input::Physical { input, .. } if std::ptr::eq(input.schema(), source) => {
                Some(input as &dyn relational::RowProvider)
            }
            _ => None,
        })
    }
}
impl Factory<'_> {
    fn prepare(&mut self, use_: relational::SourceTableUse<'_>, slots: &Slot) -> Result<Input> {
        self.inv.control.check()?;
        self.budget.check()?;
        self.scope.check_consumer(self.inv, self.budget)?;
        let started = Instant::now();
        if equal_fold(use_.catalog, "meta") {
            let result = definitions::semantics::source(
                &self.inv.home,
                &self.data.definition_commit,
                &self.data.full_build,
                use_.name,
                self.inv.flag_bool("--offline"),
                self.budget,
                &self.inv.control,
            );
            self.report.definitions_ms += started.elapsed().as_secs_f64() * 1000.0;
            let (source, provenance) = result?;
            self.budget.safety_only().charge(Cost::retained(
                size_of::<definitions::semantics::SemanticBundle>() * 2 + 128,
            ))?;
            self.report.semantic.push(provenance);
            return Ok(Input::Semantic(source));
        }
        let is_effective = equal_fold(use_.catalog, "effective");
        if !equal_fold(use_.catalog, "static") && !is_effective {
            return Err(fault(
                "relational.unsupported",
                3,
                "SQL catalog must be static, effective or meta",
            ));
        }
        if self.report.provenance.len() >= 32 {
            return Err(limit());
        }
        let result = definitions::prepare_controlled(
            &self.inv.home,
            &self.data.definition_commit,
            use_.name,
            self.inv.flag_bool("--offline"),
            self.budget.deadline,
            Some(self.budget),
            &self.inv.control,
        );
        self.report.definitions_ms += started.elapsed().as_secs_f64() * 1000.0;
        let (bundle, dbd) = result?;
        let started = Instant::now();
        let cached = slots.entries().find_map(|entry| match &entry.input {
            Input::Physical {
                input,
                file,
                unavailable,
                bytes,
            } if input.bundle.commit == bundle.commit
                && input.bundle.identity == bundle.identity
                && input.bundle.manifest == bundle.manifest
                && input.bundle.definition == bundle.definition =>
            {
                Some((input, file, unavailable, *bytes))
            }
            _ => None,
        });
        let (base, definition, layout_hash, file, unavailable, bytes) =
            if let Some((input, file, unavailable, bytes)) = cached {
                if let Some(reader) = &self.local {
                    reader.check_source()?;
                }
                if bytes > self.maximum {
                    return Err(fault(
                        "records.metadata_limit",
                        3,
                        "Selected content exceeds byte budget",
                    ));
                }
                // Go's prepared-view cache returns the original immutable reading.
                let size = capture::json_size(&(file, unavailable), &self.budget.safety_only())?;
                self.budget
                    .safety_only()
                    .charge(Cost::retained(size.saturating_mul(4) + 512))?;
                (
                    input.base.shared()?,
                    input.definition.clone(),
                    input.layout_hash,
                    file.clone(),
                    unavailable.clone(),
                    bytes,
                )
            } else {
                // Definition syntax is checked before physical file IO in Go.
                let document = schema::Document::parse_with_check(&dbd, || {
                    self.inv.control.check()?;
                    self.budget.check()
                })?;
                let file = if self.inv.flag_bool("--cdn") {
                    let mut selected = None;
                    self.scope.read_table_batches(
                        self.data,
                        &[bundle.identity.db2_file_data_id],
                        &mut |mut files| {
                            if selected.is_some() || files.len() != 1 {
                                return Err(internal());
                            }
                            selected = files.pop();
                            Ok(Vec::new())
                        },
                    )?;
                    selected.ok_or_else(internal)?
                } else {
                    if self.local.is_none() {
                        self.local = Some(super::sql_reader::Reader::new(
                            self.inv,
                            self.data,
                            self.budget,
                        )?);
                    }
                    self.local
                        .as_mut()
                        .ok_or_else(internal)?
                        .read(bundle.identity.db2_file_data_id)?
                };
                let bytes = file.bytes.len();
                let limits = super::table_limits(self.maximum);
                let (layout, availability) = table::inspect_available_budgeted(
                    &file.bytes,
                    file.bytes.missing(),
                    limits,
                    self.budget.clone(),
                )?;
                if layout.table_hash != bundle.identity.hash {
                    return Err(fault(
                        "schema.invalid_format",
                        4,
                        "WDC table hash differs from manifest identity",
                    ));
                }
                let definition = document.select(
                    &self.data.full_build,
                    &format!("{:08X}", layout.layout_hash),
                )?;
                let table = table::open_owned_budgeted(
                    file.bytes,
                    &definition,
                    limits,
                    &availability.skipped,
                    &availability.ranges,
                    self.budget.clone(),
                )?;
                self.budget.safety_only().charge(Cost::retained(
                    size_of::<schema::Definition>() + 2 * size_of::<usize>(),
                ))?;
                (
                    provider::Physical::new(table, self.budget.clone())?,
                    Arc::new(definition),
                    layout.layout_hash,
                    file.reading,
                    serde_json::to_value(&availability.partitions)?,
                    bytes,
                )
            };
        self.report.preparation_ms += started.elapsed().as_secs_f64() * 1000.0;
        let overlay = if is_effective {
            let (plan, report) = self.snapshots.prepare(
                base.schema().columns.as_slice(),
                base.schema().complete,
                &definition,
                &bundle,
                self.hotfix,
            )?;
            self.budget
                .safety_only()
                .charge(Cost::retained(size_of::<effective::Report>() * 2 + 128))?;
            self.report.effective.push(report);
            Some(plan)
        } else {
            None
        };
        let input = provider::PreparedInput {
            name: use_.key.to_owned(),
            base,
            overlay,
            definition,
            bundle,
            layout_hash,
        };
        let safety = self.budget.safety_only();
        let size = capture::json_size(
            &(
                &file,
                &unavailable,
                &input.bundle,
                input.definition.as_ref(),
            ),
            &safety,
        )?;
        safety.charge(Cost::retained(size.saturating_mul(4).saturating_add(1024)))?;
        let mut reading = json!({"complete":input.schema().complete,"truncated":false,"file":file,"sources":input.bundle,"layoutHash":format!("{layout_hash:08X}"),"schema":input.definition.as_ref()});
        if unavailable
            .as_array()
            .is_some_and(|parts| !parts.is_empty())
        {
            reading["unavailablePartitions"] = unavailable.clone();
        }
        self.report.provenance.push(reading);
        self.report.decoded_bytes = self
            .report
            .decoded_bytes
            .checked_add(bytes)
            .ok_or_else(limit)?;
        Ok(Input::Physical {
            input,
            file,
            unavailable,
            bytes,
        })
    }
}
