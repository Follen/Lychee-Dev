//! Spell profiles use real workspace leases and authenticated local table input.
//! These synthetic fixtures are not evidence for an installed game client.
use super::*;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell::U, Fixture, table};
use lycheedev_core::{
    control::{OperationControl, TrackedCancellation},
    metadata::Metadata,
    storage,
    workspace::data_resources::{self, Policy},
};
use std::{fs, path::Path, sync::mpsc};

const COMMANDS: [&str; 2] = ["data spell auras", "data spell summons"];

fn fixture() -> Fixture {
    Fixture::new(&[table(
        "SpellEffect",
        303,
        &[
            ("ID", "uint"),
            ("SpellID", "uint"),
            ("EffectIndex", "uint"),
            ("Effect", "uint"),
            ("EffectMiscValue", "uint"),
        ],
        &[
            vec![U(1), U(7), U(0), U(6), U(0)],
            vec![U(2), U(7), U(1), U(28), U(5000)],
        ],
    )])
}

fn captures(fx: &Fixture) -> usize {
    let metadata = Metadata::open(&fx.home, false).unwrap();
    let count = metadata.list::<Value>("capture/", 100).unwrap().len();
    metadata.close().unwrap();
    count
}

fn released(fx: &Fixture) {
    data_resources::acquire(&fx.home, Policy::Measurement, &OperationControl::default())
        .unwrap()
        .close()
        .unwrap();
}

fn held(home: &Path) {
    let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
    assert_eq!(
        data_resources::acquire(home, Policy::Measurement, &short)
            .err()
            .expect("navigation still owns a real slot after reading")
            .code,
        "command.deadline"
    );
}

fn gate_held(home: &Path) {
    assert_eq!(
        storage::ExclusiveLock::resource(&home.join("locks/data-resources-v1"), "admission")
            .err()
            .expect("failed waiter must not release the other owner's lease")
            .code,
        "workspace.busy"
    );
}

fn verify(fx: &Fixture, command: &str, out: &Outcome) {
    assert!(out.complete);
    assert_eq!(out.data["complete"], true);
    assert_eq!(out.data["partial"], false);
    assert_eq!(out.data["truncated"], false);
    assert_eq!(out.data["tables"][0]["contentVerified"], true);
    assert_eq!(
        out.data["tableVerification"][0]["contentWholeDecodedCKey"],
        true
    );
    if command == "data spell auras" {
        assert_eq!(out.data["hasAura"], json!([7]));
        assert_eq!(out.data["noAura"], json!([]));
        assert_eq!(out.data["relations"].as_array().unwrap().len(), 2);
    } else {
        assert_eq!(out.data["count"], 1);
        assert_eq!(out.data["summons"][0]["npcID"], 5000);
        assert_eq!(out.data["relations"].as_array().unwrap().len(), 1);
    }
    let memory = &out.data["memoryBudget"];
    assert_eq!(
        memory["mode"],
        if cfg!(windows) {
            "windows_adaptive"
        } else {
            "fixed"
        }
    );
    for key in [
        "retainedBytes",
        "metadataBytes",
        "peakScratchBytes",
        "growths",
    ] {
        assert!(memory[key].is_u64(), "missing memoryBudget.{key}");
    }
    if cfg!(windows) {
        assert_eq!(memory["concurrentQueries"], 2);
        assert!(memory["ceilingBytes"].as_u64().unwrap() > 0);
    }
    assert!(out.data["resources"]["retainedBytes"].as_u64().unwrap() > 0);
    assert!(out.data["resources"]["metadataBytes"].as_u64().unwrap() > 0);
    assert_eq!(out.data["resources"]["networkRequests"], 0);
    assert!(out.data["tables"][0].get("memoryBudget").is_none());
    assert_eq!(out.provenance.len(), 1);
    let raw = evidence::read_blob(&fx.home, &out.provenance[0]["blob"], 16 << 20).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), out.data);
    assert_eq!(storage::sha256(&raw), out.provenance[0]["blob"]["sha256"]);
    let verb = command.strip_prefix("data spell ").unwrap();
    assert_eq!(
        out.provenance[0]["provenance"]["kind"],
        format!("spell-{verb}")
    );
    assert_eq!(
        out.provenance[0]["provenance"]["locator"],
        format!("spell:{verb}:7")
    );
    assert_eq!(out.provenance[0]["provenance"]["snapshot"], fx.pin);
}

#[test]
fn local_spell_profiles_hold_admission_through_read_and_capture_memory_context() {
    let fx = fixture();
    for command in COMMANDS {
        let before = captures(&fx);
        let inv = fx.invocation(command, &[("--spell-id", "7")]);
        let mut visited = false;
        let out = dispatch_after_read(&inv, &mut || {
            visited = true;
            held(&fx.home);
            Ok(())
        })
        .unwrap();
        assert!(visited);
        verify(&fx, command, &out);
        assert_eq!(captures(&fx), before + 1);
        released(&fx);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn spell_profiles_parse_before_admission_but_resolve_pin_after_original_deadline_wait() {
    let fx = fixture();
    let owner =
        data_resources::acquire(&fx.home, Policy::Measurement, &OperationControl::default())
            .unwrap();
    for command in COMMANDS {
        for cdn in [false, true] {
            let mut inv = fx.invocation(command, &[("--spell-id", "invalid")]);
            if cdn {
                inv.flags.insert("--cdn".into(), "true".into());
                inv.flags.remove("--installation");
            }
            assert_eq!(
                dispatch(&inv).unwrap_err().code,
                "command.invalid_arguments"
            );
            inv.flags.insert("--spell-id".into(), "7".into());
            inv.flags
                .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
            inv.control =
                OperationControl::uncancelable(Instant::now() + Duration::from_millis(75));
            let error = dispatch_after_read(&inv, &mut || panic!("must not read before admission"))
                .unwrap_err();
            let original = inv.control.check().unwrap_err();
            assert_eq!(
                serde_json::to_value(error).unwrap(),
                serde_json::to_value(original).unwrap()
            );
            gate_held(&fx.home);
        }
    }
    assert_eq!(captures(&fx), 0);
    owner.close().unwrap();
    for command in COMMANDS {
        let mut inv = fx.invocation(command, &[("--spell-id", "7")]);
        inv.flags
            .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
        assert_eq!(dispatch(&inv).unwrap_err().code, "vault.record_missing");
        released(&fx);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn local_spell_profiles_wait_for_real_admission_then_finish_and_release() {
    let fx = fixture();
    for command in COMMANDS {
        let owner =
            data_resources::acquire(&fx.home, Policy::Measurement, &OperationControl::default())
                .unwrap();
        let inv = fx.invocation(command, &[("--spell-id", "7")]);
        let out = std::thread::scope(|threads| {
            let (started_tx, started_rx) = mpsc::channel();
            let (finished_tx, finished_rx) = mpsc::channel();
            let inv = &inv;
            let home = &fx.home;
            let waiter = threads.spawn(move || {
                started_tx.send(()).unwrap();
                let result = dispatch_after_read(inv, &mut || {
                    held(home);
                    Ok(())
                });
                finished_tx.send(()).unwrap();
                result
            });
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let waiting = finished_rx.recv_timeout(Duration::from_millis(75));
            owner.close().unwrap();
            let result = waiter.join().unwrap();
            assert_eq!(waiting, Err(mpsc::RecvTimeoutError::Timeout));
            result.unwrap()
        });
        verify(&fx, command, &out);
        released(&fx);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn spell_profiles_cancel_queued_admission_without_pin_read_capture_or_foreign_release() {
    let fx = fixture();
    let owner =
        data_resources::acquire(&fx.home, Policy::Measurement, &OperationControl::default())
            .unwrap();
    for command in COMMANDS {
        for cdn in [false, true] {
            let mut inv = fx.invocation(command, &[("--spell-id", "7")]);
            if cdn {
                inv.flags.insert("--cdn".into(), "true".into());
                inv.flags.remove("--installation");
            }
            inv.flags
                .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
            let cancelled = TrackedCancellation::new();
            inv.control = OperationControl::tracked(
                Instant::now() + Duration::from_secs(5),
                cancelled.clone(),
            );
            let error = std::thread::scope(|threads| {
                let (started_tx, started_rx) = mpsc::channel();
                let (finished_tx, finished_rx) = mpsc::channel();
                let inv = &inv;
                let waiter = threads.spawn(move || {
                    started_tx.send(()).unwrap();
                    let result =
                        dispatch_after_read(inv, &mut || panic!("must not read before admission"));
                    finished_tx.send(()).unwrap();
                    result
                });
                started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                let waiting = finished_rx.recv_timeout(Duration::from_millis(75));
                cancelled.cancel();
                let result = waiter.join().unwrap();
                assert_eq!(waiting, Err(mpsc::RecvTimeoutError::Timeout));
                result.unwrap_err()
            });
            let original = inv.control.check().unwrap_err();
            assert_eq!(original.code, "command.cancelled");
            assert_eq!(
                serde_json::to_value(error).unwrap(),
                serde_json::to_value(original).unwrap()
            );
            gate_held(&fx.home);
            assert_eq!(captures(&fx), 0);
        }
    }
    owner.close().unwrap();
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn local_spell_profiles_cancel_after_read_without_capture_and_release_admission() {
    let fx = fixture();
    for command in COMMANDS {
        let mut inv = fx.invocation(command, &[("--spell-id", "7")]);
        let cancelled = TrackedCancellation::new();
        inv.control = OperationControl::tracked(inv.control.deadline(), cancelled.clone());
        let mut visited = false;
        let error = dispatch_after_read(&inv, &mut || {
            visited = true;
            held(&fx.home);
            cancelled.cancel();
            Ok(())
        })
        .unwrap_err();
        assert!(visited);
        let original = inv.control.check().unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::to_value(original).unwrap()
        );
        assert_eq!(captures(&fx), 0);
        released(&fx);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn local_spell_profiles_source_change_after_read_rejects_capture_and_releases_admission() {
    let fx = fixture();
    for command in COMMANDS {
        let inv = fx.invocation(command, &[("--spell-id", "7")]);
        let mut visited = false;
        let result = dispatch_after_read(&inv, &mut || {
            visited = true;
            held(&fx.home);
            fs::write(fx.client.join("version.txt"), "12.1.0.69934")?;
            Ok(())
        });
        fs::write(fx.client.join("version.txt"), catalog_fixture::BUILD).unwrap();
        assert!(visited);
        assert_eq!(result.unwrap_err().code, "records.pinned_build_changed");
        assert_eq!(captures(&fx), 0);
        released(&fx);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn local_spell_profiles_callback_failure_stays_primary_over_cancelled_cleanup() {
    let fx = fixture();
    for command in COMMANDS {
        let mut inv = fx.invocation(command, &[("--spell-id", "7")]);
        let cancelled = TrackedCancellation::new();
        inv.control = OperationControl::tracked(inv.control.deadline(), cancelled.clone());
        let primary = fault("fixture.after_read", 4, "original callback failure");
        let error = dispatch_after_read(&inv, &mut || {
            held(&fx.home);
            cancelled.cancel();
            Err(primary.clone())
        })
        .unwrap_err();
        assert_eq!(error.code, primary.code);
        assert_eq!(error.exit_code, primary.exit_code);
        assert_eq!(error.message, primary.message);
        assert_eq!(
            error.details["schema"],
            "lycheedev.admitted-workspace-errors.v1"
        );
        assert_eq!(
            error.details["causes"],
            json!([primary, inv.control.check().unwrap_err()])
        );
        assert_eq!(captures(&fx), 0);
        released(&fx);
    }
    fs::remove_dir_all(fx.root).unwrap();
}
