//! Field-order collection against the complete frozen map-based SQL producer.
//! The oracle below is provider.rs from 145093631d0bf69c91470ff5569092f9afed1dfb
//! (blob 07c9ce3dd05737214f87147690bbcbadc7efb49d), through Physical's complete
//! RowProvider implementation. It deliberately retains its own projection,
//! scalar, flatten, cursor, error and admission logic. Only the public map row
//! collector is shared with production; no new SQL helper supplies expectations.
//! These executable regressions are not a claim of native execution or speed.
use super::*;
use std::{
    ops::Range,
    sync::atomic::{AtomicBool, Ordering},
};

#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../../tests/support/partial_strings.rs"]
mod strings_fixture;

#[allow(dead_code)]
mod legacy {
    //! Query-local physical owners. Tables borrow the callback's external FileBytes;
    //! schemas, patch plans and definitions never borrow sibling owner fields.
    use super::*;
    use crate::resource::RetainedReservation;
    use relational::{
        EqualityCursor, EqualityOutcome, RowCursor, RowProvider, RowYield, Scalar, Source,
    };
    use std::collections::BTreeSet;

    fn binding() -> lycheedev_core::ToolkitError {
        fault(
            "relational.invalid_binding",
            4,
            "SQL provider schema or row is invalid",
        )
    }
    fn limit() -> lycheedev_core::ToolkitError {
        fault(
            "relational.resource_limit",
            3,
            "SQL provider allocation exceeds its bound",
        )
    }
    fn scalar(value: table::Value) -> Result<Scalar> {
        match value {
            table::Value::Null => Ok(Scalar::Null),
            table::Value::Int(value) => Ok(Scalar::Int(value)),
            table::Value::UInt(value) => Ok(Scalar::UInt(value)),
            table::Value::Float(value) if value.is_finite() => Ok(Scalar::Float(value)),
            table::Value::Float32(value) if value.is_finite() => Ok(Scalar::Float(value as f64)),
            table::Value::String(value) => Ok(Scalar::String(value)),
            _ => Err(fault(
                "relational.type_mismatch",
                4,
                "SQL provider scalar type is invalid",
            )),
        }
    }

    pub(super) fn emit_rows(
        mut cursor: Box<dyn RowCursor + '_>,
        emit: &mut RowYield<'_>,
    ) -> Result<()> {
        while cursor.advance()? {
            emit(cursor.row())?;
        }
        Ok(())
    }

    struct Projection {
        indices: BTreeSet<usize>,
        names: BTreeSet<String>,
    }
    struct FlatRow<'query> {
        cells: Vec<Scalar>,
        _vector: RetainedReservation<'query>,
        // Keep the emptied map alive for the same scope as the old callback.
        _raw: BTreeMap<String, table::Value>,
    }
    type PhysicalRow<'query> = table::LeasedRow<'query, FlatRow<'query>>;

    struct ScanCursor<'query, 'body> {
        current: Option<PhysicalRow<'query>>,
        physical: &'query Physical<'body>,
        projection: Option<Projection>,
        next: usize,
        done: bool,
        _allocation: RetainedReservation<'query>,
    }
    impl ScanCursor<'_, '_> {
        fn advance_inner(&mut self) -> Result<bool> {
            let Some(id) = self.physical.table.id_at(self.next) else {
                self.done = true;
                // This used to run after the last consumer returned, not before it.
                self.physical.budget.check()?;
                return Ok(false);
            };
            self.physical.budget.check()?;
            self.next += 1;
            let raw = self.physical.table.leased_row_projected(
                id,
                1 << 20,
                self.projection.as_ref().map(|selected| &selected.names),
            )?;
            self.current = Some(raw.try_map(|raw| {
                self.physical.flatten(
                    raw,
                    self.projection.as_ref().map(|selected| &selected.indices),
                )
            })?);
            Ok(true)
        }
    }
    impl RowCursor for ScanCursor<'_, '_> {
        fn advance(&mut self) -> Result<bool> {
            self.current = None;
            if self.done {
                return Ok(false);
            }
            let result = self.advance_inner();
            if result.is_err() {
                self.done = true;
            }
            result
        }
        fn row(&self) -> &[Scalar] {
            self.current.as_ref().map_or(&[], |row| &row.value().cells)
        }
    }
    struct EqualCursor<'query, 'body> {
        current: Option<PhysicalRow<'query>>,
        physical: &'query Physical<'body>,
        pending: Option<u32>,
        _allocation: RetainedReservation<'query>,
    }
    impl RowCursor for EqualCursor<'_, '_> {
        fn advance(&mut self) -> Result<bool> {
            self.current = None;
            let Some(id) = self.pending.take() else {
                // Physical equality has no trailing budget check.
                return Ok(false);
            };
            let raw = match self.physical.table.leased_row_projected(id, 1 << 20, None) {
                Ok(raw) => raw,
                Err(error) if error.code == "table.record_not_found" => return Ok(false),
                Err(error) => return Err(error),
            };
            // Only decoder misses are handled as empty. Flattening and later
            // consumer errors, even record_not_found, remain the original error.
            self.current = Some(raw.try_map(|raw| self.physical.flatten(raw, None))?);
            Ok(true)
        }
        fn row(&self) -> &[Scalar] {
            self.current.as_ref().map_or(&[], |row| &row.value().cells)
        }
    }

    pub(super) struct Physical<'body> {
        pub table: Arc<table::Table<'body>>,
        schema: Source,
        identity: usize,
        budget: Arc<Budget>,
    }
    impl<'body> Physical<'body> {
        pub fn new(table: table::Table<'body>, budget: Arc<Budget>) -> Result<Self> {
            let (schema, identity) = Self::prepare_schema(&table, &budget)?;
            // One stable owner can be shared by static/effective aliases of the
            // exact same prepared bundle without rebinding or rebuilding its index.
            budget.safety_only().charge(Cost::retained(
                size_of::<table::Table<'body>>() + 2 * size_of::<usize>(),
            ))?;
            Ok(Self {
                table: Arc::new(table),
                schema,
                identity,
                budget,
            })
        }
        pub fn shared(&self) -> Result<Self> {
            let (schema, identity) = Self::prepare_schema(&self.table, &self.budget)?;
            Ok(Self {
                table: self.table.clone(),
                schema,
                identity,
                budget: self.budget.clone(),
            })
        }
        fn prepare_schema(table: &table::Table<'_>, budget: &Budget) -> Result<(Source, usize)> {
            budget.check()?;
            let mut count = 0usize;
            let mut strings = 0usize;
            for field in table.columns() {
                let width = if field.array {
                    field.elements as usize
                } else {
                    1
                };
                count = count
                    .checked_add(width)
                    .filter(|n| *n <= 4096)
                    .ok_or_else(|| {
                        fault(
                            "table.limit_exceeded",
                            3,
                            "SQL table scalar column bound exceeded",
                        )
                    })?;
                strings = strings
                    .checked_add(width.checked_mul(field.name.len() + 12).ok_or_else(limit)?)
                    .ok_or_else(limit)?;
            }
            // Fixed Go TableSource ReserveMetadata(count*128), independently from
            // the conservative Rust allocation estimate. No row/index is decoded.
            let logical = count.checked_mul(128).ok_or_else(limit)?;
            let allocation = count
                .checked_mul(size_of::<String>() + 64)
                .and_then(|n| n.checked_add(strings))
                .ok_or_else(limit)?;
            budget.charge_split(Cost::metadata(logical), Cost::metadata(allocation))?;
            let mut columns = Vec::new();
            columns.try_reserve_exact(count).map_err(|_| limit())?;
            let mut identity = None;
            for field in table.columns() {
                if field.identity {
                    if identity.is_some() || field.array {
                        return Err(binding());
                    }
                    identity = Some(columns.len());
                }
                if field.array {
                    for i in 0..field.elements {
                        columns.push(format!("{}[{i}]", field.name));
                    }
                } else {
                    columns.push(field.name.clone());
                }
            }
            Ok((
                Source {
                    columns,
                    rows: vec![],
                    complete: table.is_complete(),
                },
                identity.ok_or_else(binding)?,
            ))
        }
        pub fn schema(&self) -> &Source {
            &self.schema
        }

        fn flatten<'query>(
            &'query self,
            mut raw: BTreeMap<String, table::Value>,
            selected: Option<&BTreeSet<usize>>,
        ) -> Result<FlatRow<'query>> {
            let bytes = self
                .schema
                .columns
                .len()
                .checked_mul(size_of::<Scalar>())
                .and_then(|n| n.checked_add(64))
                .ok_or_else(limit)?;
            // reserve_retained always has zero logical cost. Borrow the stable
            // owner, not a temporary safety_only view, across cursor suspension.
            let vector = self.budget.reserve_retained(bytes)?;
            let mut row = Vec::new();
            row.try_reserve_exact(self.schema.columns.len())
                .map_err(|_| limit())?;
            let mut offset = 0usize;
            for field in self.table.columns() {
                self.budget.check()?;
                let width = if field.array {
                    field.elements as usize
                } else {
                    1
                };
                let wanted = |i| selected.is_none_or(|indices| indices.contains(&i));
                if !(offset..offset + width).any(wanted) {
                    row.resize(row.len() + width, Scalar::Null);
                    offset += width;
                    continue;
                }
                let value = raw.remove(&field.name).ok_or_else(binding)?;
                if field.array {
                    let table::Value::Array(values) = value else {
                        return Err(binding());
                    };
                    if values.len() != width {
                        return Err(binding());
                    }
                    for value in values {
                        row.push(if wanted(offset) {
                            scalar(value)?
                        } else {
                            Scalar::Null
                        });
                        offset += 1;
                    }
                } else {
                    row.push(scalar(value)?);
                    offset += 1;
                }
            }
            Ok(FlatRow {
                cells: row,
                _vector: vector,
                _raw: raw,
            })
        }
        fn projection(&self, columns: &[usize]) -> Result<Projection> {
            self.budget.check()?;
            if columns.iter().any(|i| *i >= self.schema.columns.len()) {
                return Err(binding());
            }
            let bytes = columns
                .len()
                .checked_mul(192)
                .and_then(|n| {
                    n.checked_add(self.schema.columns.iter().map(String::len).sum::<usize>())
                })
                .ok_or_else(limit)?;
            // Projection's logical charge counts requested scalar indices, even
            // when several array elements select the same named field.
            self.budget.charge_split(
                Cost::metadata(columns.len().checked_mul(128).ok_or_else(limit)?),
                Cost::metadata(bytes),
            )?;
            let indices: BTreeSet<_> = columns.iter().copied().collect();
            let mut names = BTreeSet::new();
            let mut offset = 0usize;
            for field in self.table.columns() {
                let width = if field.array {
                    field.elements as usize
                } else {
                    1
                };
                if (offset..offset + width).any(|i| indices.contains(&i)) {
                    names.insert(field.name.clone());
                }
                offset += width;
            }
            Ok(Projection { indices, names })
        }
    }
    impl RowProvider for Physical<'_> {
        fn cursor(&self, selected: Option<&[usize]>) -> Result<Box<dyn RowCursor + '_>> {
            let projection = selected
                .map(|columns| self.projection(columns))
                .transpose()?;
            self.budget.check()?;
            // Go's default SQL TableSource uses a 4 MiB sorted-ID scan allowance.
            // Rust reuses its already-admitted sorted IDs but retains this limit.
            if self.table.logical_count() > (4 << 20) / 4 {
                return Err(fault(
                    "table.limit_exceeded",
                    3,
                    "SQL table scan index exceeds its bound",
                ));
            }
            let allocation = self
                .budget
                .reserve_retained(size_of::<ScanCursor<'_, '_>>())?;
            Ok(Box::new(ScanCursor {
                current: None,
                physical: self,
                projection,
                next: 0,
                done: false,
                _allocation: allocation,
            }))
        }
        fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
            emit_rows(self.cursor(None)?, emit)
        }
        fn scan_selected(&self, columns: &[usize], emit: &mut RowYield<'_>) -> Result<()> {
            emit_rows(self.cursor(Some(columns))?, emit)
        }
        fn has_equal(&self) -> bool {
            true
        }
        fn equal_cursor(&self, column: &str, value: &Scalar) -> Result<EqualityCursor<'_>> {
            // Match Go's callback order. The evaluator checks original control
            // while evaluating the candidate; wrong-column/NULL/type branches do
            // not perform a speculative table read or replace their first error.
            if !crate::hotfix::remote::equal_fold(column, &self.schema.columns[self.identity]) {
                return Ok(EqualityCursor::Declined);
            }
            let id = match value {
                Scalar::Null => return Ok(EqualityCursor::Empty),
                Scalar::Int(n) => match u32::try_from(*n) {
                    Ok(n) => n,
                    Err(_) => return Ok(EqualityCursor::Empty),
                },
                Scalar::UInt(n) => match u32::try_from(*n) {
                    Ok(n) => n,
                    Err(_) => return Ok(EqualityCursor::Empty),
                },
                Scalar::Float(n) if !n.is_finite() => {
                    return Err(fault(
                        "relational.numeric_range",
                        4,
                        "SQL number exceeds supported finite 64-bit range",
                    ));
                }
                Scalar::Float(n) if *n < 0.0 || *n > u32::MAX as f64 || n.trunc() != *n => {
                    return Ok(EqualityCursor::Empty);
                }
                Scalar::Float(n) => *n as u32,
                _ => {
                    return Err(fault(
                        "relational.type_mismatch",
                        4,
                        "SQL scalar types are incompatible",
                    ));
                }
            };
            let allocation = self
                .budget
                .reserve_retained(size_of::<EqualCursor<'_, '_>>())?;
            Ok(EqualityCursor::Rows(Box::new(EqualCursor {
                current: None,
                physical: self,
                pending: Some(id),
                _allocation: allocation,
            })))
        }

        fn equal(
            &self,
            column: &str,
            value: &Scalar,
            emit: &mut RowYield<'_>,
        ) -> Result<EqualityOutcome> {
            match self.equal_cursor(column, value)? {
                EqualityCursor::Declined => Ok(EqualityOutcome::Declined),
                EqualityCursor::Empty => Ok(EqualityOutcome::Handled),
                EqualityCursor::Rows(cursor) => {
                    emit_rows(cursor, emit)?;
                    Ok(EqualityOutcome::Handled)
                }
            }
        }
    }

    pub(super) fn scan_bytes() -> u64 {
        size_of::<ScanCursor<'_, '_>>() as u64
    }
    pub(super) fn equal_bytes() -> u64 {
        size_of::<EqualCursor<'_, '_>>() as u64
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    logical: Cost,
    safety: Cost,
    refundable: u64,
}
impl Snapshot {
    fn read(budget: &Budget, cursor_bytes: u64) -> Self {
        let mut safety = budget.snapshot().unwrap();
        safety.retained_bytes = safety.retained_bytes.checked_sub(cursor_bytes).unwrap();
        Self {
            logical: budget.logical_snapshot().unwrap(),
            safety,
            refundable: budget
                .refundable_retained_bytes()
                .checked_sub(cursor_bytes)
                .unwrap(),
        }
    }
}

struct Pair<'body> {
    current: Physical<'body>,
    legacy: legacy::Physical<'body>,
    current_budget: Arc<Budget>,
    legacy_budget: Arc<Budget>,
    current_cancel: Arc<AtomicBool>,
    legacy_cancel: Arc<AtomicBool>,
}
impl<'body> Pair<'body> {
    fn new(raw: &'body [u8], definition: &schema::Definition) -> Self {
        Self::available(raw, definition, &BTreeSet::new(), &[])
    }
    fn available(
        raw: &'body [u8],
        definition: &schema::Definition,
        unavailable: &BTreeSet<usize>,
        ranges: &[Range<usize>],
    ) -> Self {
        let deadline = Instant::now() + Duration::from_secs(30);
        let current_cancel = Arc::new(AtomicBool::new(false));
        let legacy_cancel = Arc::new(AtomicBool::new(false));
        let current_budget = Arc::new(Budget::new(
            Cost::defaults(),
            deadline,
            Some(current_cancel.clone()),
        ));
        let legacy_budget = Arc::new(Budget::new(
            Cost::defaults(),
            deadline,
            Some(legacy_cancel.clone()),
        ));
        let open = |budget: &Arc<Budget>| {
            table::open_budgeted(
                raw,
                definition,
                Default::default(),
                unavailable,
                ranges,
                budget.clone(),
            )
            .unwrap()
        };
        let current = Physical::new(open(&current_budget), current_budget.clone()).unwrap();
        let legacy = legacy::Physical::new(open(&legacy_budget), legacy_budget.clone()).unwrap();
        let pair = Self {
            current,
            legacy,
            current_budget,
            legacy_budget,
            current_cancel,
            legacy_cancel,
        };
        assert_eq!(pair.current.schema().columns, pair.legacy.schema().columns);
        assert_eq!(
            pair.current.schema().complete,
            pair.legacy.schema().complete
        );
        assert!(pair.current.schema().rows.is_empty());
        assert!(pair.legacy.schema().rows.is_empty());
        pair.check(0, 0);
        pair
    }
    fn check(&self, current_cursor: u64, legacy_cursor: u64) {
        // The only accepted safety-ledger difference is the actual cursor's
        // sizeof. Row scratch, projection metadata and every refund must match.
        assert_eq!(
            Snapshot::read(&self.current_budget, current_cursor),
            Snapshot::read(&self.legacy_budget, legacy_cursor),
        );
    }
    fn cancel(&self, value: bool) {
        self.current_cancel.store(value, Ordering::Release);
        self.legacy_cancel.store(value, Ordering::Release);
    }
    fn leave_work(&self, remaining: u64) {
        for budget in [&self.current_budget, &self.legacy_budget] {
            let used = budget.logical_snapshot().unwrap().decode_work;
            budget
                .charge(Cost {
                    decode_work: Cost::defaults().decode_work - used - remaining,
                    ..Cost::default()
                })
                .unwrap();
        }
        self.check(0, 0);
    }
}

fn error_value(error: &lycheedev_core::ToolkitError) -> serde_json::Value {
    serde_json::to_value(error).unwrap()
}
fn same_error(current: &lycheedev_core::ToolkitError, legacy: &lycheedev_core::ToolkitError) {
    // Preserve message, exit code and all structured details, not merely code.
    assert_eq!(error_value(current), error_value(legacy));
}
fn exact_row(current: &[Scalar], legacy: &[Scalar]) {
    assert_eq!(current.len(), legacy.len());
    for (index, (current, legacy)) in current.iter().zip(legacy).enumerate() {
        match (current, legacy) {
            (Scalar::Float(a), Scalar::Float(b)) => {
                assert_eq!(a.to_bits(), b.to_bits(), "float cell {index}")
            }
            _ => assert_eq!(current, legacy, "cell {index}"),
        }
    }
}
#[derive(Debug)]
struct ScanResult {
    rows: Vec<Vec<Scalar>>,
    error: Option<serde_json::Value>,
}
fn compare_scan(pair: &Pair<'_>, selected: Option<&[usize]>) -> ScanResult {
    let prior = pair.current_budget.refundable_retained_bytes();
    let bindings = table::projected_row_bindings();
    let (mut current, mut legacy) =
        match (pair.current.cursor(selected), pair.legacy.cursor(selected)) {
            (Ok(current), Ok(legacy)) => (current, legacy),
            (Err(current), Err(legacy)) => {
                same_error(&current, &legacy);
                pair.check(0, 0);
                assert_eq!(pair.current_budget.refundable_retained_bytes(), prior);
                return ScanResult {
                    rows: vec![],
                    error: Some(error_value(&current)),
                };
            }
            _ => panic!("cursor creation diverged"),
        };
    let opened_bindings = table::projected_row_bindings();
    assert_eq!(opened_bindings - bindings, usize::from(selected.is_some()));
    let current_bytes = size_of::<ScanCursor<'_, '_>>() as u64;
    pair.check(current_bytes, legacy::scan_bytes());
    let mut rows = vec![];
    let error = loop {
        let a = current.advance();
        let b = legacy.advance();
        pair.check(current_bytes, legacy::scan_bytes());
        assert_eq!(
            table::projected_row_bindings(),
            opened_bindings,
            "selection rebound during advance"
        );
        match (a, b) {
            (Ok(true), Ok(true)) => {
                exact_row(current.row(), legacy.row());
                let before = Snapshot::read(&pair.current_budget, current_bytes);
                exact_row(current.row(), legacy.row());
                assert_eq!(
                    Snapshot::read(&pair.current_budget, current_bytes),
                    before,
                    "row() decoded or charged twice"
                );
                rows.push(current.row().to_vec());
            }
            (Ok(false), Ok(false)) => break None,
            (Err(current), Err(legacy)) => {
                same_error(&current, &legacy);
                break Some(error_value(&current));
            }
            (a, b) => panic!("advance diverged: {a:?} / {b:?}"),
        }
    };
    assert!(current.row().is_empty());
    assert!(legacy.row().is_empty());
    let stopped = Snapshot::read(&pair.current_budget, current_bytes);
    assert!(!current.advance().unwrap());
    assert!(!legacy.advance().unwrap());
    assert_eq!(Snapshot::read(&pair.current_budget, current_bytes), stopped);
    drop(current);
    drop(legacy);
    pair.check(0, 0);
    assert_eq!(pair.current_budget.refundable_retained_bytes(), prior);
    ScanResult { rows, error }
}

fn compare_equal(pair: &Pair<'_>, column: &str, value: &Scalar) -> ScanResult {
    let prior = pair.current_budget.refundable_retained_bytes();
    let bindings = table::projected_row_bindings();
    let result = match (
        pair.current.equal_cursor(column, value),
        pair.legacy.equal_cursor(column, value),
    ) {
        (Ok(EqualityCursor::Declined), Ok(EqualityCursor::Declined))
        | (Ok(EqualityCursor::Empty), Ok(EqualityCursor::Empty)) => ScanResult {
            rows: vec![],
            error: None,
        },
        (Err(current), Err(legacy)) => {
            same_error(&current, &legacy);
            ScanResult {
                rows: vec![],
                error: Some(error_value(&current)),
            }
        }
        (Ok(EqualityCursor::Rows(mut current)), Ok(EqualityCursor::Rows(mut legacy))) => {
            let current_bytes = size_of::<EqualCursor<'_, '_>>() as u64;
            pair.check(current_bytes, legacy::equal_bytes());
            let mut rows = vec![];
            let error = match (current.advance(), legacy.advance()) {
                (Ok(true), Ok(true)) => {
                    exact_row(current.row(), legacy.row());
                    rows.push(current.row().to_vec());
                    None
                }
                (Ok(false), Ok(false)) => None,
                (Err(current), Err(legacy)) => {
                    same_error(&current, &legacy);
                    Some(error_value(&current))
                }
                (a, b) => panic!("equality diverged: {a:?} / {b:?}"),
            };
            pair.check(current_bytes, legacy::equal_bytes());
            assert!(!current.advance().unwrap());
            assert!(!legacy.advance().unwrap());
            assert!(current.row().is_empty());
            assert!(legacy.row().is_empty());
            ScanResult { rows, error }
        }
        _ => panic!("equality dispatch diverged"),
    };
    assert_eq!(
        table::projected_row_bindings(),
        bindings,
        "full equality does not bind a projection"
    );
    pair.check(0, 0);
    assert_eq!(pair.current_budget.refundable_retained_bytes(), prior);
    result
}

fn put32(raw: &mut [u8], at: usize, value: u32) {
    raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn definition(spec: &fixture::Table) -> schema::Definition {
    schema::Document::parse(spec.dbd.as_bytes())
        .unwrap()
        .select(fixture::BUILD, &format!("{:08X}", spec.layout))
        .unwrap()
}
fn relation_field(template: &schema::Field) -> schema::Field {
    let mut field = template.clone();
    field.name = "MissingRelation".into();
    field.inline = false;
    field.identity = false;
    field.relation = true;
    field.array = false;
    field.elements = 1;
    field
}
struct Wide {
    raw: Vec<u8>,
    definition: schema::Definition,
    expected: Vec<Vec<Scalar>>,
}
fn wide() -> Wide {
    use fixture::Cell::{Array, F, I, I64, Text, U};
    let mut fields: Vec<(String, &str)> = [
        ("ID", "uint"),
        ("ZSigned64", "signed64"),
        ("AUnsigned64", "signed64"),
        ("Signed32", "signed"),
        ("Fraction", "float"),
        ("NegativeZero", "float"),
        ("Unicode", "text"),
        ("Empty", "text"),
        ("UnsignedArray", "array3"),
        ("SignedArray", "array2signed"),
        ("FloatArray", "array2float"),
        ("StringArray", "array2signed"),
    ]
    .into_iter()
    .map(|(name, kind)| (name.into(), kind))
    .collect();
    fields.extend((0..40).map(|i| (format!("Extra{:02}", 39 - i), "signed")));
    let mut first = vec![
        U(10),
        I64(i64::MIN + 1),
        I64(-1),
        I(i32::MIN),
        F(0.1),
        F(-0.0),
        Text("荔枝🙂 and text".into()),
        Text(String::new()),
        Array(vec![U(0), U(u32::MAX), U(17)]),
        Array(vec![I(-1), I(i32::MAX)]),
        Array(vec![F(f32::MIN_POSITIVE), F(-0.0)]),
        Array(vec![Text("甲".into()), Text("乙🙂".into())]),
    ];
    let mut second = vec![
        U(20),
        I64(i64::MAX),
        I64(i64::MIN),
        I(i32::MAX),
        F(-123.75),
        F(0.0),
        Text("long🙂".repeat(200)),
        Text(String::new()),
        Array(vec![U(9), U(8), U(7)]),
        Array(vec![I(i32::MIN), I(0)]),
        Array(vec![F(0.25), F(f32::MAX)]),
        Array(vec![Text(String::new()), Text("终".into())]),
    ];
    first.extend((0..40).map(|i| I(-1000 - i)));
    second.extend((0..40).map(|i| I(1000 + i)));
    let refs: Vec<_> = fields
        .iter()
        .map(|(name, kind)| (name.as_str(), *kind))
        .collect();
    let mut spec = fixture::table("ProjectionWide", 801, &refs, &[first, second]);
    let mut definition = definition(&spec);
    definition.fields[2].signed = false;
    definition.fields[11].kind = "locstring".into();
    let relation = relation_field(&definition.fields[0]);
    definition.fields.insert(1, relation);
    put32(&mut spec.raw, 108, 2);
    // Reverse copy-chain order exercises the original resolved physical owner.
    for value in [101u32, 100, 100, 10] {
        spec.raw.extend(value.to_le_bytes());
    }
    let mut first = vec![
        Scalar::UInt(10),
        Scalar::Null,
        Scalar::Int(i64::MIN + 1),
        Scalar::UInt(u64::MAX),
        Scalar::Int(i32::MIN as i64),
        Scalar::Float(0.1f32 as f64),
        Scalar::Float(-0.0),
        Scalar::String("荔枝🙂 and text".into()),
        Scalar::String(String::new()),
        Scalar::UInt(0),
        Scalar::UInt(u32::MAX as u64),
        Scalar::UInt(17),
        Scalar::Int(-1),
        Scalar::Int(i32::MAX as i64),
        Scalar::Float(f32::MIN_POSITIVE as f64),
        Scalar::Float(-0.0),
        Scalar::String("甲".into()),
        Scalar::String("乙🙂".into()),
    ];
    let mut second = vec![
        Scalar::UInt(20),
        Scalar::Null,
        Scalar::Int(i64::MAX),
        Scalar::UInt(1u64 << 63),
        Scalar::Int(i32::MAX as i64),
        Scalar::Float(-123.75),
        Scalar::Float(0.0),
        Scalar::String("long🙂".repeat(200)),
        Scalar::String(String::new()),
        Scalar::UInt(9),
        Scalar::UInt(8),
        Scalar::UInt(7),
        Scalar::Int(i32::MIN as i64),
        Scalar::Int(0),
        Scalar::Float(0.25),
        Scalar::Float(f32::MAX as f64),
        Scalar::String(String::new()),
        Scalar::String("终".into()),
    ];
    first.extend((0..40).map(|i| Scalar::Int(-1000 - i)));
    second.extend((0..40).map(|i| Scalar::Int(1000 + i)));
    let mut copy100 = first.clone();
    copy100[0] = Scalar::UInt(100);
    let mut copy101 = first.clone();
    copy101[0] = Scalar::UInt(101);
    Wide {
        raw: spec.raw,
        definition,
        expected: vec![first, second, copy100, copy101],
    }
}

#[test]
fn projected_wide_full_rows_and_copy_equality_match_frozen_complete_producer() {
    let spec = wide();
    assert_eq!(spec.definition.fields.len(), 53);
    let pair = Pair::new(&spec.raw, &spec.definition);
    assert_eq!(pair.current.schema().columns.len(), 58);
    let result = compare_scan(&pair, None);
    assert!(result.error.is_none());
    assert_eq!(result.rows.len(), spec.expected.len());
    for (row, expected) in result.rows.iter().zip(&spec.expected) {
        exact_row(row, expected);
    }
    for (id, expected) in [10, 20, 100, 101].into_iter().zip(&spec.expected) {
        let result = compare_equal(&pair, "id", &Scalar::UInt(id));
        assert!(result.error.is_none());
        assert_eq!(result.rows.len(), 1);
        exact_row(&result.rows[0], expected);
    }
    assert!(
        compare_equal(&pair, "ID", &Scalar::UInt(999))
            .rows
            .is_empty()
    );
}

#[test]
fn projected_wide_sparse_empty_duplicate_and_out_of_order_selection_match_oracle() {
    let spec = wide();
    for selected in [
        vec![],
        vec![57],
        vec![1, 3, 6, 10, 13, 15, 17, 57],
        vec![57, 10, 10, 3, 17, 0, 0],
        (0..58).rev().collect(),
    ] {
        let pair = Pair::new(&spec.raw, &spec.definition);
        let before = pair.current_budget.logical_snapshot().unwrap();
        let result = compare_scan(&pair, Some(&selected));
        assert!(result.error.is_none());
        assert_eq!(result.rows.len(), 4);
        for (row, full) in result.rows.iter().zip(&spec.expected) {
            let expected: Vec<_> = full
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    if selected.contains(&index) {
                        value.clone()
                    } else {
                        Scalar::Null
                    }
                })
                .collect();
            exact_row(row, &expected);
        }
        let after = pair.current_budget.logical_snapshot().unwrap();
        assert_eq!(
            after.metadata_bytes - before.metadata_bytes,
            selected.len() as u64 * 128
        );
        assert_eq!(
            after.retained_bytes - before.retained_bytes,
            selected.len() as u64 * 128
        );
    }
}

#[test]
fn projected_invalid_selection_precedes_metadata_and_decode_and_plan_rejects_invalid_ordinals() {
    let spec = wide();
    for selected in [
        vec![58],
        vec![0, 58],
        vec![usize::MAX],
        vec![57, 57, usize::MAX],
    ] {
        let pair = Pair::new(&spec.raw, &spec.definition);
        let before = Snapshot::read(&pair.current_budget, 0);
        let result = compare_scan(&pair, Some(&selected));
        assert_eq!(result.error.unwrap()["code"], "relational.invalid_binding");
        assert_eq!(Snapshot::read(&pair.current_budget, 0), before);
    }
    let pair = Pair::new(&spec.raw, &spec.definition);
    for ordinals in [vec![0, 0], vec![2, 1], vec![53], vec![usize::MAX]] {
        let before = Snapshot::read(&pair.current_budget, 0);
        assert_eq!(
            pair.current
                .table
                .projected_rows(ordinals)
                .err()
                .unwrap()
                .code,
            "table.invalid_format"
        );
        assert_eq!(Snapshot::read(&pair.current_budget, 0), before);
    }
}

#[test]
fn projected_late_unselected_bad_string_and_float_are_not_skipped_or_prefetched() {
    for kind in ["string", "float"] {
        let mut spec = wide();
        let last = spec.definition.fields.last_mut().unwrap();
        last.kind = kind.into();
        let start = u32::from_le_bytes(spec.raw[80..84].try_into().unwrap()) as usize;
        let stride = u32::from_le_bytes(spec.raw[12..16].try_into().unwrap()) as usize;
        put32(&mut spec.raw, start + stride - 4, 0);
        put32(
            &mut spec.raw,
            start + 2 * stride - 4,
            if kind == "string" {
                0x7fff_ffff
            } else {
                f32::NAN.to_bits()
            },
        );
        for selected in [None, Some(vec![]), Some(vec![0, 3])] {
            let pair = Pair::new(&spec.raw, &spec.definition);
            let result = compare_scan(&pair, selected.as_deref());
            assert_eq!(
                result.rows.len(),
                1,
                "first valid row must reach consumer before row two"
            );
            assert_eq!(result.error.unwrap()["code"], "table.invalid_format");
        }
    }
}

#[test]
fn projected_complete_decode_error_precedes_early_sql_array_shape_error() {
    let mut spec = wide();
    // Table's public typed Definition input permits this noninline array; its
    // missing relation decodes as Null, which SQL must reject only after the
    // complete decoder has validated the later fields. DBD text rejects it.
    spec.definition.fields[1].array = true;
    spec.definition.fields[1].elements = 2;
    let pair = Pair::new(&spec.raw, &spec.definition);
    let baseline = compare_scan(&pair, None);
    assert_eq!(
        baseline.error.unwrap()["code"],
        "relational.invalid_binding"
    );
    spec.definition.fields.last_mut().unwrap().kind = "string".into();
    for selected in [None, Some(vec![0, 1])] {
        let pair = Pair::new(&spec.raw, &spec.definition);
        let result = compare_scan(&pair, selected.as_deref());
        assert!(result.rows.is_empty());
        assert_eq!(result.error.unwrap()["code"], "table.invalid_format");
        let lookup = compare_equal(&pair, "ID", &Scalar::UInt(10));
        assert_eq!(lookup.error.unwrap()["code"], "table.invalid_format");
    }
}

fn sparse(padding: u8, array_relation: bool) -> (Vec<u8>, schema::Definition) {
    let mut raw = vec![0; 141];
    raw[..4].copy_from_slice(b"WDC3");
    for (at, value) in [
        (4, 1),
        (8, 1),
        (12, 1),
        (20, 0xaabbccdd),
        (24, 0x12345678),
        (28, 10),
        (32, 10),
        (44, 1),
        (56, 24),
        (68, 1),
        (80, 140),
        (84, 1),
        (92, 141),
        (104, 1),
    ] {
        put32(&mut raw, at, value);
    }
    raw[40..42].copy_from_slice(&1u16.to_le_bytes());
    raw[118..120].copy_from_slice(&4u16.to_le_bytes());
    raw[140] = 7 | padding;
    raw.extend(140u32.to_le_bytes());
    raw.extend(1u16.to_le_bytes());
    raw.extend(10u32.to_le_bytes());
    let mut definition = schema::Document::parse(b"COLUMNS\nint ID\nint Value\nint Parent\n\nBUILD 12.1.0.69875\nLAYOUT 12345678\n$noninline,id$ID\n$noninline,relation$Parent\nValue<8>\n")
        .unwrap().select("12.1.0.69875", "12345678").unwrap();
    // Exercise a sub-byte sparse field through the accepted typed definition.
    definition.fields[2].bits = 4;
    if array_relation {
        definition.fields[1].array = true;
        definition.fields[1].elements = 2;
    }
    (raw, definition)
}

#[test]
fn projected_sparse_terminal_padding_precedes_sql_shape_and_respects_unused_fields() {
    let (raw, definition) = sparse(0, false);
    let pair = Pair::new(&raw, &definition);
    let result = compare_scan(&pair, None);
    assert!(result.error.is_none());
    exact_row(
        &result.rows[0],
        &[Scalar::UInt(10), Scalar::Null, Scalar::Int(7)],
    );
    let (raw, definition) = sparse(0, true);
    let pair = Pair::new(&raw, &definition);
    assert_eq!(
        compare_scan(&pair, None).error.unwrap()["code"],
        "relational.invalid_binding"
    );
    for array_relation in [false, true] {
        let (raw, definition) = sparse(0x80, array_relation);
        for selected in [None, Some(vec![0]), Some(vec![1]), Some(vec![])] {
            let pair = Pair::new(&raw, &definition);
            let result = compare_scan(&pair, selected.as_deref());
            assert!(result.rows.is_empty());
            assert_eq!(result.error.unwrap()["code"], "table.invalid_format");
        }
    }
}

#[test]
fn projected_partial_cross_partition_strings_keep_full_coverage_and_unselected_failures() {
    for gap in [0, 4, 7, 8, 16, 512] {
        let spec = strings_fixture::table(802, gap, b"visible\0");
        let pool = strings_fixture::pool(&spec.raw);
        for selected in [None, Some(vec![]), Some(vec![0]), Some(vec![1, 1])] {
            let pair = Pair::available(
                &spec.raw,
                &definition(&spec),
                &BTreeSet::from([1]),
                &[pool + gap..pool + gap + 8],
            );
            assert!(!pair.current.schema().complete);
            assert_eq!(pair.current.table.coverage(), pair.legacy.table.coverage());
            let result = compare_scan(&pair, selected.as_deref());
            if gap < 8 {
                assert!(result.rows.is_empty());
                assert_eq!(result.error.unwrap()["code"], "table.partition_unavailable");
            } else {
                assert!(result.error.is_none());
                assert_eq!(result.rows.len(), 1);
                assert!(!pair.current.schema().complete);
            }
            // The unavailable second physical ID is still a decoder miss.
            assert!(compare_equal(&pair, "ID", &Scalar::UInt(2)).rows.is_empty());
        }
    }
}

#[test]
fn projected_exact_decode_work_boundary_matches_full_oracle_without_refunding_work() {
    let spec = wide();
    let probe = Pair::new(&spec.raw, &spec.definition);
    let before = probe.current_budget.logical_snapshot().unwrap().decode_work;
    assert_eq!(compare_equal(&probe, "ID", &Scalar::UInt(10)).rows.len(), 1);
    let row_work = probe.current_budget.logical_snapshot().unwrap().decode_work - before;
    assert!(
        row_work
            >= 1 + spec
                .definition
                .fields
                .iter()
                .map(|f| f.elements as u64)
                .sum::<u64>()
    );
    for remaining in [0, row_work - 1, row_work, row_work + 1] {
        for selected in [None, Some(vec![]), Some(vec![0, 3, 10, 57])] {
            let pair = Pair::new(&spec.raw, &spec.definition);
            pair.leave_work(remaining);
            let before = pair.current_budget.logical_snapshot().unwrap();
            let result = compare_scan(&pair, selected.as_deref());
            assert_eq!(result.rows.len(), usize::from(remaining >= row_work));
            assert_eq!(
                result.error.unwrap()["code"],
                "records.query_resource_budget"
            );
            assert!(
                pair.current_budget.logical_snapshot().unwrap().decode_work >= before.decode_work
            );
            assert_eq!(pair.current_budget.refundable_retained_bytes(), 0);
        }
    }
}

#[test]
fn projected_cancellation_before_open_between_rows_and_terminal_preserves_oracle_order() {
    let spec = wide();
    let pair = Pair::new(&spec.raw, &spec.definition);
    pair.cancel(true);
    for selected in [None, Some(vec![]), Some(vec![usize::MAX])] {
        assert_eq!(
            compare_scan(&pair, selected.as_deref()).error.unwrap()["code"],
            "operation.cancelled"
        );
    }
    for (column, value, expected) in [
        ("wrong", Scalar::Bool(true), None),
        ("ID", Scalar::Null, None),
        ("ID", Scalar::Bool(true), Some("relational.type_mismatch")),
        (
            "ID",
            Scalar::Float(f64::NAN),
            Some("relational.numeric_range"),
        ),
        ("ID", Scalar::UInt(10), Some("operation.cancelled")),
    ] {
        let result = compare_equal(&pair, column, &value);
        assert_eq!(
            result.error.as_ref().and_then(|e| e["code"].as_str()),
            expected
        );
    }
    pair.cancel(false);
    for stop_after in [1, 4] {
        let mut current = pair.current.cursor(Some(&[0, 3, 57])).unwrap();
        let mut legacy = pair.legacy.cursor(Some(&[0, 3, 57])).unwrap();
        for _ in 0..stop_after {
            assert!(current.advance().unwrap());
            assert!(legacy.advance().unwrap());
            exact_row(current.row(), legacy.row());
        }
        pair.cancel(true);
        same_error(
            &current.advance().unwrap_err(),
            &legacy.advance().unwrap_err(),
        );
        assert!(current.row().is_empty());
        pair.check(size_of::<ScanCursor<'_, '_>>() as u64, legacy::scan_bytes());
        assert!(!current.advance().unwrap());
        assert!(!legacy.advance().unwrap());
        drop(current);
        drop(legacy);
        pair.check(0, 0);
        assert_eq!(pair.current_budget.refundable_retained_bytes(), 0);
        pair.cancel(false);
    }
    let EqualityCursor::Rows(mut current) =
        pair.current.equal_cursor("ID", &Scalar::UInt(10)).unwrap()
    else {
        panic!("row cursor");
    };
    let EqualityCursor::Rows(mut legacy) =
        pair.legacy.equal_cursor("ID", &Scalar::UInt(10)).unwrap()
    else {
        panic!("row cursor");
    };
    assert!(current.advance().unwrap());
    assert!(legacy.advance().unwrap());
    pair.cancel(true);
    assert!(!current.advance().unwrap());
    assert!(!legacy.advance().unwrap());
    drop(current);
    drop(legacy);
    pair.check(0, 0);
    assert_eq!(pair.current_budget.refundable_retained_bytes(), 0);
}

#[test]
fn projected_consumer_error_and_early_drop_refund_current_row_but_preserve_metadata() {
    let spec = wide();
    let pair = Pair::new(&spec.raw, &spec.definition);
    for selected in [None, Some(vec![0, 3, 10, 17, 57])] {
        let before = pair.current_budget.logical_snapshot().unwrap();
        let mut current = pair.current.cursor(selected.as_deref()).unwrap();
        let mut legacy = pair.legacy.cursor(selected.as_deref()).unwrap();
        assert!(current.advance().unwrap());
        assert!(legacy.advance().unwrap());
        exact_row(current.row(), legacy.row());
        pair.check(size_of::<ScanCursor<'_, '_>>() as u64, legacy::scan_bytes());
        assert!(
            pair.current_budget.refundable_retained_bytes()
                > size_of::<ScanCursor<'_, '_>>() as u64
        );
        drop(current);
        drop(legacy);
        pair.check(0, 0);
        assert_eq!(pair.current_budget.refundable_retained_bytes(), 0);
        let after = pair.current_budget.logical_snapshot().unwrap();
        assert!(after.decode_work > before.decode_work);
        assert_eq!(
            after.metadata_bytes - before.metadata_bytes,
            selected.as_ref().map_or(0, |v| v.len() as u64 * 128)
        );
    }
    let sentinel = || {
        fault(
            "table.record_not_found",
            4,
            "consumer sentinel with full details",
        )
        .with_details(json!({"phase": "after_row", "ordinal": 1, "nested": [null, 9, "保留"]}))
    };
    let current = pair
        .current
        .equal("ID", &Scalar::UInt(100), &mut |_| Err(sentinel()))
        .unwrap_err();
    let legacy = pair
        .legacy
        .equal("ID", &Scalar::UInt(100), &mut |_| Err(sentinel()))
        .unwrap_err();
    same_error(&current, &legacy);
    assert_eq!(error_value(&current), error_value(&sentinel()));
    pair.check(0, 0);
    assert_eq!(pair.current_budget.refundable_retained_bytes(), 0);
}

fn fill_retained(budget: &Budget, allowance: u64) -> RetainedReservation<'_> {
    let used = budget.snapshot().unwrap().retained_bytes;
    budget
        .reserve_retained((Cost::defaults().retained_bytes - used - allowance) as usize)
        .unwrap()
}

#[test]
fn projected_exact_retained_boundary_keeps_decoder_then_flatten_admission_and_refunds() {
    let spec = wide();
    for selected in [None, Some(vec![0, 3, 10, 17, 57]), Some(vec![])] {
        // Observe only the frozen producer's complete first-row obligation.
        // Its map and SQL vector guards coexist until the consumer returns.
        let probe = Pair::new(&spec.raw, &spec.definition);
        let mut old_cursor = probe.legacy.cursor(selected.as_deref()).unwrap();
        let opened = probe.legacy_budget.refundable_retained_bytes();
        assert!(old_cursor.advance().unwrap());
        let row_bytes = probe.legacy_budget.refundable_retained_bytes() - opened;
        assert!(row_bytes > spec.expected[0].len() as u64 * size_of::<Scalar>() as u64);
        drop(old_cursor);
        for allowance in [0, row_bytes - 1, row_bytes, row_bytes + 1] {
            let pair = Pair::new(&spec.raw, &spec.definition);
            let mut current = pair.current.cursor(selected.as_deref()).unwrap();
            let mut legacy = pair.legacy.cursor(selected.as_deref()).unwrap();
            // Give both implementations identical absolute headroom after
            // their separately recorded cursor sizeof charges. These guards
            // admit accounting only; no enormous fixture allocation is made.
            let current_fill = fill_retained(&pair.current_budget, allowance);
            let legacy_fill = fill_retained(&pair.legacy_budget, allowance);
            assert_eq!(
                Snapshot::read(&pair.current_budget, 0),
                Snapshot::read(&pair.legacy_budget, 0)
            );
            match (current.advance(), legacy.advance()) {
                (Ok(true), Ok(true)) => {
                    assert!(allowance >= row_bytes);
                    exact_row(current.row(), legacy.row());
                    assert_eq!(
                        pair.current_budget.snapshot().unwrap().retained_bytes,
                        Cost::defaults().retained_bytes - allowance + row_bytes
                    );
                }
                (Err(current), Err(legacy)) => {
                    assert!(allowance < row_bytes);
                    same_error(&current, &legacy);
                    assert_eq!(current.code, "records.query_resource_budget");
                    assert_eq!(
                        pair.current_budget.snapshot().unwrap().retained_bytes,
                        Cost::defaults().retained_bytes - allowance
                    );
                }
                (a, b) => panic!("retained admission diverged: {a:?} / {b:?}"),
            }
            assert_eq!(
                Snapshot::read(&pair.current_budget, 0),
                Snapshot::read(&pair.legacy_budget, 0)
            );
            drop(current);
            drop(legacy);
            drop(current_fill);
            drop(legacy_fill);
            pair.check(0, 0);
            assert_eq!(pair.current_budget.refundable_retained_bytes(), 0);
        }
    }
}

#[test]
fn projected_late_decode_error_precedes_exhausted_sql_vector_reservation() {
    let spec = wide();
    let selected = [0, 3];
    let table_bytes = {
        let probe = Pair::new(&spec.raw, &spec.definition);
        let mut cursor = probe.legacy.cursor(Some(&selected)).unwrap();
        let opened = probe.legacy_budget.refundable_retained_bytes();
        assert!(cursor.advance().unwrap());
        let row_bytes = probe.legacy_budget.refundable_retained_bytes() - opened;
        let vector_bytes = (probe.legacy.schema().columns.len() * size_of::<Scalar>() + 64) as u64;
        row_bytes.checked_sub(vector_bytes).unwrap()
    };
    let mut malformed = spec.definition.clone();
    malformed.fields.last_mut().unwrap().kind = "string".into();
    for (definition, expected) in [
        (&spec.definition, "records.query_resource_budget"),
        (&malformed, "table.invalid_format"),
    ] {
        let pair = Pair::new(&spec.raw, definition);
        let mut current = pair.current.cursor(Some(&selected)).unwrap();
        let mut legacy = pair.legacy.cursor(Some(&selected)).unwrap();
        // Exactly enough for all original table row scratch and preceding
        // strings; none of the following SQL scalar vector is admitted.
        let current_fill = fill_retained(&pair.current_budget, table_bytes);
        let legacy_fill = fill_retained(&pair.legacy_budget, table_bytes);
        let current_error = current.advance().unwrap_err();
        let legacy_error = legacy.advance().unwrap_err();
        same_error(&current_error, &legacy_error);
        assert_eq!(current_error.code, expected);
        assert!(current.row().is_empty());
        assert!(legacy.row().is_empty());
        assert_eq!(
            Snapshot::read(&pair.current_budget, 0),
            Snapshot::read(&pair.legacy_budget, 0)
        );
        drop(current);
        drop(legacy);
        drop(current_fill);
        drop(legacy_fill);
        pair.check(0, 0);
        assert_eq!(pair.current_budget.refundable_retained_bytes(), 0);
    }
}

#[test]
fn projected_nested_cursor_leases_match_oracle_without_refunding_suspended_outer_row() {
    let spec = wide();
    let pair = Pair::new(&spec.raw, &spec.definition);
    let bindings = table::projected_row_bindings();
    let mut current = pair.current.cursor(Some(&[0, 3, 7, 17, 57])).unwrap();
    let mut legacy = pair.legacy.cursor(Some(&[0, 3, 7, 17, 57])).unwrap();
    assert!(current.advance().unwrap());
    assert!(legacy.advance().unwrap());
    exact_row(current.row(), legacy.row());
    let outer_row = current.row().to_vec();
    let before = Snapshot::read(&pair.current_budget, 0);
    let EqualityCursor::Rows(mut current_inner) =
        pair.current.equal_cursor("ID", &Scalar::UInt(20)).unwrap()
    else {
        panic!("expected nested current cursor");
    };
    let EqualityCursor::Rows(mut legacy_inner) =
        pair.legacy.equal_cursor("ID", &Scalar::UInt(20)).unwrap()
    else {
        panic!("expected nested oracle cursor");
    };
    assert!(current_inner.advance().unwrap());
    assert!(legacy_inner.advance().unwrap());
    exact_row(current_inner.row(), legacy_inner.row());
    exact_row(current_inner.row(), &spec.expected[1]);
    exact_row(current.row(), &outer_row);
    exact_row(legacy.row(), &outer_row);
    pair.check(
        (size_of::<ScanCursor<'_, '_>>() + size_of::<EqualCursor<'_, '_>>()) as u64,
        legacy::scan_bytes() + legacy::equal_bytes(),
    );
    assert!(pair.current_budget.refundable_retained_bytes() > before.refundable);
    drop(current_inner);
    drop(legacy_inner);
    let after = Snapshot::read(&pair.current_budget, 0);
    assert_eq!(after.refundable, before.refundable);
    assert_eq!(after.safety.retained_bytes, before.safety.retained_bytes);
    assert_eq!(after.logical.metadata_bytes, before.logical.metadata_bytes);
    assert!(after.logical.decode_work > before.logical.decode_work);
    pair.check(size_of::<ScanCursor<'_, '_>>() as u64, legacy::scan_bytes());
    exact_row(current.row(), &outer_row);
    exact_row(legacy.row(), &outer_row);
    assert!(current.advance().unwrap());
    assert!(legacy.advance().unwrap());
    exact_row(current.row(), legacy.row());
    assert_eq!(table::projected_row_bindings(), bindings + 1);
    drop(current);
    drop(legacy);
    pair.check(0, 0);
    assert_eq!(pair.current_budget.refundable_retained_bytes(), 0);
}
