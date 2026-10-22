use emmylua_code_analysis::EmmyLuaAnalysis;
use lycheedev_emmy_adapter::{IngestionFailure, ingest_public_batch};
use std::sync::Arc;

#[test]
fn byte_admission_is_atomic_and_empty_delete_batches_keep_exact_costs() {
    let path = std::env::current_dir()
        .unwrap()
        .join("owned-ingestion-limit.lua");
    let text: Arc<str> = Arc::from("return 1\n");
    let mut analysis = EmmyLuaAnalysis::new();
    let failed = ingest_public_batch(
        &mut analysis,
        vec![(path.clone(), Some(text.clone()))],
        text.len() - 1,
        || Ok::<_, ()>(()),
    );
    assert!(matches!(failed, Err(IngestionFailure::InvalidInput(_))));
    assert!(analysis.db.file_ids().is_empty());
    let batch = ingest_public_batch(
        &mut analysis,
        vec![(path.clone(), Some(text.clone()))],
        text.len(),
        || Ok::<_, ()>(()),
    )
    .unwrap();
    assert_eq!(batch.text_cost.retained_bytes, text.len());
    let removed =
        ingest_public_batch(&mut analysis, vec![(path, None)], 0, || Ok::<_, ()>(())).unwrap();
    assert_eq!(removed.file_ids, batch.file_ids);
    assert_eq!(removed.text_cost.retained_bytes, 0);
    assert_eq!(removed.text_cost.largest_file_bytes, 0);
    assert!(analysis.db.get_file_text(batch.file_ids[0]).is_none());
}

#[test]
fn utf8_chunks_keep_every_byte_and_check_original_control_at_each_owned_boundary() {
    let path = std::env::current_dir()
        .unwrap()
        .join("owned-ingestion-unicode.lua");
    let text: Arc<str> = Arc::from(format!("--{}😀漢字\nreturn 1\n", "x".repeat(16 * 1024 - 3)));
    let mut checks = 0usize;
    let mut analysis = EmmyLuaAnalysis::new();
    let batch = ingest_public_batch(
        &mut analysis,
        vec![(path.clone(), Some(text.clone()))],
        text.len(),
        || {
            checks += 1;
            Ok::<_, usize>(())
        },
    )
    .unwrap();
    assert_eq!(
        analysis.db.get_file_text(batch.file_ids[0]),
        Some(text.as_ref())
    );
    assert_eq!(batch.text_cost.retained_bytes, text.len());
    assert_eq!(batch.text_cost.largest_file_bytes, text.len());
    assert_ne!(
        analysis
            .db
            .get_file_text(batch.file_ids[0])
            .unwrap()
            .as_ptr(),
        text.as_ptr()
    );
    for stop in 1..=checks {
        let mut check = 0usize;
        let result = ingest_public_batch(
            &mut EmmyLuaAnalysis::new(),
            vec![(path.clone(), Some(text.clone()))],
            text.len(),
            || {
                check += 1;
                if check == stop { Err(stop) } else { Ok(()) }
            },
        );
        assert!(matches!(result, Err(IngestionFailure::Interrupted(index)) if index == stop));
        assert_eq!(check, stop);
    }
}
