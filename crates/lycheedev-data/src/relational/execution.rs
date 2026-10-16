//! Stackless query driver. A suspended query owns its state, but never owns or
//! polls a child query future. Child requests cross a single owned mailbox;
//! only this loop polls query frames, on the original thread and deadline.
use super::*;
use crate::resource::RetainedReservation;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    ops::Deref,
    pin::Pin,
    rc::Rc,
    task::{Context as PollContext, Poll, Waker},
};

pub(super) struct Held<'a, T> {
    pub value: T,
    _allocation: Option<RetainedReservation<'a>>,
}
impl<T> Deref for Held<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}
pub(super) type Shared<'a, T> = Rc<Held<'a, T>>;
#[derive(Clone)]
pub(super) enum SourceRef<'a> {
    Borrowed(&'a Source),
    Owned(Shared<'a, Source>),
}
impl Deref for SourceRef<'_> {
    type Target = Source;
    fn deref(&self) -> &Source {
        match self {
            Self::Borrowed(s) => s,
            Self::Owned(s) => s,
        }
    }
}
pub(super) type Sources<'a> = BTreeMap<String, SourceRef<'a>>;
#[derive(Clone, Copy)]
pub(super) enum Child {
    Binding(usize),
    Branch(usize),
    Derived,
    Join(usize),
}
#[derive(Clone)]
enum Root<'a> {
    Borrowed(&'a Program),
    Shared(std::sync::Arc<Program>),
}
#[derive(Clone)]
pub(super) struct Query<'a> {
    root: Root<'a>,
    path: Option<Shared<'a, Vec<Child>>>,
}
impl<'a> Query<'a> {
    fn borrowed(program: &'a Program) -> Self {
        Self {
            root: Root::Borrowed(program),
            path: None,
        }
    }
    pub fn shared(program: std::sync::Arc<Program>) -> Self {
        Self {
            root: Root::Shared(program),
            path: None,
        }
    }
    pub fn child(&self, child: Child, control: &Control<'a, '_>) -> Result<Self> {
        let length = self.path.as_ref().map_or(0, |p| p.len()) + 1;
        let path = control.own(length * std::mem::size_of::<Child>(), || {
            let mut path = Vec::with_capacity(length);
            if let Some(old) = &self.path {
                path.extend_from_slice(old);
            }
            path.push(child);
            path
        })?;
        Ok(Self {
            root: self.root.clone(),
            path: Some(path),
        })
    }
}
impl Deref for Query<'_> {
    type Target = Program;
    fn deref(&self) -> &Program {
        let mut value = match &self.root {
            Root::Borrowed(p) => *p,
            Root::Shared(p) => p,
        };
        for child in self.path.as_ref().into_iter().flat_map(|p| p.iter()) {
            value = match *child {
                Child::Binding(i) => &value.bindings[i].1,
                Child::Branch(i) => &value.branches[i],
                Child::Derived => value.derived.as_deref().expect("derived handle"),
                Child::Join(i) => value.joins[i].derived.as_deref().expect("join handle"),
            };
        }
        value
    }
}
#[derive(Clone)]
pub(super) enum Owners<'a> {
    Empty,
    Query(Query<'a>),
    Columns(Shared<'a, Vec<String>>),
}
impl Owners<'_> {
    fn view(&self) -> ColumnOwners<'_> {
        match self {
            Self::Empty => ColumnOwners::Single(""),
            Self::Query(q) => ColumnOwners::Single(&q.alias),
            Self::Columns(c) => ColumnOwners::PerColumn(c),
        }
    }
}
#[derive(Clone)]
pub(super) enum Fields<'a> {
    Borrowed(subquery::Fields<'a>),
    Owned {
        source: SourceRef<'a>,
        owners: Owners<'a>,
        width: usize,
    },
}
impl<'a> Fields<'a> {
    pub fn new(source: SourceRef<'a>, owners: Owners<'a>) -> Self {
        let width = source.columns.len();
        Self::Owned {
            source,
            owners,
            width,
        }
    }
    pub fn prefix(&self, width: usize) -> Self {
        match self {
            Self::Owned { source, owners, .. } => Self::Owned {
                source: source.clone(),
                owners: owners.clone(),
                width,
            },
            Self::Borrowed(f) => Self::Borrowed(subquery::Fields {
                columns: &f.columns[..width],
                alias: f.alias,
            }),
        }
    }
    pub fn view(&self) -> subquery::Fields<'_> {
        match self {
            Self::Borrowed(f) => *f,
            Self::Owned {
                source,
                owners,
                width,
            } => subquery::Fields {
                columns: &source.columns[..*width],
                alias: owners.view(),
            },
        }
    }
}
#[derive(Clone)]
pub(super) struct Correlation<'a> {
    pub fields: Fields<'a>,
    pub mapping: Shared<'a, Vec<Option<usize>>>,
}
impl Correlation<'_> {
    fn view(&self) -> subquery::Correlation<'_> {
        let f = self.fields.view();
        subquery::Correlation {
            columns: f.columns,
            alias: f.alias,
            mapping: &self.mapping,
        }
    }
}
pub(super) type Cursor<'a> = Shared<'a, RefCell<Box<dyn provider::RowCursor + 'a>>>;
#[derive(Clone)]
pub(super) enum Row<'a> {
    Empty,
    Source(SourceRef<'a>, usize),
    Values(Shared<'a, Vec<Scalar>>),
    Mutable(Shared<'a, RefCell<Vec<Scalar>>>),
    Cursor(Cursor<'a>),
}
impl Row<'_> {
    pub fn with<T>(&self, run: impl FnOnce(&[Scalar]) -> T) -> T {
        match self {
            Self::Empty => run(&[]),
            Self::Source(s, i) => run(&s.rows[*i]),
            Self::Values(r) => run(r),
            Self::Mutable(r) => run(&r.borrow()),
            Self::Cursor(c) => run(c.borrow().row()),
        }
    }
}
#[derive(Clone)]
pub(super) enum Outer<'a> {
    Borrowed(&'a dyn subquery::OuterFrame),
    Owned(Shared<'a, Frame<'a>>),
}
impl Outer<'_> {
    fn view(&self) -> &dyn subquery::OuterFrame {
        match self {
            Self::Borrowed(f) => *f,
            Self::Owned(f) => &f.value,
        }
    }
}
pub(super) struct Frame<'a> {
    row: Row<'a>,
    fields: Fields<'a>,
    resolver: Fields<'a>,
    mapping: Option<Shared<'a, Vec<Option<usize>>>>,
    allowed: Option<Correlation<'a>>,
    parent: Option<Outer<'a>>,
}
impl Drop for Frame<'_> {
    fn drop(&mut self) {
        let mut next = self.parent.take();
        while let Some(Outer::Owned(owner)) = next {
            match Rc::try_unwrap(owner) {
                Ok(mut held) => next = held.value.parent.take(),
                Err(_) => break,
            }
        }
    }
}
impl subquery::OuterFrame for Frame<'_> {
    fn fields(&self) -> subquery::Fields<'_> {
        self.fields.view()
    }
    fn parent(&self) -> Option<&dyn subquery::OuterFrame> {
        self.parent.as_ref().map(Outer::view)
    }
    fn allowed(&self) -> Option<subquery::Correlation<'_>> {
        self.allowed.as_ref().map(Correlation::view)
    }
    fn clone_value(&self, index: usize, work: &mut Work<'_>) -> Result<Scalar> {
        let fields = self.fields.view();
        let resolver = self.resolver.view();
        let index = subquery::resolved_index(
            fields,
            resolver,
            self.mapping.as_deref().map(|m| m.value.as_slice()),
            index,
        )?;
        self.row
            .with(|row| work.clone_scalar(row.get(index).ok_or_else(binding)?))
    }
}
#[derive(Clone)]
pub(super) struct Scope<'a> {
    pub outer: Option<Outer<'a>>,
    pub callback: Option<Shared<'a, Sources<'a>>>,
    pub fields: Fields<'a>,
    pub allowed: Option<Correlation<'a>>,
}
impl<'a> Scope<'a> {
    pub fn select(&self, sources: Shared<'a, Sources<'a>>) -> Self {
        Self {
            callback: Some(sources),
            allowed: None,
            ..self.clone()
        }
    }
    fn view(&self) -> subquery::Scope<'_> {
        subquery::Scope {
            outer: self.outer.as_ref().map(Outer::view),
            callback: self.callback.as_ref().map(|s| subquery::Callback::Owned(s)),
            fields: self.fields.view(),
            allowed: self.allowed.as_ref().map(Correlation::view),
        }
    }
}
#[derive(Clone)]
pub(super) struct Context<'a> {
    pub scope: Scope<'a>,
    pub group: Option<Correlation<'a>>,
    pub complete: Shared<'a, Cell<bool>>,
}
impl Context<'_> {
    pub fn view(&self) -> subquery::Context<'_> {
        subquery::Context {
            scope: self.scope.view(),
            group: self.group.as_ref().map(Correlation::view),
            complete: Some(&self.complete),
        }
    }
}
struct Request<'a> {
    query: Query<'a>,
    sources: Shared<'a, Sources<'a>>,
    scope: Scope<'a>,
}
pub(super) struct Control<'a, 'w> {
    work: RefCell<&'a mut Work<'w>>,
    parameters: &'a BTreeMap<String, Scalar>,
    pending: RefCell<Option<Request<'a>>>,
    result: RefCell<Option<Result<QueryResult>>>,
}
impl<'a, 'w: 'a> Control<'a, 'w> {
    pub fn work<T>(&self, run: impl FnOnce(&mut Work<'w>) -> Result<T>) -> Result<T> {
        run(&mut self.work.borrow_mut())
    }
    pub fn planning(&self) -> bool {
        self.work.borrow().planning
    }
    pub fn parameters(&self) -> &BTreeMap<String, Scalar> {
        self.parameters
    }
    pub fn shared_budget(&self) -> Option<&'w crate::resource::Budget> {
        self.work.borrow().shared
    }
    pub fn own<T>(&self, extra: usize, make: impl FnOnce() -> T) -> Result<Shared<'a, T>> {
        let bytes = extra
            .checked_add(std::mem::size_of::<Held<'a, T>>() + 2 * std::mem::size_of::<usize>())
            .ok_or_else(budget)?;
        let allocation = self
            .shared_budget()
            .map(|b| b.reserve_retained(bytes))
            .transpose()?;
        Ok(Rc::new(Held {
            value: make(),
            _allocation: allocation,
        }))
    }
    pub fn share<T>(&self, value: T) -> Result<Shared<'a, T>> {
        self.own(0, || value)
    }
    pub async fn query(
        &self,
        query: Query<'a>,
        sources: Shared<'a, Sources<'a>>,
        scope: Scope<'a>,
    ) -> Result<QueryResult> {
        ChildResult {
            control: self,
            request: Some(Request {
                query,
                sources,
                scope,
            }),
        }
        .await
    }
    pub async fn subquery(
        &self,
        query: std::sync::Arc<Program>,
        row: Row<'a>,
        fields: Fields<'a>,
        context: &Context<'a>,
        any: bool,
    ) -> Result<QueryResult> {
        self.work(|w| w.consume(0))?;
        let sources = context.scope.callback.clone().ok_or_else(unsupported)?;
        let resolver = context.group.as_ref().map_or(fields, |g| g.fields.clone());
        let fields = if self.planning() {
            resolver.clone()
        } else {
            context.scope.fields.clone()
        };
        let allowed = if self.planning() {
            context
                .group
                .clone()
                .or_else(|| context.scope.allowed.clone())
        } else {
            context.scope.allowed.clone()
        };
        let frame = self.share(Frame {
            row,
            fields: fields.clone(),
            resolver,
            mapping: context.group.as_ref().map(|g| g.mapping.clone()),
            allowed: allowed.clone(),
            parent: context.scope.outer.clone(),
        })?;
        let scope = Scope {
            outer: Some(Outer::Owned(frame)),
            fields,
            allowed,
            ..context.scope.clone()
        };
        let result = self.query(Query::shared(query), sources, scope).await?;
        if !result.complete {
            context.complete.set(false);
        }
        if !any && result.columns.len() != 1 {
            return Err(subquery::cardinality());
        }
        Ok(result)
    }
    pub fn source(&self, sources: &Sources<'a>, name: &RelationName) -> Result<SourceRef<'a>> {
        if let Some(key) = name.lexical_key() {
            let mut matched = sources.iter().filter(|(n, _)| identifier_lower(n) == key);
            if let Some((_, s)) = matched.next() {
                if matched.next().is_some() {
                    return Err(binding());
                }
                return Ok(s.clone());
            }
        }
        if let Some(s) = sources.get(&name.physical_key()) {
            self.work(|w| w.measure_root(name))?;
            return Ok(s.clone());
        }
        self.work(|w| w.resolve_root(name)).map(SourceRef::Borrowed)
    }
    pub fn temporary_scope(&self) -> Temporary<'_, 'a, 'w> {
        Temporary {
            control: self,
            mark: self.temporary_mark(),
        }
    }
    pub fn temporary_mark(&self) -> usize {
        self.work.borrow().temporaries.len()
    }
    pub fn temporary_end(&self, mark: usize) {
        self.work.borrow_mut().temporaries.truncate(mark);
    }
}
pub(super) struct Temporary<'c, 'a, 'w> {
    control: &'c Control<'a, 'w>,
    mark: usize,
}
impl Drop for Temporary<'_, '_, '_> {
    fn drop(&mut self) {
        self.control
            .work
            .borrow_mut()
            .temporaries
            .truncate(self.mark);
    }
}
struct ChildResult<'c, 'a, 'w> {
    control: &'c Control<'a, 'w>,
    request: Option<Request<'a>>,
}
impl Future for ChildResult<'_, '_, '_> {
    type Output = Result<QueryResult>;
    fn poll(mut self: Pin<&mut Self>, _: &mut PollContext<'_>) -> Poll<Self::Output> {
        if let Some(request) = self.request.take() {
            assert!(self.control.pending.borrow().is_none());
            *self.control.pending.borrow_mut() = Some(request);
            Poll::Pending
        } else {
            Poll::Ready(
                self.control
                    .result
                    .borrow_mut()
                    .take()
                    .expect("completed child query"),
            )
        }
    }
}

pub(super) fn execute<'a, 'w: 'a>(
    program: &'a Program,
    sources: &'a BTreeMap<String, &'a Source>,
    scope: subquery::Scope<'a>,
    parameters: &'a BTreeMap<String, Scalar>,
    work: &'a mut Work<'w>,
) -> Result<QueryResult> {
    // The original query-entry charge precedes all structural heap admission.
    work.spend()?;
    let control = Control {
        work: RefCell::new(work),
        parameters,
        pending: RefCell::new(None),
        result: RefCell::new(None),
    };
    let map_bytes = |map: &BTreeMap<String, &Source>| -> Result<usize> {
        map.keys().try_fold(0usize, |n, key| {
            n.checked_add(key.len() + 96).ok_or_else(budget)
        })
    };
    let inputs = control.own(map_bytes(sources)?, || {
        sources
            .iter()
            .map(|(n, s)| (n.clone(), SourceRef::Borrowed(*s)))
            .collect::<Sources<'a>>()
    })?;
    let callback = match scope.callback {
        Some(subquery::Callback::Map(map)) => Some(control.own(map_bytes(map)?, || {
            map.iter()
                .map(|(n, s)| (n.clone(), SourceRef::Borrowed(*s)))
                .collect::<Sources<'a>>()
        })?),
        Some(subquery::Callback::Owned(map)) => {
            Some(control.own(map.keys().map(|name| name.len() + 96).sum(), || map.clone())?)
        }
        None => None,
    };
    let allowed = scope
        .allowed
        .map(|a| {
            control
                .own(
                    a.mapping.len() * std::mem::size_of::<Option<usize>>(),
                    || a.mapping.to_vec(),
                )
                .map(|mapping| Correlation {
                    fields: Fields::Borrowed(subquery::Fields {
                        columns: a.columns,
                        alias: a.alias,
                    }),
                    mapping,
                })
        })
        .transpose()?;
    let scope = Scope {
        outer: scope.outer.map(Outer::Borrowed),
        callback,
        fields: Fields::Borrowed(scope.fields),
        allowed,
    };
    drive(
        Request {
            query: Query::borrowed(program),
            sources: inputs,
            scope,
        },
        &control,
    )
}
fn drive<'c, 'a: 'c, 'w: 'a>(
    root: Request<'a>,
    control: &'c Control<'a, 'w>,
) -> Result<QueryResult> {
    struct Entry<'c, 'a> {
        future: Pin<Box<dyn Future<Output = Result<QueryResult>> + 'c>>,
        _allocation: Option<RetainedReservation<'a>>,
        #[cfg(test)]
        _trace: stack_safety_tests::QueryEntry,
    }
    let mut stack = task_stack::Stack::new(control.shared_budget())?;
    let mut request = Some(root);
    let mut first = true;
    let mut cx = PollContext::from_waker(Waker::noop());
    loop {
        if let Some(next) = request.take() {
            if first {
                first = false;
            } else {
                control.work(|w| w.spend())?;
            }
            let future = run(next, control);
            let heap_bytes = std::mem::size_of_val(&future);
            let allocation = control
                .shared_budget()
                .map(|b| b.reserve_retained(heap_bytes))
                .transpose()?;
            #[cfg(test)]
            let marker = 0u8;
            let entry = Entry {
                future: Box::pin(future),
                _allocation: allocation,
                #[cfg(test)]
                _trace: stack_safety_tests::enter_query(&marker, heap_bytes),
            };
            stack.push(entry)?;
        }
        let mut entry = stack.pop().ok_or_else(unsupported)?;
        match poll_query(entry.future.as_mut(), &mut cx) {
            Poll::Pending => {
                request = control.pending.borrow_mut().take();
                if request.is_none() {
                    return Err(unsupported());
                }
                stack.push(entry)?;
            }
            Poll::Ready(result) => {
                drop(entry);
                if stack.len() == 0 {
                    return result;
                }
                *control.result.borrow_mut() = Some(result);
            }
        }
    }
}
async fn run<'a, 'w: 'a>(request: Request<'a>, control: &Control<'a, 'w>) -> Result<QueryResult> {
    execution_query::run(request.query, request.sources, request.scope, control).await
}

#[inline(never)]
fn poll_query(
    future: Pin<&mut dyn Future<Output = Result<QueryResult>>>,
    context: &mut PollContext<'_>,
) -> Poll<Result<QueryResult>> {
    #[cfg(test)]
    let marker = 0u8;
    #[cfg(test)]
    let _poll = stack_safety_tests::enter_poll(&marker);
    future.poll(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_weak_outer_frames_drop_iteratively_without_row_or_source_cycles() {
        let fields = Fields::Borrowed(subquery::Fields {
            columns: &[],
            alias: ColumnOwners::Single(""),
        });
        let mut parent = None;
        let mut weak = Vec::new();
        for _ in 0..16384 {
            let frame = Rc::new(Held {
                value: Frame {
                    row: Row::Empty,
                    fields: fields.clone(),
                    resolver: fields.clone(),
                    mapping: None,
                    allowed: None,
                    parent,
                },
                _allocation: None,
            });
            weak.push(Rc::downgrade(&frame));
            parent = Some(Outer::Owned(frame));
        }
        drop(parent);
        assert!(weak.iter().all(|frame| frame.upgrade().is_none()));
    }
}
