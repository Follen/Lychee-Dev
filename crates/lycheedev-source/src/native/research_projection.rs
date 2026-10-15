//! A single-use move of an already computed projection within one borrowed scope.
//! This is not a session cache: all four inputs must remain alive and immutable.
use serde_json::Value;

// Eligibility bounds transient retention by serialized bytes, not a claim about
// total allocator/RSS usage. No cloned JSON tree or graph is retained.
pub(super) const MAX_RETAINED_BYTES: usize = 1 << 20;

pub(crate) struct RetainedProjection<'a, S, Q, G, O> {
    snapshot: &'a S,
    query: &'a Q,
    graph: Option<&'a G>,
    origins: &'a O,
    result: Value,
    steps: usize,
}
impl<'a, S, Q, G, O> RetainedProjection<'a, S, Q, G, O> {
    pub(super) fn new<E>(
        snapshot: &'a S, query: &'a Q, graph: Option<&'a G>, origins: &'a O,
        result: Value, steps: usize, mut poll: impl FnMut() -> Result<(), E>,
    ) -> Result<Option<Self>, E> {
        if !within_limit(&result, MAX_RETAINED_BYTES, &mut poll)? { return Ok(None); }
        Ok(Some(Self { snapshot, query, graph, origins, result, steps }))
    }
    pub(super) fn matches(&self, snapshot: &S, query: &Q, graph: Option<&G>, origins: &O) -> bool {
        std::ptr::eq(self.snapshot, snapshot) && std::ptr::eq(self.query, query)
            && std::ptr::eq(self.origins, origins) && match (self.graph, graph) {
                (Some(first), Some(second)) => std::ptr::eq(first, second),
                (None, None) => true,
                _ => false,
            }
    }
    /// Replay logical query costs without reconstructing or cloning the result.
    /// The caller supplies the SAME step function, including its return reserve.
    pub(super) fn finish<E>(self, mut poll: impl FnMut() -> Result<(), E>, mut step: impl FnMut() -> Result<(), E>) -> Result<Value, E> {
        poll()?;
        for _ in 0..self.steps { step()?; }
        poll()?;
        Ok(self.result)
    }
}

fn within_limit<E>(value: &Value, limit: usize, poll: &mut impl FnMut() -> Result<(), E>) -> Result<bool, E> {
    struct Counter<'a, E, F> { bytes: usize, limit: usize, poll: &'a mut F, error: Option<E> }
    impl<E, F: FnMut() -> Result<(), E>> std::io::Write for Counter<'_, E, F> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Err(error) = (self.poll)() {
                self.error = Some(error);
                return Err(std::io::Error::other("projection retention cancelled"));
            }
            self.bytes = self.bytes.checked_add(bytes.len()).filter(|bytes| *bytes <= self.limit)
                .ok_or_else(|| std::io::Error::other("projection retention limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    poll()?;
    let mut counter = Counter { bytes: 0, limit, poll, error: None };
    let fits = serde_json::to_writer(&mut counter, value).is_ok();
    if let Some(error) = counter.error { return Err(error); }
    (counter.poll)()?;
    Ok(fits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn projection_move_keeps_exact_json_and_allocation() {
        let (snapshot, query, graph, origins) = (1usize, 2usize, 3usize, 4usize);
        let value = json!({"relations":[{"path":"\u{feff}😀\r\n界.xml","site":{"start":3,"end":7},"position":{"line":0,"character":2}}],"analysis":{"state":"partial"}});
        let expected = serde_json::to_vec(&value).unwrap();
        let pointer = value["relations"].as_array().unwrap().as_ptr();
        let retained = RetainedProjection::new(&snapshot, &query, Some(&graph), &origins, value, 7, || Ok::<_, usize>(())).unwrap().unwrap();
        let mut steps = 0;
        let result = retained.finish(|| Ok::<_, usize>(()), || { steps += 1; Ok(()) }).unwrap();
        assert_eq!(steps, 7);
        assert_eq!(result["relations"].as_array().unwrap().as_ptr(), pointer);
        assert_eq!(serde_json::to_vec(&result).unwrap(), expected);
    }

    #[test]
    fn projection_binding_rejects_every_other_borrowed_scope() {
        let (snapshot, query, graph, origins) = (String::from("same"), String::from("same"), String::from("same"), String::from("same"));
        let alternate = String::from("same");
        let cache = RetainedProjection::new(&snapshot, &query, Some(&graph), &origins, Value::Null, 0, || Ok::<_, usize>(())).unwrap().unwrap();
        assert!(cache.matches(&snapshot, &query, Some(&graph), &origins));
        assert!(!cache.matches(&alternate, &query, Some(&graph), &origins));
        assert!(!cache.matches(&snapshot, &alternate, Some(&graph), &origins));
        assert!(!cache.matches(&snapshot, &query, Some(&alternate), &origins));
        assert!(!cache.matches(&snapshot, &query, Some(&graph), &alternate));
        assert!(!cache.matches(&snapshot, &query, None, &origins));
        let cache = RetainedProjection::<_, _, String, _>::new(&snapshot, &query, None, &origins, Value::Null, 0, || Ok::<_, usize>(())).unwrap().unwrap();
        assert!(cache.matches(&snapshot, &query, None, &origins));
        assert!(!cache.matches(&snapshot, &query, Some(&graph), &origins));
    }

    #[test]
    fn projection_retention_exact_limit_and_large_result_fallback() {
        for length in [0, 1, 15, 16, 1023] {
            let value = json!("a".repeat(length));
            let bytes = serde_json::to_vec(&value).unwrap().len();
            for limit in [0, bytes - 1, bytes, bytes + 1] {
                assert_eq!(within_limit(&value, limit, &mut || Ok::<_, usize>(())).unwrap(), limit >= bytes);
            }
        }
        let (snapshot, query, graph, origins) = (1usize, 2usize, 3usize, 4usize);
        for length in [MAX_RETAINED_BYTES - 2, MAX_RETAINED_BYTES - 1] {
            let cache = RetainedProjection::new(&snapshot, &query, Some(&graph), &origins, json!("x".repeat(length)), 0, || Ok::<_, usize>(())).unwrap();
            assert_eq!(cache.is_some(), length == MAX_RETAINED_BYTES - 2);
        }
    }

    #[test]
    fn projection_retention_propagates_each_typed_cancellation() {
        let value = json!({"relations":[{"text":"😀\r\n\"escaped\""},null,true,42]});
        let mut total = 0;
        assert!(within_limit(&value, usize::MAX, &mut || { total += 1; Ok::<_, usize>(()) }).unwrap());
        for stop in 1..=total {
            let mut seen = 0;
            assert_eq!(within_limit(&value, usize::MAX, &mut || { seen += 1; if seen == stop { Err(stop) } else { Ok(()) } }), Err(stop));
            assert_eq!(seen, stop);
        }
        let mut seen = 0;
        assert_eq!(within_limit(&value, 0, &mut || { seen += 1; if seen == 3 { Err(99usize) } else { Ok(()) } }), Err(99));
    }

    #[test]
    fn projection_replay_never_hides_later_budget_or_cancellation() {
        let (snapshot, query, graph, origins) = (1usize, 2usize, 3usize, 4usize);
        for stop in 1..=7 {
            let cache = RetainedProjection::new(&snapshot, &query, Some(&graph), &origins, json!([1,2]), 7, || Ok::<_, usize>(())).unwrap().unwrap();
            let mut used = 0;
            assert_eq!(cache.finish(|| Ok::<_, usize>(()), || { used += 1; if used == stop { Err(stop) } else { Ok(()) } }), Err(stop));
            assert_eq!(used, stop);
        }
        for logical_steps in [0, 7] {
            for stop in [1, 2] {
                let cache = RetainedProjection::new(&snapshot, &query, Some(&graph), &origins, Value::Null, logical_steps, || Ok::<_, usize>(())).unwrap().unwrap();
                let mut seen = 0;
                let mut used = 0;
                assert_eq!(cache.finish(|| { seen += 1; if seen == stop { Err(stop) } else { Ok(()) } }, || { used += 1; Ok(()) }), Err(stop));
                assert_eq!(used, if stop == 1 { 0 } else { logical_steps });
            }
        }
    }
}
