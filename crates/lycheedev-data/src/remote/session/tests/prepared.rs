//! Real synthetic CDN Session consumers. No network service or public-key
//! download is substituted; failures remain available for the consolidated run.
use super::*;

#[test]
fn same_session_reuses_prepared_root_index_pages_and_reports_root_only_once() {
    let f = Fixture::new();
    let mut admission = f.admission();
    let mut session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        f.options(),
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    let mut readings = Vec::new();
    session
        .read_batches(&[7], &mut |files| {
            assert_eq!(files[0].bytes(), f.payload);
            readings.push(files[0].reading.clone());
            Ok(vec![])
        })
        .unwrap();
    let prepared = session.prepared.as_ref().unwrap();
    assert!(prepared.root_reported);
    let root_pointer = Arc::as_ptr(&prepared.root.content);
    let metadata = f.budget.logical_snapshot().unwrap().metadata_bytes;
    let requests = f.budget.snapshot().unwrap().network_requests;
    // Prepared Root is an immutable authenticated owner. Repeated selection
    // must not re-open the old on-disk Root merely to reconstruct its index.
    let root_path = evidence::blob_path(&f.home, &prepared.root.blob.sha256).unwrap();
    std::fs::remove_file(root_path).unwrap();
    session
        .read_batches(&[7], &mut |files| {
            assert_eq!(files[0].bytes(), f.payload);
            readings.push(files[0].reading.clone());
            Ok(vec![])
        })
        .unwrap();
    assert_eq!(
        Arc::as_ptr(&session.prepared.as_ref().unwrap().root.content),
        root_pointer
    );
    assert_eq!(readings[0]["decodedCache"]["contentDecodeCalls"], 2);
    assert_eq!(readings[1]["decodedCache"]["contentDecodeCalls"], 0);
    assert_eq!(readings[1]["decodedCache"]["hits"], 1);
    assert_eq!(readings[1]["decodedCache"]["blobReads"], 0);
    assert_eq!(
        f.budget.logical_snapshot().unwrap().metadata_bytes,
        metadata
    );
    assert_eq!(f.budget.snapshot().unwrap().network_requests, requests);
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn successful_file_reading_consumes_root_stats_before_failing_callback() {
    let f = Fixture::new();
    let mut admission = f.admission();
    let mut session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        f.options(),
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    let mut callbacks = 0;
    let error = session
        .read_batches(&[7], &mut |files| {
            callbacks += 1;
            assert_eq!(files[0].reading["decodedCache"]["contentDecodeCalls"], 2);
            Err(fault(
                "test.consumer",
                5,
                "consumer stopped after a successful reading",
            ))
        })
        .unwrap_err();
    assert_eq!(error.code, "test.consumer");
    assert_eq!(callbacks, 1);
    assert!(session.prepared.as_ref().unwrap().root_reported);
    session
        .read_batches(&[7], &mut |files| {
            callbacks += 1;
            assert_eq!(files[0].reading["decodedCache"]["contentDecodeCalls"], 0);
            assert_eq!(files[0].reading["decodedCache"]["hits"], 1);
            Ok(vec![])
        })
        .unwrap();
    assert_eq!(callbacks, 2);
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn failed_first_selection_keeps_unreported_preparation_for_next_valid_id() {
    let f = Fixture::new();
    let mut admission = f.admission();
    let mut session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        f.options(),
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    assert_eq!(
        session
            .read_batches(&[8], &mut |_| panic!("missing ID callback"))
            .unwrap_err()
            .code,
        "records.content_missing"
    );
    assert!(!session.prepared.as_ref().unwrap().root_reported);
    let root = Arc::as_ptr(&session.prepared.as_ref().unwrap().root.content);
    session
        .read_batches(&[7], &mut |files| {
            assert_eq!(files[0].reading["decodedCache"]["contentDecodeCalls"], 2);
            Ok(vec![])
        })
        .unwrap();
    assert_eq!(
        Arc::as_ptr(&session.prepared.as_ref().unwrap().root.content),
        root
    );
    assert!(session.prepared.as_ref().unwrap().root_reported);
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn same_session_verified_second_body_excludes_root_readback_and_prepare_cost() {
    let f = Fixture::new();
    let mut admission = f.admission();
    let mut session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        f.options(),
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    let mut statistics = Vec::new();
    for _ in 0..2 {
        session
            .with_verified(7, &mut |file| {
                statistics.push(file.reading["decodedCache"].clone());
                let mut bytes = vec![0; f.payload.len()];
                assert_eq!(file.blob.read_checked(&mut bytes)?, bytes.len());
                assert_eq!(bytes, f.payload);
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(statistics[0]["contentDecodeCalls"], 2);
    assert_eq!(statistics[0]["blobReads"], 1);
    assert_eq!(statistics[1]["contentDecodeCalls"], 0);
    assert_eq!(statistics[1]["blobReads"], 1);
    assert_eq!(statistics[1]["hits"], 1);
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn files_counter_deltas_sum_real_preparation_and_selected_reads_once() {
    let f = Fixture::new();
    let mut admission = f.admission();
    let mut session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        f.options(),
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    let mut readings = Vec::new();
    session
        .read_batches(&[7], &mut |files| {
            readings.push(files[0].reading["decodedCache"].clone());
            Ok(if readings.len() == 1 { vec![7] } else { vec![] })
        })
        .unwrap();
    let counters = session.files.as_ref().unwrap().counters().unwrap();
    let summed = |key: &str| {
        readings
            .iter()
            .map(|value| value[key].as_u64().unwrap())
            .sum::<u64>()
    };
    assert_eq!(summed("fragmentLoads"), counters.fragment_loads);
    assert_eq!(summed("blockCacheHits"), counters.block_cache_hits);
    assert!(counters.fragment_loads > 0);
    assert!(counters.block_cache_hits > 0);
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
