//! Diagnostic seam: retain the same authenticated full Lua workspace twice
//! under the production aggregate commit limit, without named-pipe/CLI callers.
use lycheedev_core::{control::OperationControl, selection, storage};
use lycheedev_source::{environment, git, index, luals::Runtime};
use serde_json::json;
use sha2::Digest;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[test]
#[ignore = "exact pinned full Source workspace and LuaLS ZIP; diagnostic two-request 2GiB job"]
fn actual_two_full_workspace_batches_under_aggregate_commit_limit() {
    let home = PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("fixed home"));
    let pin = selection::load_pinned(
        &home,
        &std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin"),
    )
    .unwrap()
    .spec
    .source
    .unwrap();
    let zip = PathBuf::from(std::env::var_os("LYCHEEDEV_LUALS_ZIP").expect("exact ZIP"));
    let staging =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/rust-source-repeat-session-real");
    fs::create_dir_all(&staging).unwrap();
    let runtime_root = staging.join(format!("runtime-{}", std::process::id()));
    let runtime = Runtime::unpack_pinned(&zip, &runtime_root).unwrap();
    let control = OperationControl::default();
    let indexed = index::build_with_control(&home, &pin, &control).unwrap();
    let env = environment::from_pin(&home, &pin, &control).unwrap();
    let definitions_sha = storage::sha256(env.definitions.as_bytes());
    let mut source_identity = sha2::Sha256::new();
    {
        for document in &indexed.documents {
            source_identity.update(document.path.as_bytes());
            source_identity.update([0]);
            source_identity.update(document.sha256.as_bytes());
            source_identity.update([0]);
            source_identity.update(document.bytes.to_le_bytes());
        }
    }
    let documents = indexed.documents.len();
    let source_sha = { format!("{:x}", source_identity.finalize()) };
    // Independent Git tree + framed-blob manifest, retained alongside the
    // experiment, binds the entire source set before any semantic work starts.
    assert_eq!(pin.exact_commit, "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b");
    assert_eq!(documents, 4036);
    assert_eq!(
        source_sha,
        "88d68f2c4bdb352245cbcb53cd41ec906b294e2ea599ee6374e00be169be7928"
    );
    let symbol = indexed
        .symbols
        .iter()
        .find(|s| s.kind == "declaration" && s.name == "CallbackRegistryMixin:RegisterCallback")
        .expect("fixed declaration");
    let (raw, _) =
        git::blob_with_control(&home, &pin, &symbol.path, git::MAX_SOURCE_BYTES, &control).unwrap();
    let line = std::str::from_utf8(&raw)
        .unwrap()
        .lines()
        .nth(symbol.line - 1)
        .unwrap();
    let column = line[..line.find("RegisterCallback").unwrap()]
        .encode_utf16()
        .count()
        + 2;
    let batches = [
        json!({"kind":"references","path":symbol.path,"position":{"line":symbol.line-1,"character":column}}),
        json!({"kind":"hover","path":symbol.path,"position":{"line":symbol.line-1,"character":column}}),
    ];
    let mut job = lycheedev_windows::BrokerJob::own_current_process(2 << 30).unwrap();
    let owner_process = lycheedev_windows::Process::open(std::process::id()).unwrap();
    let before = json!({"phase":"beforeWorker","job":job.memory_accounting().unwrap(),"owner":owner_process.memory().unwrap()});
    let mut session = runtime
        .open_source_session_with_control(
            &home,
            &pin,
            &indexed,
            env.definitions.as_bytes(),
            &control,
        )
        .unwrap();
    let pid = session.pid().unwrap();
    let configuration = session.configuration_identity().unwrap();
    assert_eq!(configuration["coverage"]["luaFiles"], 2551);
    assert_eq!(configuration["coverage"]["excludedFiles"], 0);
    // Hash the actual locked library with bounded scratch in both profiles.
    // Keep the proof even when an analysis error later retires its workspace.
    let locked_library = {
        let library = configuration["configuration"]["workspace.library"][0]
            .as_str()
            .unwrap();
        let mut file = fs::File::open(Path::new(library).join("api.d.lua")).unwrap();
        let mut hash = sha2::Sha256::new();
        let mut buffer = [0; 65536];
        let mut bytes = 0;
        loop {
            let count = file.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            bytes += count;
            assert!(bytes <= 8 << 20);
            hash.update(&buffer[..count]);
        }
        let sha256 = format!("{:x}", hash.finalize());
        assert_eq!(sha256, definitions_sha);
        json!({"sha256":sha256,"bytes":bytes})
    };
    let worker_process = lycheedev_windows::Process::open(pid).unwrap();
    let opened = json!({"phase":"workerOpened","job":job.memory_accounting().unwrap(),"owner":owner_process.memory().unwrap(),"worker":worker_process.memory().unwrap()});
    // This optional diagnostic variable releases only derived objects already
    // consumed and authenticated by the full immutable session. It changes no
    // source/catalog/definition/query inputs or preload/file/memory limits.
    let release_consumed =
        std::env::var("LYCHEEDEV_SOURCE_RELEASE_DERIVED").is_ok_and(|value| value == "1");
    if release_consumed {
        drop(indexed);
        drop(env);
        drop(raw);
    }
    let after_consumed = json!({"phase":"afterConsumedDerivedDecision","released":release_consumed,"job":job.memory_accounting().unwrap(),"owner":owner_process.memory().unwrap(),"worker":worker_process.memory().unwrap()});
    let first = session.analyze_with_control(&batches, &control);
    let after_first = json!({"phase":"afterFirst","job":job.memory_accounting().unwrap(),"owner":owner_process.memory().unwrap(),"worker":worker_process.memory().ok()});
    let second = match &first {
        Ok(_) => session.analyze_with_control(&batches, &control),
        Err(error) => Err(error.clone()),
    };
    let after_second = json!({"phase":"afterSecond","job":job.memory_accounting().unwrap(),"owner":owner_process.memory().unwrap(),"worker":worker_process.memory().ok()});
    let errors = [first.as_ref().err(),second.as_ref().err()].map(|error|error.map(|e|json!({"code":e.code,"exitCode":e.exit_code,"message":e.message.chars().take(1024).collect::<String>()})));
    let same_pid = session.pid() == Some(pid);
    let count = session.batches();
    let same = first
        .as_ref()
        .ok()
        .zip(second.as_ref().ok())
        .is_some_and(|(a, b)| a == b);
    session.close();
    drop(session);
    job.disarm().unwrap();
    let report = std::env::var_os("LYCHEEDEV_SOURCE_MEMORY_REPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|| staging.join("memory-report.json"));
    fs::create_dir_all(report.parent().unwrap()).unwrap();
    fs::write(report, serde_json::to_vec_pretty(&json!({"sourcePin":pin,"workerPid":pid,"batches":count,"samePid":same_pid,"sameAnalysis":same,"errors":errors,"inputIdentity":{"documents":documents,"documentPathSHA256SizeDigest":source_sha,"definitionsSHA256":definitions_sha,"lockedLibrary":locked_library,"actualLockedConfiguration":configuration,"releaseConsumedDerived":release_consumed},"memory":[before,opened,after_consumed,after_first,after_second],"aggregateCommitLimitBytes":2u64<<30,"scope":"SDK two actual full-workspace retained batches under production commit limit; job peak is distinct from unsupported current aggregate commit; process private commit is distinct from RSS; no public pipe broker, not original AV fixed"})).unwrap()).unwrap();
    drop(runtime);
    fs::remove_dir_all(runtime_root).unwrap();
    let first = first.unwrap();
    let second = second.unwrap();
    assert!(same_pid);
    assert_eq!(count, 2);
    assert_eq!(first, second);
    assert_eq!(first["coverage"]["luaFiles"], 2551);
    assert!(
        !first["results"][0]["locations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
