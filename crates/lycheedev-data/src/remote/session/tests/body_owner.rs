use super::*;
use crate::local_query::{File as PublicFile, FileBytes};
#[test]
fn returned_file_owner_outlives_session_without_copy_or_early_refund() {
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
    let mut returned = Vec::new();
    session
        .read_batches(&[7, 7], &mut |files| {
            for file in files {
                let pointer = file.bytes().as_ptr();
                let reading = serde_json::to_vec(&file.reading).unwrap();
                let (body, value) = file.into_parts();
                let file = PublicFile {
                    bytes: FileBytes::remote(body),
                    reading: value,
                };
                assert_eq!(file.bytes.as_ptr(), pointer);
                assert_eq!(serde_json::to_vec(&file.reading).unwrap(), reading);
                returned.push(file);
            }
            Ok(Vec::new())
        })
        .unwrap();
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    assert_eq!(returned[0].bytes.as_ptr(), returned[1].bytes.as_ptr());
    let used = f.budget.snapshot().unwrap();
    let logical = f.budget.logical_snapshot().unwrap();
    assert_eq!(returned[0].bytes.as_ref(), f.payload);
    drop(returned.pop());
    assert_eq!(
        f.budget.snapshot().unwrap(),
        used,
        "one remaining body owner still owns all bytes"
    );
    drop(returned);
    let after = f.budget.snapshot().unwrap();
    assert_eq!(
        used.retained_bytes - after.retained_bytes,
        f.payload.len() as u64 + 256
    );
    assert_eq!(after.network_requests, used.network_requests);
    assert_eq!(after.network_bytes, used.network_bytes);
    assert_eq!(after.decode_work, used.decode_work);
    assert_eq!(f.budget.logical_snapshot().unwrap(), logical);
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn failed_callback_drops_owned_bodies_and_keeps_observed_work() {
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
    let mut during = None;
    let error = session
        .read_batches(&[7], &mut |files| {
            let owned = files
                .into_iter()
                .map(|file| {
                    let (body, reading) = file.into_parts();
                    PublicFile {
                        bytes: FileBytes::remote(body),
                        reading,
                    }
                })
                .collect::<Vec<_>>();
            during = Some(f.budget.snapshot().unwrap());
            assert_eq!(owned[0].bytes.as_ref(), f.payload);
            Err(fault(
                "test.callback",
                5,
                "consumer rejected authenticated body",
            ))
        })
        .unwrap_err();
    assert_eq!(error.code, "test.callback");
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    let after = f.budget.snapshot().unwrap();
    let during = during.unwrap();
    assert!(after.retained_bytes < during.retained_bytes);
    assert_eq!(after.network_requests, during.network_requests);
    assert_eq!(after.network_bytes, during.network_bytes);
    assert_eq!(after.decode_work, during.decode_work);
    std::fs::remove_dir_all(f.home).unwrap();
}
