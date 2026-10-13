//! Public entry points inject native capabilities; all progression belongs to the lifecycle.
use crate::{journal::DurableStore, slots::Instance};
use crate::{
    lifecycle::{Action, Phase, Session, Store, Transport},
    managed::ManagedInstallation,
};
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError};
use lycheedev_core::{metadata::Metadata, storage, workspace};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub window: lycheedev_windows::WindowIdentity,
    pub installation: PathBuf,
    pub actor: String,
    pub build: String,
    pub module_sha256: String,
    pub runtime: String,
}
pub trait Services {
    fn instances(&self, inv: &Invocation) -> Result<crate::passive::Report> {
        crate::passive::discover(inv, &crate::passive::WindowsSource)
    }
    fn targets(&self, inv: &Invocation) -> Result<Vec<Target>>;
    fn open(&self, target: &Target, session: Option<&Session>) -> Result<Box<dyn Transport>>;
    fn managed(&self) -> &dyn ManagedInstallation;
}
pub fn dispatch_with(inv: &Invocation, services: &impl Services) -> Result<Outcome> {
    // Validate the built-in observation before touching journals or native services.
    let bugs = bugs_source(inv)?;
    if inv.command == "live connect"
        && let Some(session) = inv.flag("--session")
    {
        let mut resume = inv.clone();
        resume.command = "live resume".into();
        resume.positional = Some(session.into());
        return dispatch_with(&resume, services);
    }
    if inv.command.starts_with("live probe ")
        && [
            "live probe put",
            "live probe list",
            "live probe show",
            "live probe remove",
        ]
        .contains(&inv.command.as_str())
    {
        return crate::probes::dispatch(inv);
    }
    if inv.command == "live instances" {
        return Ok(Outcome::new(serde_json::to_value(services.instances(inv)?)?));
    }
    if inv.command == "live connect" {
        let targets = services.targets(inv)?;
        if targets.len() != 1 {
            return Err(ToolkitError::new(
                "selection.ambiguous",
                "select exactly one current game instance",
                2,
            )
            .with_details(serde_json::json!({"targets":targets})));
        }
        let target = &targets[0];
        workspace::open(&inv.home)?;
        let mut transport = services.open(target, None)?;
        if target.actor.is_empty() || target.runtime.len() != 32 {
            return Err(ToolkitError::new(
                "live.identity_unavailable",
                "qualified current runtime and actor are required",
                3,
            ));
        }
        services.managed().verify_clean(
            &target.installation,
            lycheedev_core::VERSION,
            crate::SLOT_TRANSPORT,
            crate::SLOT_COUNT,
        )?;
        let owner = lycheedev_windows::random::token().map_err(native_error)?;
        let epoch = lycheedev_windows::random::token().map_err(native_error)?;
        let mut session = Session::new(
            Instance {
                pid: target.window.process.pid,
                created: target.window.process.created,
                module_sha256: target.module_sha256.clone(),
                runtime: target.runtime.clone(),
            },
            target.actor.clone(),
            epoch,
            owner.clone(),
        );
        let root = inv.home.join("live/v2");
        fs::create_dir_all(&root)?;
        storage::atomic_json(&root.join(format!("CON-{owner}.target.json")), target)?;
        let mut store = DurableStore::create(&inv.home, &target.installation, &session)
            .map_err(native_error)?;
        transport
            .prepare_connection(&mut session)
            .map_err(native_error)?;
        store.checkpoint(&session).map_err(native_error)?;
        drive(inv, &mut session, &mut store, transport.as_mut())?;
        return view(inv, &session);
    }
    if ["live status", "live session"].contains(&inv.command.as_str()) {
        workspace::open(&inv.home)?;
        let (owner, requested) = resolve(inv, inv.argument()?)?;
        let session = read_session(inv, &owner, requested.as_deref())?;
        return view(inv, &session);
    }
    if [
        "live execute",
        "live bugs",
        "live probe load",
        "live resume",
        "live disconnect",
        "live run",
        "live finish",
        "live ack",
        "live reload",
        "live cancel",
        "live abandon",
    ]
    .contains(&inv.command.as_str())
    {
        workspace::open(&inv.home)?;
        let selector = if [
            "live execute",
            "live bugs",
            "live probe load",
            "live reload",
        ]
        .contains(&inv.command.as_str())
        {
            inv.required("--session")?
        } else {
            inv.argument()?
        };
        let (owner, requested) = resolve(inv, selector)?;
        let target: Target = storage::read_json(
            &inv.home
                .join("live/v2")
                .join(format!("CON-{owner}.target.json")),
        )?;
        let (mut store, mut session) =
            DurableStore::resume(&inv.home, &target.installation, &owner).map_err(native_error)?;
        // Validate the exact request before any host mutation or native capability is opened.
        if let Some(id) = requested.as_deref()
            && session.request.as_ref().is_none_or(|r| r.id != id)
            && session
                .capacity_rotation
                .as_ref()
                .is_none_or(|r| r.next.id != id)
        {
            let original = read_session(inv, &owner, Some(id))?;
            if matches!(
                original.phase,
                Phase::Settled | Phase::Cancelled | Phase::Abandoned
            ) {
                return view(inv, &original);
            }
            return Err(ToolkitError::new(
                "live.original_lifecycle_required",
                "request is not the current frozen task",
                6,
            ));
        }
        if matches!(
            session.phase,
            Phase::Closed | Phase::Cancelled | Phase::Abandoned
        ) && inv.command != "live reload"
            && session.capacity_rotation.is_none()
        {
            store.complete_terminal(&session).map_err(native_error)?;
            return view(inv, &session);
        }
        if ["live cancel", "live abandon"].contains(&inv.command.as_str()) {
            if inv.command == "live cancel" {
                session.cancel().map_err(native_error)?;
            } else {
                session.abandon().map_err(native_error)?;
            }
            store.checkpoint(&session).map_err(native_error)?;
            return view(inv, &session);
        }
        if inv.command == "live reload" {
            let mut observation = services.open(&target, Some(&session))?;
            session
                .capture_available_report(&mut store, observation.as_mut())
                .map_err(native_error)?;
            let key = inv.required("--request")?;
            if key.is_empty() || key.len() > 256 {
                return Err(ToolkitError::arguments(
                    "reload request must be 1..256 bytes",
                ));
            }
            let request =
                storage::sha256(format!("{owner}:reload:{key}").as_bytes())[..32].to_owned();
            if session
                .reload
                .as_ref()
                .is_some_and(|reload| reload.request == request && reload.to_runtime.is_some())
            {
                return view(inv, &session);
            }
            let budget = inv.control.remaining()?.as_millis().min(i64::MAX as u128) as i64;
            session
                .request_reload(request, chrono::Utc::now().timestamp_millis() + budget)
                .map_err(native_error)?;
            store.checkpoint(&session).map_err(native_error)?;
            store.reactivate().map_err(native_error)?;
        }
        if ["live execute", "live bugs", "live probe load"].contains(&inv.command.as_str()) {
            if inv.command == "live probe load" {
                inv.required("--probe")?;
                inv.required("--request")?;
                inv.required("--budget-seconds")?;
            }
            let code = if let Some(code) = bugs {
                code.into_bytes()
            } else {
                match (inv.flag("--file"), inv.flag("--probe")) {
                    (Some(path), None) => {
                        let meta = fs::metadata(path)?;
                        if !meta.is_file() || meta.len() == 0 || meta.len() > 262144 {
                            return Err(ToolkitError::arguments(
                                "probe must be a non-empty regular file of at most 256 KiB",
                            ));
                        }
                        fs::read(path)?
                    }
                    (None, Some(name)) => crate::probes::code(inv, name)?,
                    _ => {
                        return Err(ToolkitError::arguments(
                            "live execute requires exactly one of --file or --probe",
                        ));
                    }
                }
            };
            let code = String::from_utf8(code)
                .map_err(|_| ToolkitError::invalid("Lua source must be UTF-8"))?;
            let budget = if inv.command == "live bugs" {
                5
            } else {
                inv.usize_flag("--budget-seconds", 120, 1, 120)? as u16
            };
            let key = inv
                .flag("--request")
                .map(str::to_owned)
                .unwrap_or(lycheedev_windows::random::token().map_err(native_error)?);
            if key.is_empty() || key.len() > 256 {
                return Err(ToolkitError::arguments(
                    "request key must be nonempty and at most 256 bytes",
                ));
            }
            let id = storage::sha256(format!("{owner}:{key}").as_bytes())[..32].to_owned();
            let digest = storage::sha256(code.as_bytes());
            let meta = Metadata::open(&inv.home, true)?;
            let request_key = format!("native/request/{id}");
            if meta.generation(&request_key)?.is_some() {
                let original = read_session(inv, &owner, Some(&id))?;
                let r = original
                    .request
                    .as_ref()
                    .ok_or_else(|| ToolkitError::invalid("request record missing"))?;
                if r.digest != digest || r.budget_seconds != budget {
                    return Err(ToolkitError::new(
                        "live.request_conflict",
                        "same request key has different frozen source or budget",
                        4,
                    ));
                }
                if matches!(
                    original.phase,
                    Phase::Settled | Phase::Cancelled | Phase::Abandoned
                ) {
                    return view(inv, &original);
                }
                if session.request.as_ref().is_none_or(|r| r.id != id)
                    && session
                        .capacity_rotation
                        .as_ref()
                        .is_none_or(|r| r.next.id != id)
                {
                    return Err(ToolkitError::new(
                        "live.original_lifecycle_required",
                        "request belongs to retained original lifecycle",
                        6,
                    ));
                }
            } else {
                drop(meta);
                if session.needs_capacity_rotation() {
                    let deadline = chrono::Utc::now().timestamp_millis()
                        + inv.control.remaining()?.as_millis().min(i64::MAX as u128) as i64;
                    session
                        .queue_capacity(
                            id.clone(),
                            code,
                            budget,
                            lycheedev_windows::random::token().map_err(native_error)?,
                            deadline,
                        )
                        .map_err(native_error)?;
                } else {
                    session
                        .prepare(id.clone(), code, budget, u64::MAX)
                        .map_err(native_error)?;
                }
                store.checkpoint(&session).map_err(native_error)?;
                let mut meta = Metadata::open(&inv.home, true)?;
                meta.put(
                    &request_key,
                    &serde_json::json!({"owner":owner,"request":id}),
                    None,
                )?;
            }
            if inv.command == "live probe load" {
                // The immutable host request is the v4 load receipt. No LoD slot or input is spent.
                return view(inv, &session);
            }
        }
        if ["live ack", "live finish"].contains(&inv.command.as_str())
            && (session
                .request
                .as_ref()
                .is_none_or(|r| r.result_digest.is_none())
                || !matches!(
                    session.phase,
                    Phase::ResultDurable
                        | Phase::SettlePublished
                        | Phase::SettleAwaiting
                        | Phase::Settled
                ))
        {
            return Err(ToolkitError::new(
                "live.verified_report_required",
                "acknowledgement requires the exact durable report; use live run or resume",
                6,
            ));
        }
        if inv.command == "live disconnect" {
            session.request_close().map_err(native_error)?;
            store.checkpoint(&session).map_err(native_error)?;
        }
        if let Some(id) = requested
            && session.request.as_ref().is_none_or(|r| r.id != id)
            && session
                .capacity_rotation
                .as_ref()
                .is_none_or(|r| r.next.id != id)
        {
            let original = read_session(inv, &owner, Some(&id))?;
            if original.phase == Phase::Settled {
                return view(inv, &original);
            }
            return Err(ToolkitError::new(
                "live.original_lifecycle_required",
                "request is not the current frozen task",
                6,
            ));
        }
        let mut transport = services.open(&target, Some(&session))?;
        transport
            .prepare_connection(&mut session)
            .map_err(native_error)?;
        transport
            .prepare_request(&mut session)
            .map_err(native_error)?;
        store.checkpoint(&session).map_err(native_error)?;
        drive(inv, &mut session, &mut store, transport.as_mut())?;
        return view(inv, &session);
    }
    Err(ToolkitError::new(
        "live.migration_incomplete",
        "native command integration is being completed",
        3,
    ))
}
fn bugs_source(inv: &Invocation) -> Result<Option<String>> {
    if inv.command != "live bugs" {
        return Ok(None);
    }
    inv.required("--session")?;
    let key = inv.required("--request")?;
    if key.len() > 256 || key.chars().any(|c| (c as u32) < 32 || c == '\u{7f}') {
        return Err(ToolkitError::arguments(
            "live bugs request must be 1..256 bytes without control characters",
        ));
    }
    // The Go public parser defaults --count to 80. Only the bounded integer is
    // interpolated; request/account never enter executable Lua. Native CON uses
    // provider storage, so --account has no SavedVariables effect, as in Go v3.
    let count = inv.usize_flag("--count", 80, 1, 100)?;
    Ok(Some(format!(
        "local snapshot, reason = LycheeDevInternal.Diagnostics.SnapshotRecentErrors({count}, \"provider_storage\")\nreturn {{schema=\"lycheedev.bugs.v1\",status=snapshot and \"completed\" or \"unavailable\",complete=snapshot and snapshot.complete==true or false,snapshot=snapshot,error=reason}}"
    )))
}
fn native_error(reason: String) -> ToolkitError {
    if reason.starts_with("command.deadline") {
        return ToolkitError::new("live.deadline_pending", reason, 6);
    }
    if reason.starts_with("command.cancelled") {
        return ToolkitError::new("live.cancelled", reason, 7);
    }
    let code = reason.split(':').next().unwrap_or("live.native").to_owned();
    let exit = if reason.starts_with("vault.publish") {
        5
    } else if reason.contains("busy") || reason.contains("pending") || reason.contains("missing") {
        6
    } else if reason.contains("changed")
        || reason.contains("unsupported")
        || reason.contains("blocked")
    {
        3
    } else {
        4
    };
    ToolkitError::new(code, reason, exit)
}
fn resolve(inv: &Invocation, selector: &str) -> Result<(String, Option<String>)> {
    if let Some(owner) = selector.strip_prefix("CON-")
        && lycheedev_core::selection::valid_digest(owner, 32)
    {
        return Ok((owner.into(), None));
    }
    if let Some(id) = selector.strip_prefix("MEM-")
        && lycheedev_core::selection::valid_digest(id, 32)
    {
        let meta = Metadata::open(&inv.home, false)?;
        let record: serde_json::Value = meta.read(&format!("native/request/{id}"))?;
        return Ok((
            record["owner"]
                .as_str()
                .ok_or_else(|| ToolkitError::invalid("request owner missing"))?
                .into(),
            Some(id.into()),
        ));
    }
    Err(ToolkitError::new(
        "live.journal_protocol_mismatch",
        "this native lifecycle requires an exact v4 CON/MEM record; older unresolved protocols require their own recovery",
        4,
    ))
}
fn read_session(inv: &Invocation, owner: &str, request: Option<&str>) -> Result<Session> {
    let root = inv.home.join("live/v2");
    let current = root.join(format!("CON-{owner}.json"));
    storage::guard_path(&inv.home, &current)?;
    if fs::metadata(&current)?.len() > 4 << 20 {
        return Err(ToolkitError::invalid("journal byte budget exceeded"));
    }
    let s: Session = storage::read_json(&current)?;
    if s.schema != "lycheedev.lifecycle.v2" || s.owner != owner {
        return Err(ToolkitError::invalid("journal identity differs"));
    }
    if let Some(id) = request
        && let Some(rotation) = &s.capacity_rotation
    {
        if rotation.next.id == id {
            let mut projected = s.clone();
            projected.request = Some(rotation.next.clone());
            return Ok(projected);
        }
        // The previous verified operation remains independently addressable during rotation.
        if s.request.as_ref().is_some_and(|r| r.id == id) {
            return storage::read_json(&root.join("results").join(format!("{id}.metadata.json")));
        }
    }
    if let Some(id) = request
        && s.request.as_ref().is_none_or(|r| r.id != id)
    {
        let path = root.join("results").join(format!("{id}.metadata.json"));
        storage::guard_path(&inv.home, &path)?;
        let s: Session = storage::read_json(&path)?;
        if s.owner != owner || s.request.as_ref().is_none_or(|r| r.id != id) {
            return Err(ToolkitError::invalid("retained request identity differs"));
        }
        return Ok(s);
    }
    Ok(s)
}
fn drive(
    inv: &Invocation,
    session: &mut Session,
    store: &mut DurableStore,
    transport: &mut dyn Transport,
) -> Result<()> {
    let deadline =
        Instant::now() + Duration::from_secs(inv.usize_flag("--wait-seconds", 120, 1, 600)? as u64);
    for _ in 0..512 {
        if inv.command == "live run"
            && matches!(session.phase, Phase::ResultDurable | Phase::Settled)
        {
            return Ok(());
        }
        inv.control.check().map_err(|error| {
            if error.code == "command.deadline" {
                ToolkitError::new("live.deadline_pending", error.message, 6)
            } else {
                error
            }
        })?;
        let before = session.phase.clone();
        let action = session.advance(store, transport, 0).map_err(native_error)?;
        if matches!(
            session.phase,
            Phase::Idle
                | Phase::Settled
                | Phase::Closed
                | Phase::Unknown
                | Phase::Cancelled
                | Phase::Abandoned
        ) {
            return Ok(());
        }
        if action == Action::Observe && before == session.phase {
            let left = deadline
                .min(inv.control.deadline())
                .saturating_duration_since(Instant::now());
            if left.is_zero()
                || !transport
                    .wait_event(left.min(Duration::from_secs(60)))
                    .map_err(native_error)?
            {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Ok(());
        }
    }
    Err(ToolkitError::new(
        "live.progress_budget",
        "bounded lifecycle steps exhausted",
        6,
    ))
}
fn view(inv: &Invocation, s: &Session) -> Result<Outcome> {
    let projected;
    let s = if let Some(rotation) = &s.capacity_rotation {
        projected = {
            let mut p = s.clone();
            p.request = Some(rotation.next.clone());
            p
        };
        &projected
    } else {
        s
    };
    let mut result = serde_json::Value::Null;
    let mut report = "unavailable";
    if let Some(r) = &s.request
        && let Some(digest) = &r.result_digest
    {
        let path = inv
            .home
            .join("live/v2/results")
            .join(format!("{}.result", r.id));
        storage::guard_path(&inv.home, &path)?;
        let meta = fs::metadata(&path)?;
        if meta.len() > 524288 {
            return Err(ToolkitError::invalid("result byte budget exceeded"));
        }
        let bytes = fs::read(&path)?;
        if storage::sha256(&bytes) != *digest {
            return Err(ToolkitError::new(
                "live.result_corrupt",
                "persisted result differs",
                4,
            ));
        }
        result = serde_json::from_slice(&bytes)?;
        report = "verified";
    }
    let complete = s.capacity_rotation.is_none()
        && (s.phase == Phase::Idle
            || (s.phase == Phase::Closed && !s.execution_unknown)
            || (s.phase == Phase::Settled && s.cleanup_complete));
    let cleanup = if s.capacity_rotation.is_some() {
        "pending"
    } else if s.phase == Phase::Abandoned {
        "abandoned"
    } else if s.phase == Phase::Cancelled {
        "cancelled"
    } else if s.cleanup_complete {
        "complete"
    } else if s.request.is_some() {
        "pending"
    } else {
        "not_requested"
    };
    let operation = s
        .request
        .as_ref()
        .map(|r| format!("MEM-{}", r.id))
        .unwrap_or_else(|| format!("CON-{}", s.owner));
    let mut out = Outcome::new(
        serde_json::json!({"connectionId":format!("CON-{}",s.owner),"operationId":operation,"phase":s.phase,"report":{"state":report,"result":result},"executionUnknown":s.execution_unknown,"cleanup":cleanup,"complete":complete}),
    );
    out.complete = complete;
    out.operation = serde_json::json!({"id":operation,"state":s.phase});
    out.data["capacity"] = serde_json::json!({"pending":s.capacity_rotation.is_some(),"completedRotations":s.capacity_history.len(),"runtimeTasks":s.sequence});
    if let Some(termination) = &s.termination {
        out.data["termination"] = serde_json::to_value(termination)?;
        let target: Target = storage::read_json(
            &inv.home
                .join("live/v2")
                .join(format!("CON-{}.target.json", s.owner)),
        )?;
        let gate = lycheedev_core::coordination::InstallationGuard::acquire(&target.installation)?;
        let claim = gate
            .registry
            .claims
            .iter()
            .find(|claim| claim.owner == s.owner)
            .ok_or_else(|| ToolkitError::invalid("terminal installation claim missing"))?;
        out.data["ownershipReleased"] = serde_json::json!(!claim.pending);
        out.data["runtimeUnloadConfirmed"] = serde_json::json!(false);
    }
    if let Some(reload) = &s.reload {
        out.data["reload"] = serde_json::to_value(reload)?;
        out.data["reload"]["complete"] = serde_json::json!(reload.to_runtime.is_some());
        if inv.command == "live reload" {
            out.complete = reload.to_runtime.is_some();
            out.data["complete"] = serde_json::json!(out.complete);
        }
    }
    Ok(out)
}
#[cfg(test)]
mod control_tests {
    #[test]
    fn bugs_arguments_are_bounded_before_any_workspace_or_native_effect() {
        use lycheedev_core::Invocation;
        use std::collections::BTreeMap;
        let mut inv = Invocation {
            command: "live bugs".into(),
            positional: None,
            flags: BTreeMap::from([
                ("--session".into(), "CON-test".into()),
                ("--request".into(), "bugs-1".into()),
            ]),
            home: Default::default(),
            cwd: Default::default(),
            control: Default::default(),
        };
        assert!(
            super::bugs_source(&inv)
                .unwrap()
                .unwrap()
                .contains("SnapshotRecentErrors(80,")
        );
        for count in ["0", "101", "-1", "1.5", "1);error('injected')"] {
            inv.flags.insert("--count".into(), count.into());
            assert_eq!(super::bugs_source(&inv).unwrap_err().exit_code, 2);
        }
        inv.flags.insert("--count".into(), "100".into());
        for key in ["", "line\nbreak", "delete\u{7f}", &"x".repeat(257)] {
            inv.flags.insert("--request".into(), key.into());
            assert_eq!(super::bugs_source(&inv).unwrap_err().exit_code, 2);
        }
        inv.flags.insert("--request".into(), "bugs-荔枝".into());
        assert!(
            super::bugs_source(&inv)
                .unwrap()
                .unwrap()
                .contains("SnapshotRecentErrors(100,")
        );
    }
    #[test]
    fn native_deadline_and_caller_cancel_remain_distinct() {
        let expired = super::native_error("command.deadline: operation deadline exhausted".into());
        assert_eq!(
            (expired.exit_code, expired.code.as_str()),
            (6, "live.deadline_pending")
        );
        let cancelled = super::native_error("command.cancelled: caller cancellation".into());
        assert_eq!(
            (cancelled.exit_code, cancelled.code.as_str()),
            (7, "live.cancelled")
        );
    }
}
