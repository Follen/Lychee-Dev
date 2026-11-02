use super::*;

#[test]
fn actual_session_aliases_hash_each_candidate_and_share_one_authenticated_body() {
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
    let mut reports = Vec::new();
    session
        .read_batches(&[7, 7], &mut |files| {
            assert_eq!(files[0].materialized.content.bytes.as_slice(), f.payload);
            assert!(Arc::ptr_eq(
                &files[0].materialized.content,
                &files[1].materialized.content
            ));
            reports.extend(
                files
                    .iter()
                    .map(|file| file.reading["decodedCache"].clone()),
            );
            Ok(Vec::new())
        })
        .unwrap();
    assert_eq!(reports[0]["contentHashCalls"], 2);
    assert_eq!(reports[0]["contentDecodeCalls"], 2);
    assert_eq!(reports[0]["blobReads"], 2);
    assert_eq!(reports[1]["hits"], 1);
    assert_eq!(reports[1]["contentHashCalls"], 1);
    assert_eq!(reports[1]["contentDecodeCalls"], 0);
    assert_eq!(reports[1]["blobReads"], 0);
    assert_eq!(reports[1]["reusedBytes"], f.payload.len());
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();

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
    session
        .read_batches(&[7], &mut |files| {
            let stats = &files[0].reading["decodedCache"];
            assert_eq!(files[0].materialized.content.bytes.as_slice(), f.payload);
            assert_eq!(stats["contentHashCalls"], 2);
            assert_eq!(stats["contentDecodeCalls"], 0);
            assert_eq!(stats["hits"], 2);
            assert_eq!(
                stats["blobReads"], 2,
                "new request re-verifies both vault bodies"
            );
            Ok(Vec::new())
        })
        .unwrap();
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}

#[test]
fn actual_session_missing_decoded_blob_is_a_miss_but_corruption_is_terminal() {
    for missing in [true, false] {
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
        let mut reference = None;
        session
            .read_batches(&[7], &mut |files| {
                reference = Some(files[0].materialized.blob.clone());
                Ok(Vec::new())
            })
            .unwrap();
        session.close().unwrap();
        admission.finish(Ok(())).unwrap();
        let path = evidence::blob_path(&f.home, &reference.unwrap().sha256).unwrap();
        if missing {
            std::fs::remove_file(&path).unwrap();
        } else {
            let mut corrupt = f.payload.clone();
            corrupt[0] ^= 1;
            std::fs::write(&path, corrupt).unwrap();
        }
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
        let result = session.read_batches(&[7], &mut |files| {
            callbacks += 1;
            assert_eq!(files[0].materialized.content.bytes.as_slice(), f.payload);
            assert_eq!(files[0].reading["decodedCache"]["contentDecodeCalls"], 1);
            Ok(Vec::new())
        });
        if missing {
            result.unwrap();
            assert_eq!(callbacks, 1);
            assert_eq!(std::fs::read(path).unwrap(), f.payload);
        } else {
            assert_eq!(result.unwrap_err().code, "vault.blob_integrity");
            assert_eq!(callbacks, 0);
        }
        session.close().unwrap();
        admission.finish(Ok(())).unwrap();
        std::fs::remove_dir_all(f.home).unwrap();
    }
}

#[test]
fn actual_session_alias_cannot_hide_corrupted_current_encoded_fragment() {
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
    let mut payload_key = None;
    session
        .read_batches(&[7], &mut |files| {
            payload_key = files[0].reading["payloadEncodingKey"]
                .as_str()
                .map(str::to_owned);
            Ok(Vec::new())
        })
        .unwrap();
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    let objects = f.backend.objects.lock().unwrap();
    let encoded = &objects[payload_key.as_ref().unwrap()];
    let path = evidence::blob_path(&f.home, &storage::sha256(encoded)).unwrap();
    let original = std::fs::read(&path).unwrap();
    assert_eq!(
        original, *encoded,
        "whole tiny encoded object is the admitted cached fragment"
    );
    let mut corrupt = original;
    *corrupt.last_mut().unwrap() ^= 1;
    std::fs::write(path, corrupt).unwrap();
    drop(objects);
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
    let error = session
        .read_batches(&[7], &mut |_| {
            panic!("corrupt source must precede alias hit")
        })
        .unwrap_err();
    assert_eq!(error.code, "vault.blob_integrity");
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
