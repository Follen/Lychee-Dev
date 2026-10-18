use super::*;
use crate::remote::{
    catalog,
    transport::{RangeReply, RangeRequest},
};
use lycheedev_core::{control::OperationControl, storage, workspace::data_resources::Policy};
use md5::{Digest, Md5};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Default)]
struct Backend {
    metadata: BTreeMap<String, Vec<u8>>,
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
    calls: Mutex<Vec<String>>,
}
impl MetadataFetcher for Backend {
    fn fetch(
        &self,
        locator: &str,
        _: usize,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<Vec<u8>> {
        control.check()?;
        budget.map_or(Ok(()), Budget::check)?;
        self.calls.lock().unwrap().push(locator.into());
        self.metadata
            .get(locator)
            .cloned()
            .ok_or_else(|| fault("records.remote_http", 5, "fixture metadata absent"))
    }
}
impl RangeFetcher for Backend {
    fn fetch(
        &self,
        locator: &str,
        range: RangeRequest,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<RangeReply> {
        control.check()?;
        if let Some(budget) = budget {
            budget.charge(Cost {
                network_requests: 1,
                ..Cost::default()
            })?;
        }
        let key = locator.rsplit('/').next().unwrap();
        self.calls.lock().unwrap().push(key.into());
        let objects = self.objects.lock().unwrap();
        let raw = objects
            .get(key)
            .ok_or_else(|| fault("records.remote_object_missing", 3, "fixture object absent"))?;
        let bytes = raw
            .get(range.offset as usize..(range.offset + range.length) as usize)
            .ok_or_else(invalid)?
            .to_vec();
        if let Some(budget) = budget {
            budget.charge_observed_network_bytes(bytes.len() as u64)?;
        }
        Ok(RangeReply {
            bytes,
            object_bytes: raw.len() as i64,
        })
    }
}
fn hash(raw: &[u8]) -> String {
    casc::hex(&Md5::digest(raw))
}
fn framed(raw: &[u8]) -> (String, Vec<u8>) {
    let chunk = [b"N".as_slice(), raw].concat();
    let mut encoded = b"BLTE".to_vec();
    encoded.extend_from_slice(&36u32.to_be_bytes());
    encoded.extend_from_slice(&[15, 0, 0, 1]);
    encoded.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&(raw.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&Md5::digest(&chunk));
    let key = hash(&encoded);
    encoded.extend(chunk);
    (key, encoded)
}
fn encoding(rows: &[(String, String, usize, usize)]) -> Vec<u8> {
    let mut c = rows.to_vec();
    c.sort_by(|a, b| a.0.cmp(&b.0));
    let mut e = rows.to_vec();
    e.sort_by(|a, b| a.1.cmp(&b.1));
    let mut cp = vec![0; 1024];
    let mut ep = vec![0; 1024];
    for (n, (ckey, ekey, decoded, _)) in c.iter().enumerate() {
        let at = n * 38;
        cp[at] = 1;
        cp[at + 2..at + 6].copy_from_slice(&(*decoded as u32).to_be_bytes());
        cp[at + 6..at + 22].copy_from_slice(&casc::key(ckey).unwrap());
        cp[at + 22..at + 38].copy_from_slice(&casc::key(ekey).unwrap());
    }
    for (n, (_, ekey, _, encoded)) in e.iter().enumerate() {
        let at = n * 25;
        ep[at..at + 16].copy_from_slice(&casc::key(ekey).unwrap());
        ep[at + 21..at + 25].copy_from_slice(&(*encoded as u32).to_be_bytes());
    }
    let mut raw = vec![0; 22];
    raw[..5].copy_from_slice(&[b'E', b'N', 1, 16, 16]);
    raw[5..7].copy_from_slice(&1u16.to_be_bytes());
    raw[7..9].copy_from_slice(&1u16.to_be_bytes());
    raw[9..13].copy_from_slice(&1u32.to_be_bytes());
    raw[13..17].copy_from_slice(&1u32.to_be_bytes());
    raw.extend_from_slice(&casc::key(&c[0].0).unwrap());
    raw.extend_from_slice(&Md5::digest(&cp));
    raw.extend(cp);
    raw.extend_from_slice(&casc::key(&e[0].1).unwrap());
    raw.extend_from_slice(&Md5::digest(&ep));
    raw.extend(ep);
    raw
}
struct Fixture {
    home: PathBuf,
    control: OperationControl,
    budget: Arc<Budget>,
    pin: DataPin,
    backend: Backend,
    payload: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_corrupt_ckey(false)
    }
    fn with_corrupt_ckey(corrupt: bool) -> Self {
        Self::with_root(corrupt, None)
    }
    fn with_root(corrupt: bool, groups: Option<(usize, bool)>) -> Self {
        Self::with_payload(
            corrupt,
            groups,
            b"fully authenticated selected payload".to_vec(),
        )
    }
    fn with_payload(corrupt: bool, groups: Option<(usize, bool)>, payload: Vec<u8>) -> Self {
        Self::with_framing(corrupt, groups, payload, framed)
    }
    fn with_framing(
        corrupt: bool,
        groups: Option<(usize, bool)>,
        payload: Vec<u8>,
        frame: fn(&[u8]) -> (String, Vec<u8>),
    ) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/remote-session-fixtures")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                storage::utc_now().replace([':', '.'], "-"),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        let control = OperationControl::default();
        let budget = Arc::new(Budget::controlled(
            Cost::defaults(),
            control.deadline(),
            control.clone(),
        ));
        let pckey = hash(&payload);
        let mut actual = payload.clone();
        if corrupt {
            actual[0] ^= 1;
        }
        let (pekey, pencoded) = frame(&actual);
        let mut root = Vec::new();
        let (foreign_groups, two_ids) = groups.unwrap_or((0, false));
        for group in 0..=foreign_groups {
            let count = if two_ids { 2u32 } else { 1 };
            root.extend_from_slice(&count.to_le_bytes());
            root.extend_from_slice(&0u32.to_le_bytes());
            root.extend_from_slice(
                &(if group == foreign_groups { 0x40u32 } else { 2 }).to_le_bytes(),
            );
            root.extend_from_slice(&7u32.to_le_bytes());
            if two_ids {
                root.extend_from_slice(&0u32.to_le_bytes());
            }
            for _ in 0..count {
                root.extend_from_slice(&casc::key(&pckey).unwrap());
                root.extend_from_slice(&0u64.to_le_bytes());
            }
        }
        let rckey = hash(&root);
        let (rekey, rencoded) = framed(&root);
        let enc = encoding(&[
            (pckey, pekey.clone(), payload.len(), pencoded.len()),
            (rckey.clone(), rekey.clone(), root.len(), rencoded.len()),
        ]);
        let (ekey, encoded) = framed(&enc);
        let build = format!(
            "root = {rckey}\nencoding = {} {ekey}\nencoding-size = {} {}\nbuild-uid = wow\n",
            hash(&enc),
            enc.len(),
            encoded.len()
        )
        .into_bytes();
        let cdn = b"archives =\n".to_vec();
        let pin = DataPin {
            product: "retail".into(),
            region: "cn".into(),
            language: "zhCN".into(),
            full_build: "12.1.5.70000".into(),
            build_config: hash(&build),
            cdn_config: hash(&cdn),
            definition_commit: "a".repeat(40),
        };
        let mut backend = Backend::default();
        let route = catalog::Distribution {
            path: "tpr/test".into(),
            hosts: vec!["cdn.example.test".into()],
        };
        backend.metadata.insert(
            crate::remote::cache::catalog_url("cn", "wow", "cdns").unwrap(),
            b"Name!STRING:0|Path!STRING:0|Hosts!STRING:0\ncn|tpr/test|cdn.example.test\n".to_vec(),
        );
        for (key, bytes) in [(&pin.build_config, build), (&pin.cdn_config, cdn)] {
            backend
                .metadata
                .insert(route.config_url(&route.hosts[0], key).unwrap(), bytes);
        }
        backend.objects.get_mut().unwrap().extend([
            (pekey, pencoded),
            (rekey, rencoded),
            (ekey, encoded),
        ]);
        Self {
            home,
            control,
            budget,
            pin,
            backend,
            payload,
        }
    }
    fn options(&self) -> Options {
        Options {
            offline: false,
            key_file: None,
            variant: String::new(),
            metadata_bytes: 1 << 20,
            content_bytes: 1 << 20,
        }
    }
    fn admission(&self) -> AdmittedWorkspace<'_> {
        AdmittedWorkspace::acquire(
            &self.home,
            Policy::Ordinary,
            &self.control,
            Some(&self.budget),
        )
        .unwrap()
    }
}
#[test]
fn borrowed_session_is_lazy_and_never_releases_the_external_admission() {
    let f = Fixture::new();
    let mut admission = f.admission();
    let session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        f.options(),
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    session.check().unwrap();
    assert!(f.backend.calls.lock().unwrap().is_empty());
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    session.close().unwrap();
    let moved = f.home.with_extension("moved");
    assert!(std::fs::rename(&f.home, &moved).is_err());
    let deadline = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
    assert!(AdmittedWorkspace::acquire(&f.home, Policy::Measurement, &deadline, None).is_err());
    admission.finish(Ok(())).unwrap();
    std::fs::rename(&f.home, &moved).unwrap();
    std::fs::remove_dir_all(moved).unwrap();
}
#[test]
fn actual_admitted_cdn_encoding_root_selected_chain_keeps_one_scoped_batch() {
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
    let mut batches = 0;
    session
        .read_batches(&[7, 7], &mut |files| {
            batches += 1;
            for file in files {
                assert_eq!(file.bytes(), f.payload);
                assert_eq!(file.entry.file_data_id, 7);
                assert_eq!(file.reading["source"], "cdn");
                assert_eq!(
                    file.reading["verification"]["contentWholeDecodedCKey"],
                    true
                );
                assert_eq!(file.reference().sha256, storage::sha256(&f.payload));
            }
            Ok(if batches == 1 { vec![7] } else { Vec::new() })
        })
        .unwrap();
    assert_eq!(batches, 2);
    assert!(f.budget.snapshot().unwrap().network_requests > 0);
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn callback_failure_is_not_replayed_and_original_scope_remains_terminal() {
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
    let mut calls = 0;
    let error = session
        .read_batches(&[7], &mut |_| {
            calls += 1;
            Err(fault("test.callback", 5, "failed callback"))
        })
        .unwrap_err();
    assert_eq!(error.code, "test.callback");
    assert_eq!(calls, 1);
    assert_eq!(f.control.stop_cause().unwrap(), None);
    f.control
        .ordered_child(f.control.deadline())
        .unwrap()
        .check()
        .unwrap();
    f.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        session
            .read_batches(&[7], &mut |_| panic!("cancelled callback"))
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert_eq!(session.close().unwrap_err().code, "command.cancelled");
    assert_eq!(
        admission.finish(Ok(())).unwrap_err().code,
        "command.cancelled"
    );
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn different_budget_and_bad_pin_cannot_borrow_an_admitted_capability() {
    let f = Fixture::new();
    let mut admission = f.admission();
    let other = Arc::new(Budget::controlled(
        Cost::defaults(),
        f.control.deadline(),
        f.control.clone(),
    ));
    assert_eq!(
        Session::borrowed_with(
            &mut admission,
            &f.pin,
            f.options(),
            &other,
            &f.backend,
            &f.backend
        )
        .err()
        .unwrap()
        .code,
        "records.file_query"
    );
    let mut bad = f.pin.clone();
    bad.build_config.clear();
    assert_eq!(
        Session::borrowed_with(
            &mut admission,
            &bad,
            f.options(),
            &f.budget,
            &f.backend,
            &f.backend
        )
        .err()
        .unwrap()
        .code,
        "selection.unresolved_data"
    );
    assert!(f.backend.calls.lock().unwrap().is_empty());
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn independent_parser_check_is_observed_without_reading_next_page_or_root_group() {
    use std::cell::Cell;
    let f = Fixture::new();
    let stopped = Cell::new(false);
    let raw = encoding(&[("1".repeat(32), "2".repeat(32), 3, 12)]);
    let content = blte::Content {
        bytes: raw,
        missing: vec![],
    };
    let limits = casc::Limits::default();
    let error = Encoding::open_with_check(&content, limits, None, || {
        stopped.set(true);
        Err(fault("test.independent", 7, "stopped"))
    })
    .err()
    .unwrap();
    assert_eq!(error.code, "test.independent");
    assert!(stopped.get());
    let index = Encoding::open(&content, limits, None).unwrap();
    assert_eq!(
        index
            .find_with_check(&"1".repeat(32), limits, None, || Err(fault(
                "test.independent",
                7,
                "stopped"
            )))
            .unwrap_err()
            .code,
        "test.independent"
    );
    let invalid_root = blte::Content {
        bytes: vec![],
        missing: vec![],
    };
    assert_eq!(
        metadata_format::lookup_root_with_check(
            &invalid_root,
            &[7],
            metadata_format::RootLimits::default(),
            None,
            || Err(fault("test.independent", 7, "stopped"))
        )
        .unwrap_err()
        .code,
        "test.independent"
    );
    std::fs::remove_dir_all(f.home).unwrap();
}

#[test]
fn valid_ekey_and_chunk_cannot_substitute_for_complete_selected_ckey() {
    let f = Fixture::with_corrupt_ckey(true);
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
            panic!("unverified bytes cannot be delivered")
        })
        .unwrap_err();
    assert_eq!(error.code, "records.content_integrity");
    let mut corrupt = f.payload.clone();
    corrupt[0] ^= 1;
    assert!(
        !evidence::blob_path(&f.home, &storage::sha256(&corrupt))
            .unwrap()
            .exists()
    );
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}

fn missing_page_key() -> ToolkitError {
    fault(
        "container.key_unavailable",
        3,
        "injected encrypted page needs loaded snapshot",
    )
    .with_details(serde_json::json!({"missingKey": true}))
}
#[test]
fn key_snapshot_rebuild_resumes_only_undelivered_ids_and_retains_work() {
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control);
    let mut callbacks = Vec::new();
    let mut runs = 0;
    recover_batches(&[7], true, |progress, rebuild| {
        runs += 1;
        budget.charge(Cost::work(1))?;
        progress.validate()?;
        if !rebuild {
            callbacks.push(progress.pending.clone());
            progress.deliver(|| Ok(vec![8, 9]))?;
            assert_eq!(progress.delivered, 1);
            return Err(missing_page_key());
        }
        assert_eq!(progress.pending, [8, 9]);
        assert_eq!(progress.delivered, 1);
        callbacks.push(progress.pending.clone());
        progress.deliver(|| Ok(Vec::new()))?;
        assert_eq!(progress.delivered, 3);
        Ok(())
    })
    .unwrap();
    assert_eq!(runs, 2);
    assert_eq!(callbacks, [vec![7], vec![8, 9]]);
    assert_eq!(budget.snapshot().unwrap().decode_work, 2);
}
#[test]
fn key_loaded_during_content_still_rebuilds_old_snapshot_only_once() {
    let mut loaded = false;
    let mut loads = 0;
    let mut runs = 0;
    let error = recover_batches(&[7, 7], true, |progress, rebuild| {
        runs += 1;
        if !rebuild {
            // Models the real content loader completing before a later
            // Encoding page consults the original empty snapshot.
            loaded = true;
        } else {
            if !loaded {
                loads += 1;
            }
            assert_eq!(progress.pending, [7, 7]);
            assert_eq!(progress.delivered, 0);
        }
        Err(missing_page_key())
    })
    .unwrap_err();
    assert_eq!(error.code, "container.key_unavailable");
    assert_eq!(runs, 2);
    assert_eq!(loads, 0);
}
#[test]
fn callback_key_error_is_never_replayed_and_id_limit_survives_rebuild() {
    let mut callbacks = 0;
    let mut runs = 0;
    let error = recover_batches(&[7], true, |progress, _| {
        runs += 1;
        progress.deliver(|| {
            callbacks += 1;
            Err(missing_page_key())
        })
    })
    .unwrap_err();
    assert_eq!(error.code, "container.key_unavailable");
    assert_eq!((runs, callbacks), (1, 1));
    let error = recover_batches(&[7; 31], true, |progress, rebuild| {
        if !rebuild {
            progress.deliver(|| Ok(vec![8, 9]))?;
            return Err(missing_page_key());
        }
        assert_eq!(progress.delivered, 31);
        progress.validate()
    })
    .unwrap_err();
    assert_eq!(error.code, "records.file_query");
}
#[test]
fn multi_locale_root_match_limit_is_per_id_not_per_batch() {
    let f = Fixture::with_root(false, Some((2048, true)));
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
    let mut calls = 0;
    session
        .read_batches(&[7, 8], &mut |files| {
            calls += 1;
            assert_eq!(files.len(), 2);
            assert_eq!(
                (files[0].entry.file_data_id, files[1].entry.file_data_id),
                (7, 8)
            );
            for file in files {
                assert_eq!(file.entry.locale_mask, 0x40);
                assert_eq!(file.bytes(), f.payload);
            }
            Ok(Vec::new())
        })
        .unwrap();
    assert_eq!(calls, 1);
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn single_id_root_match_limit_stays_4096_before_locale_filtering() {
    let f = Fixture::with_root(false, Some((4096, false)));
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
        .read_batches(&[7], &mut |_| panic!("over-limit Root must not deliver"))
        .unwrap_err();
    assert_eq!(error.code, "records.metadata_limit");
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}

mod decoded_alias;

mod body_owner;

mod default_sql;

mod default_navigation;

#[test]
fn real_remote_metadata_reads_preserve_ordered_original_scope_through_batches() {
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
    let mut calls = 0;
    session
        .read_batches(&[7], &mut |files| {
            calls += 1;
            assert_eq!(files[0].bytes(), f.payload);
            assert_eq!(f.control.stop_cause().unwrap(), None);
            let child = f.control.ordered_child(f.control.deadline()).unwrap();
            child.check().unwrap();
            assert_eq!(child.stop_cause().unwrap(), None);
            Ok(if calls == 1 { vec![7] } else { vec![] })
        })
        .unwrap();
    assert_eq!(calls, 2);
    assert!(f.budget.snapshot().unwrap().network_requests > 0);
    assert_eq!(f.control.stop_cause().unwrap(), None);
    f.control.tracked_cancel_handle().unwrap();
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}

#[test]
fn real_remote_metadata_reads_keep_independent_budget_cancellation_ordered() {
    let mut f = Fixture::new();
    let budget_control = OperationControl::uncancelable(f.control.deadline());
    f.budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        budget_control.deadline(),
        budget_control.clone(),
    ));
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
    let mut calls = 0;
    session
        .read_batches(&[7], &mut |files| {
            calls += 1;
            assert_eq!(files[0].bytes(), f.payload);
            assert_eq!(budget_control.stop_cause().unwrap(), None);
            budget_control
                .ordered_child(budget_control.deadline())
                .unwrap()
                .check()
                .unwrap();
            Ok(vec![])
        })
        .unwrap();
    assert_eq!(calls, 1);
    let requests = f.budget.snapshot().unwrap().network_requests;
    budget_control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        budget_control.stop_cause().unwrap(),
        Some(lycheedev_core::control::StopCause::Cancelled)
    );
    assert_eq!(f.control.stop_cause().unwrap(), None);
    f.control.check().unwrap();
    let error = session
        .read_batches(&[7], &mut |_| {
            panic!("stopped independent budget delivered callback")
        })
        .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.exit_code, 7);
    assert_eq!(f.budget.snapshot().unwrap().network_requests, requests);
    assert_eq!(session.close().unwrap_err().code, "command.cancelled");
    assert_eq!(
        admission.finish(Ok(())).unwrap_err().code,
        "command.cancelled"
    );
    f.control.check().unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}

mod verified;

mod default_asset_export;

mod prepared;

mod partial_tables;
