//! Child module of `luals`. Requires a sealed standalone helper.
//! Run each ignored test in its own controlled process, exclusively; ~1.75 GiB
//! commit is expected. These are quota-boundary observations, not proof that
//! any historical LuaLS memory failure or AV had this cause.
use super::*;

struct SystemConsoleImage {
    path: PathBuf,
    sha256: String,
    _held: fs::File,
}

fn system_console_image() -> SystemConsoleImage {
    use std::os::windows::{ffi::OsStringExt, fs::OpenOptionsExt};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    assert!(length > 0 && (length as usize) < buffer.len());
    let directory = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize]));
    let path = ordinary_path(directory.join("conhost.exe").canonicalize().unwrap());
    let mut held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    let metadata = held.metadata().unwrap();
    assert!(metadata.is_file() && metadata.len() <= 32 << 20);
    let mut raw = Vec::new();
    Read::read_to_end(&mut (&mut held).take((32 << 20) + 1), &mut raw).unwrap();
    assert_eq!(raw.len() as u64, metadata.len());
    SystemConsoleImage {
        path,
        sha256: storage::sha256(&raw),
        _held: held,
    }
}

#[derive(Clone, serde::Serialize)]
struct ExpectedOSConsoleRole {
    pid: u32,
    created: String,
    parent_pid: u32,
    parent_created: String,
    image_path: PathBuf,
    image_sha256: String,
}

fn observed_members(
    members: &lycheedev_windows::job::JobMembership,
) -> (Vec<lycheedev_windows::Process>, Vec<Value>) {
    let mut held = Vec::new();
    let mut rows = Vec::new();
    for &pid in &members.member_pids {
        match lycheedev_windows::Process::open(pid) {
            Ok(process) => {
                rows.push(json!({
                    "pid":process.identity().pid,"created":process.identity().created.to_string(),
                    "imagePath":process.image_path(),"parentPid":process.parent_pid(),"memory":process.memory(),
                    "scope":"held process object opened from bounded Job snapshot; observations never classify by basename"
                }));
                held.push(process);
            }
            Err(error) => rows.push(json!({"pid":pid,"openError":error})),
        }
    }
    (held, rows)
}

fn console_roles(
    members: &lycheedev_windows::job::JobMembership,
    held: &[lycheedev_windows::Process],
    peers: &[&lycheedev_windows::Process],
    image: &SystemConsoleImage,
) -> std::result::Result<Vec<ExpectedOSConsoleRole>, String> {
    if held.len() != members.member_pids.len() {
        return Err("unobserved Job member".into());
    }
    let mut roles = Vec::new();
    let mut parents = std::collections::BTreeSet::new();
    for peer in peers {
        if !held
            .iter()
            .any(|member| member.identity() == peer.identity())
        {
            return Err("known held peer absent from exact Job membership".into());
        }
    }
    for member in held {
        if peers
            .iter()
            .any(|peer| peer.identity() == member.identity())
        {
            continue;
        }
        let actual_path = ordinary_path(member.image_path()?);
        if !actual_path
            .to_string_lossy()
            .eq_ignore_ascii_case(&image.path.to_string_lossy())
        {
            return Err(format!("unknown member image: {}", actual_path.display()));
        }
        let parent_pid = member.parent_pid()?;
        let parent = peers
            .iter()
            .find(|peer| peer.identity().pid == parent_pid)
            .ok_or_else(|| "console parent is not a known held peer".to_owned())?;
        if member.identity().created < parent.identity().created || !parents.insert(parent_pid) {
            return Err("console creation/one-per-known-parent admission failed".into());
        }
        roles.push(ExpectedOSConsoleRole {
            pid: member.identity().pid,
            created: member.identity().created.to_string(),
            parent_pid,
            parent_created: parent.identity().created.to_string(),
            image_path: actual_path,
            image_sha256: image.sha256.clone(),
        });
    }
    Ok(roles)
}

fn observe(mode: &str) {
    let helper = std::env::var_os("LYCHEEDEV_QUOTA_HELPER").expect("sealed helper required");
    let expected =
        std::env::var("LYCHEEDEV_QUOTA_HELPER_SHA256").expect("sealed helper SHA256 required");
    use std::os::windows::fs::OpenOptionsExt;
    let mut held_helper = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&helper)
        .unwrap();
    let metadata = held_helper.metadata().unwrap();
    assert!(metadata.is_file() && metadata.len() <= 32 << 20);
    let mut helper_bytes = Vec::new();
    Read::read_to_end(
        &mut (&mut held_helper).take((32 << 20) + 1),
        &mut helper_bytes,
    )
    .unwrap();
    assert_eq!(helper_bytes.len() as u64, metadata.len());
    let actual_sha256 = storage::sha256(&helper_bytes);
    assert_eq!(actual_sha256, expected);
    drop(helper_bytes);
    let out = std::env::var_os("LYCHEEDEV_QUOTA_REPORT").expect("fresh report path required");
    assert!(!Path::new(&out).exists());
    let root = Path::new(&out).parent().unwrap();
    // Caller prepares the parent evidence directory, but this native case
    // directory must be new. Never overlay an earlier allocation/exit report.
    fs::create_dir(root).unwrap();
    let control = lycheedev_core::control::OperationControl::new(
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
    );
    let mut aggregate = lycheedev_windows::BrokerJob::own_current_process(2 << 30).unwrap();
    let aggregate_before = aggregate.memory_accounting().unwrap();
    let nonce = lycheedev_windows::random::token().unwrap();
    let console_image = system_console_image();
    let mut command = Command::new(&helper);
    command
        .arg(mode)
        .env("LYCHEEDEV_QUOTA_HELPER_AUTH", &nonce)
        .current_dir(root);
    let mut rpc = Rpc::start(command, control).unwrap();
    let child = lycheedev_windows::Process::open(rpc.child.id()).unwrap();
    let prepared = rpc.request("fixture/prepare", json!({})).unwrap();
    let prepared_value: Value = serde_json::from_slice(prepared.result()).unwrap();
    let prepared_result_raw = String::from_utf8(prepared.result().to_vec()).unwrap();
    assert_eq!(prepared_value["nonce"], nonce);
    let pid = prepared_value["descendantPid"].as_u64().unwrap() as u32;
    drop(prepared);
    let descendant = if pid == 0 {
        None
    } else {
        Some(lycheedev_windows::Process::open(pid).unwrap())
    };
    let before = child.memory().unwrap();
    let child_job_before = rpc.job.memory_accounting().unwrap();
    let members_before = rpc.job.membership().unwrap();
    let owner = lycheedev_windows::Process::open(std::process::id()).unwrap();
    let (held_members_before, observed_members_before) = observed_members(&members_before);
    let mut peers = vec![&child];
    if let Some(descendant) = &descendant {
        peers.push(descendant);
    }
    let roles_before = console_roles(
        &members_before,
        &held_members_before,
        &peers,
        &console_image,
    );
    let preallocation = serde_json::to_vec_pretty(&json!({
        "schema":"lycheedev.source-native-quota-preallocation.v1","mode":mode,"nonce":nonce,
        "owner":{"pid":owner.identity().pid,"created":owner.identity().created.to_string(),"imagePath":owner.image_path()},
        "helper":{"path":helper,"sha256":actual_sha256,"bytes":metadata.len(),"heldReadLock":true},
        "originalChild":{"pid":child.identity().pid,"created":child.identity().created.to_string(),"imagePath":child.image_path()},
        "preparedResultRaw":prepared_result_raw,"preparedResult":prepared_value,
        "aggregateBefore":aggregate_before,"childJobBefore":child_job_before,
        "membersBefore":members_before,"observedMembers":observed_members_before,"consoleRoleAdmission":roles_before,"allocationSent":false,
        "systemConsoleImage":{"path":console_image.path,"sha256":console_image.sha256,"heldReadLock":true,"authenticode":"not_claimed"},
        "scope":"saved before admission/limit assertions and allocation; only exact locked system image plus held known-parent and creation bounds may admit one console role per parent"
    })).unwrap();
    assert!(preallocation.len() <= 8 << 20);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("preallocation-observation.json"))
        .unwrap()
        .write_all(&preallocation)
        .unwrap();
    assert_eq!(aggregate_before.job_memory_limit, 2 << 30);
    assert_eq!(
        child_job_before.job_memory_limit,
        LUALS_COMMIT_BUDGET as u64
    );
    assert_ne!(child_job_before.limit_flags & 0x200, 0);
    let roles_before = roles_before.unwrap();
    let mut expected_members = peers
        .iter()
        .map(|peer| peer.identity().pid)
        .collect::<Vec<_>>();
    expected_members.extend(roles_before.iter().map(|role| role.pid));
    expected_members.sort_unstable();
    let mut admitted_members = members_before.member_pids.clone();
    admitted_members.sort_unstable();
    assert_eq!(admitted_members, expected_members);
    // No allocation request is sent until exact limits and held-identity-bound
    // membership have been observed and checked against this case's nonce.
    let allocation = rpc.request("fixture/allocate", json!({})).unwrap();
    let value: Value = serde_json::from_slice(allocation.result()).unwrap();
    assert_eq!(value["nonce"], nonce);
    drop(allocation);
    let after = child.memory().unwrap();
    let descendant_memory = descendant.as_ref().map(|p| p.memory().unwrap());
    let aggregate_after = aggregate.memory_accounting().unwrap();
    let child_job_after = rpc.job.memory_accounting().unwrap();
    let members_after = rpc.job.membership().unwrap();
    let (held_members_after, observed_members_after) = observed_members(&members_after);
    let roles_after = console_roles(&members_after, &held_members_after, &peers, &console_image);
    let postallocation = serde_json::to_vec_pretty(&json!({
        "schema":"lycheedev.source-native-quota-postallocation.v1","mode":mode,"nonce":nonce,
        "allocation":value,"allocationSent":true,"childJobAfter":child_job_after,
        "membersAfter":members_after,"observedMembers":observed_members_after,"consoleRoleAdmission":roles_after,
        "scope":"partial observation saved before postallocation role/limit assertions; no unknown member ignored"
    })).unwrap();
    assert!(postallocation.len() <= 8 << 20);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("postallocation-observation.json"))
        .unwrap()
        .write_all(&postallocation)
        .unwrap();
    let roles_after = roles_after.unwrap();
    for role in &roles_after {
        if let Some(previous) = roles_before
            .iter()
            .find(|previous| previous.parent_pid == role.parent_pid)
        {
            assert_eq!(
                (previous.pid, &previous.created),
                (role.pid, &role.created),
                "one console role per known parent across the case"
            );
        }
    }
    let mut expected_after = peers
        .iter()
        .map(|peer| peer.identity().pid)
        .collect::<Vec<_>>();
    expected_after.extend(roles_after.iter().map(|role| role.pid));
    expected_after.sort_unstable();
    let mut actual_members = members_after.member_pids.clone();
    actual_members.sort_unstable();
    assert_eq!(actual_members, expected_after);
    assert_eq!(
        child_job_before.job_memory_limit,
        LUALS_COMMIT_BUDGET as u64
    );
    assert_eq!(child_job_after.job_memory_limit, LUALS_COMMIT_BUDGET as u64);
    let total_allocated = value["directCommittedBytes"].as_u64().unwrap()
        + value["descendantCommittedBytes"].as_u64().unwrap();
    let error = value["allocationError"].as_u64().unwrap();
    // Refusal must occur near the configured budget; early machine exhaustion
    // fails this test rather than being accepted as a quota demonstration.
    let bound_observed = error != 0
        && total_allocated <= LUALS_COMMIT_BUDGET as u64
        && total_allocated >= LUALS_COMMIT_BUDGET as u64 - (64 << 20);
    drop(rpc);
    let cancel = AtomicBool::new(false);
    let child_exit = child
        .wait_exit(Instant::now() + Duration::from_secs(2), &cancel)
        .unwrap();
    let descendant_exit = descendant.as_ref().map(|p| {
        p.wait_exit(Instant::now() + Duration::from_secs(2), &cancel)
            .unwrap()
    });
    let mut member_retirements = Vec::new();
    let mut retired_identities = std::collections::BTreeSet::new();
    for process in held_members_before.iter().chain(&held_members_after) {
        if retired_identities.insert((process.identity().pid, process.identity().created)) {
            member_retirements.push(json!({
                "pid":process.identity().pid,"created":process.identity().created.to_string(),
                "kernelExit":process.wait_exit(Instant::now() + Duration::from_secs(2), &cancel)
            }));
        }
    }
    let aggregate_retired = aggregate.memory_accounting().unwrap();
    aggregate.disarm().unwrap();
    let report = serde_json::to_vec_pretty(&json!({
        "schema":"lycheedev.source-native-child-quota-observation.v1","mode":mode,"nonce":nonce,
        "helper":{"path":helper,"sha256":actual_sha256,"bytes":metadata.len(),"heldReadLock":true},
        "childIdentity":{"pid":child.identity().pid,"created":child.identity().created.to_string()},
        "descendantIdentity":descendant.as_ref().map(|p| json!({"pid":p.identity().pid,"created":p.identity().created.to_string()})),
        "configuredChildCommitBytes":LUALS_COMMIT_BUDGET,"allocation":value,"before":before,"after":after,
        "aggregateBefore":aggregate_before,"aggregateAfter":aggregate_after,"aggregateRetired":aggregate_retired,
        "childJobBefore":child_job_before,"childJobAfter":child_job_after,
        "membersBefore":members_before,"membersAfter":members_after,
        "consoleRolesBefore":roles_before,"consoleRolesAfter":roles_after,"memberRetirements":member_retirements,
        "systemConsoleImage":{"path":console_image.path,"sha256":console_image.sha256,"heldReadLock":true,"authenticode":"not_claimed"},
        "descendantMemory":descendant_memory,"childExit":child_exit,"descendantExit":descendant_exit,
        "boundObserved":bound_observed,
        "scope":"actual production Rpc JobGuard queried limit/peak/membership and held process identities; original2GiB aggregate; post-attach descendant only; no current aggregate commit or memory-limit notification; near-budget refusal alone does not exclusively attribute its cause"
    })).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .unwrap()
        .write_all(&report)
        .unwrap();
    assert_eq!(child_exit, 7);
    assert!(descendant_exit.is_none_or(|exit| exit == 7));
    assert!(
        member_retirements
            .iter()
            .all(|row| row["kernelExit"]["Ok"].is_u64()),
        "every admitted original member handle must observe a kernel exit"
    );
    assert_eq!(
        aggregate_retired.active_processes, 1,
        "only this test owner may remain in its aggregate Job"
    );
    assert!(
        bound_observed,
        "actual committed allocations did not reach and refuse near the configured partition"
    );
    drop(held_helper);
}

#[test]
#[ignore = "exclusive ~1.75GiB actual native commit; sealed helper and fresh report required"]
fn actual_child_commit_partition_refuses_native_allocations_and_reaps() {
    observe("direct");
}

#[test]
#[ignore = "exclusive ~1.75GiB actual native commit; sealed helper and fresh report required"]
fn post_attach_descendant_shares_actual_child_partition_and_reaps() {
    observe("aggregate");
}
