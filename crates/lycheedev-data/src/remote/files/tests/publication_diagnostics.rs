//! Diagnostic successor: preserve both joined worker causes before ordered
//! selection can legitimately ignore a later result. No production changes.
use super::*;

#[test]
fn joined_archive_probe_publication_preserves_both_original_worker_outcomes() {
    for bad_first in [true, false] {
        let (key, raw) = bare(b"body");
        let size = raw.len();
        let (a, mut ia) = index(&key, size, 0);
        let (b, mut ib) = index(&key, size, 1);
        // The two 1-byte probes really publish the same immutable blob.
        assert_eq!(ia[0], ib[0]);
        let probe_hash = storage::sha256(&ia[..1]);
        if bad_first {
            *ia.last_mut().unwrap() ^= 1;
        } else {
            *ib.last_mut().unwrap() ^= 1;
        }
        let mut fixture = Fixture::new(&format!("archives = {a} {b}\n"));
        fixture.backend.gate = Some((Mutex::new(0), Condvar::new()));
        fixture.object(&format!("{a}.index"), ia);
        fixture.object(&format!("{b}.index"), ib);
        let budget = Budget::controlled(
            Cost::defaults(),
            fixture.control.deadline(),
            fixture.control.clone(),
        );
        let files = fixture.files(false, Some(&budget), 2);
        let loose = files.range(&key, 0, 8, size as i64);
        assert_eq!(loose.err().unwrap().code, "records.remote_object_missing");
        let archives = vec![a.clone(), b.clone()];
        let joined = files.lookup_batch(&key, size as i64, &archives, &archives);
        let outcomes = match &joined {
            Ok(results) => results.iter().map(|value| match value {
                Ok(span) => serde_json::json!({"object":span.object,"offset":span.offset,"bytes":span.bytes}),
                Err(error) => serde_json::json!({"error":error}),
            }).collect::<Vec<_>>(),
            Err(error) => vec![serde_json::json!({"joinError":error})],
        };
        let counters = files.counters().unwrap();
        let calls = fixture.backend.calls.lock().unwrap().clone();
        eprintln!(
            "{}",
            serde_json::json!({"schema":"lycheedev.archive-join-diagnostic.v1",
            "badFirst":bad_first,"probeSHA256":probe_hash,"outcomes":outcomes,
            "requests":counters.requests,"fragmentLoads":counters.fragment_loads,
            "observedRequests":budget.snapshot().unwrap().network_requests,"calls":calls})
        );
        // All workers have joined before either assertion or fixture teardown.
        assert_eq!(fixture.backend.active.load(Ordering::SeqCst), 0);
        let results = joined.unwrap();
        assert_eq!(results.len(), 2);
        for (ordinal, result) in results.iter().enumerate() {
            if (ordinal == 0) == bad_first {
                assert_eq!(
                    result.as_ref().err().unwrap().code,
                    "records.configuration_integrity",
                    "unexpected worker cause: {outcomes:?}"
                );
            } else {
                assert!(
                    result.is_ok(),
                    "valid worker did not complete: {outcomes:?}"
                );
            }
        }
        assert_eq!(
            counters.fragment_loads, 5,
            "both original outcomes: {outcomes:?}"
        );
        assert_eq!(counters.requests, calls.len() as u64);
        assert_eq!(
            counters.requests,
            budget.snapshot().unwrap().network_requests
        );
        drop(files);
        std::fs::remove_dir_all(fixture.home).unwrap();
    }
}
