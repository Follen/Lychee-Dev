//! Iterative ownership teardown, including not-yet-admitted lowering results.
//! A rejected long infix tree must not recurse through Box/Vec/Arc destructors.
use super::*;

enum Pending {
    Term(Term),
    Query(Box<Program>),
    Branches(std::vec::IntoIter<Program>),
    Bindings(std::vec::IntoIter<(String, Program)>),
}

impl Drop for Term {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        term(self, &mut pending);
        drain(pending);
    }
}
impl Drop for Program {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        query(self, &mut pending);
        drain(pending);
    }
}
fn drain(mut pending: Vec<Pending>) {
    while let Some(mut item) = pending.pop() {
        match &mut item {
            Pending::Term(value) => term(value, &mut pending),
            Pending::Query(value) => query(value, &mut pending),
            Pending::Branches(values) => {
                for mut value in values.by_ref() {
                    query(&mut value, &mut pending);
                }
            }
            Pending::Bindings(values) => {
                for (_, mut value) in values.by_ref() {
                    query(&mut value, &mut pending);
                }
            }
        }
        // The ordinary Drop calls now see only detached leaves/empty query
        // containers. They cannot descend into any remaining owned subtree.
    }
}
fn child(value: &mut Box<Term>, pending: &mut Vec<Pending>) {
    pending.push(Pending::Term(std::mem::replace(
        value.as_mut(),
        Term::Value(Scalar::Null),
    )));
}
fn empty_query() -> std::sync::Arc<Program> {
    // A permanent leaf lets Drop take ownership of an Arc field safely.
    // Cloning this sentinel allocates nothing and cannot release a query tree.
    static EMPTY: std::sync::OnceLock<std::sync::Arc<Program>> = std::sync::OnceLock::new();
    EMPTY
        .get_or_init(|| {
            std::sync::Arc::new(Program {
                explain: false,
                analyze: false,
                recursive: false,
                bindings: Vec::new(),
                branches: Vec::new(),
                derived: None,
                table: RelationName::internal(""),
                alias: String::new(),
                projection: Vec::new(),
                filter: None,
                order: Vec::new(),
                parameters: BTreeSet::new(),
                limit: None,
                offset: None,
                distinct: false,
                joins: Vec::new(),
                groups: Vec::new(),
                having: None,
            })
        })
        .clone()
}
fn shared(value: &mut std::sync::Arc<Program>, pending: &mut Vec<Pending>) {
    // get_mut is insufficient: a Weak prevents it even for the final strong
    // owner, and another strong owner may disappear immediately afterwards.
    // into_inner atomically either transfers the final body or releases our
    // strong reference without dropping that body. Weak handles do not stop
    // transfer. Thus no Arc destructor can reenter a child Program::drop here.
    let owned = std::mem::replace(value, empty_query());
    if let Some(mut value) = std::sync::Arc::into_inner(owned) {
        query(&mut value, pending);
    }
}
fn term(value: &mut Term, pending: &mut Vec<Pending>) {
    match value {
        Term::Unary(_, input) | Term::NullTest(input, _) | Term::Cast(input, _) => {
            child(input, pending)
        }
        Term::Binary(left, _, right) | Term::Like(left, right, _) => {
            child(left, pending);
            child(right, pending);
        }
        Term::Between(input, low, high, _) => {
            child(input, pending);
            child(low, pending);
            child(high, pending);
        }
        Term::In(input, values, _) => {
            child(input, pending);
            pending.extend(std::mem::take(values).into_iter().map(Pending::Term));
        }
        Term::InSubquery(input, nested, _) => {
            child(input, pending);
            shared(nested, pending);
        }
        Term::ScalarSubquery(nested) | Term::Exists(nested) => shared(nested, pending),
        Term::Call(_, values, _) | Term::Aggregate(_, values, _) => {
            pending.extend(std::mem::take(values).into_iter().map(Pending::Term));
        }
        Term::Case(input, cases, otherwise) => {
            pending.extend(
                input
                    .take()
                    .into_iter()
                    .chain(otherwise.take())
                    .map(|term| Pending::Term(*term)),
            );
            for (condition, result) in std::mem::take(cases) {
                pending.extend([Pending::Term(condition), Pending::Term(result)]);
            }
        }
        _ => {}
    }
}
fn query(value: &mut Program, pending: &mut Vec<Pending>) {
    if !value.bindings.is_empty() {
        pending.push(Pending::Bindings(
            std::mem::take(&mut value.bindings).into_iter(),
        ));
    }
    if !value.branches.is_empty() {
        pending.push(Pending::Branches(
            std::mem::take(&mut value.branches).into_iter(),
        ));
    }
    pending.extend(value.derived.take().map(Pending::Query));
    for projection in &mut value.projection {
        pending.extend(projection.expr.take().map(Pending::Term));
    }
    pending.extend(
        value
            .filter
            .take()
            .into_iter()
            .chain(value.having.take())
            .chain(value.limit.take())
            .chain(value.offset.take())
            .map(Pending::Term),
    );
    pending.extend(
        std::mem::take(&mut value.order)
            .into_iter()
            .map(|(term, _, _)| Pending::Term(term)),
    );
    pending.extend(
        std::mem::take(&mut value.groups)
            .into_iter()
            .map(Pending::Term),
    );
    for link in &mut value.joins {
        pending.extend(link.derived.take().into_iter().map(Pending::Query));
        pending.extend(link.condition.take().map(Pending::Term));
    }
}
