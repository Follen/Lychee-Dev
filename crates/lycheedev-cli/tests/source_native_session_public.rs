//! Run this complete controller executable through the repository owned-process
//! runner. Its clients are real CLI processes and the retained owner is closed
//! before the controller returns; this is not an in-process session mock.
#![cfg(windows)]
#[path = "support/native_session_faults.rs"]
mod native_session_faults;
#[allow(dead_code)]
#[path = "support/data_agent_supervisor.rs"]
mod data_supervisor;
#[path = "support/independent_owners.rs"]
mod independent_owners;
use lycheedev_core::{
    control::OperationControl,
    evidence,
    selection::{self, SelectionSpec, SourcePin},
};
use lycheedev_source::{environment, git, index, luac, native_research, request::SourceContext};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

fn git_run(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Native source fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Native source fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn cli(home: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_lycheedev"))
        .args(args)
        .current_dir(home.parent().unwrap())
        .args(["--home", home.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(output.status.success(), "{value}");
    assert_eq!(value["schema"], "lycheedev.result.v1");
    assert_eq!(value["ok"], true, "{value}");
    value
}
fn pin(home: &Path, commit: String) -> selection::PinnedSet {
    selection::pin_selection(
        home,
        SelectionSpec {
            source: Some(SourcePin {
                repository: "wow-ui-source".into(),
                product: "retail".into(),
                requested_ref: "fixture".into(),
                exact_commit: commit,
                parser_revision: "source-v1".into(),
            }),
            ..Default::default()
        },
    )
    .unwrap()
}
struct Fixture {
    root: PathBuf,
    home: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // A failed assertion still retires the one session created by this fixture.
        let _ = Command::new(env!("CARGO_BIN_EXE_lycheedev"))
            .args([
                "source",
                "session",
                "close",
                "--home",
                self.home.to_str().unwrap(),
                "--format",
                "json",
            ])
            .output();
        if std::env::var_os("LYCHEEDEV_SOURCE_NATIVE_KEEP_FIXTURE").is_none() {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
fn analysis_child(_home: &Path, owner: u32) -> lycheedev_windows::Process {
    // Optional phase files are not discovery or completion receipts. Inventory
    // the real owner's children with the installed Windows management provider,
    // then authenticate the selected child through an actual held OS handle.
    // This fixture observation is outside the measured request budget.
    let owner_process=lycheedev_windows::Process::open(owner).unwrap();
    owner_process.verify().unwrap();
    let powershell=PathBuf::from(std::env::var_os("SystemRoot").expect("Windows system root"))
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let script=format!("$ErrorActionPreference='Stop'; $p=@(Get-CimInstance Win32_Process -Filter 'ParentProcessId = {owner}'); ConvertTo-Json -Compress -InputObject @($p | ForEach-Object {{ [uint32] $_.ProcessId }})");
    let mut command=Command::new(powershell);
    command.args(["-NoLogo","-NoProfile","-NonInteractive","-Command",&script]);
    let observation_control=OperationControl::uncancelable(Instant::now()+Duration::from_secs(5));
    let output=lycheedev_core::process::run(command,None,65536,65536,&observation_control).unwrap();
    assert_eq!(output.status,0,"{}",String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.len()<=65536);
    let pids:Vec<u32>=serde_json::from_slice(&output.stdout).unwrap();
    let expected=PathBuf::from(env!("CARGO_BIN_EXE_lycheedev")).canonicalize().unwrap();
    let mut children=Vec::new();
    for pid in pids {
        let Ok(child)=lycheedev_windows::Process::open(pid) else {continue};
        if child.image_path().unwrap().canonicalize().unwrap()==expected {
            assert_ne!(pid,owner);
            assert_eq!(child.parent_pid().unwrap(),owner);
            child.verify().unwrap();
            children.push(child);
        }
    }
    owner_process.verify().unwrap();
    assert_eq!(children.len(),1,"one actual retained analysis child");
    children.pop().unwrap()
}
fn rejects_unowned_private_analysis_entry(home: &Path) {
    let marker="--source-native-session-worker";
    let malformed=Command::new(env!("CARGO_BIN_EXE_lycheedev")).arg(marker).output().unwrap();
    assert_eq!(malformed.status.code(),Some(2)); assert!(malformed.stdout.is_empty());
    let own=lycheedev_windows::Process::open(std::process::id()).unwrap();
    let token=lycheedev_windows::random::token().unwrap();
    for created in [own.identity().created.saturating_add(1),own.identity().created] {
        let output=Command::new(env!("CARGO_BIN_EXE_lycheedev"))
            .arg(marker).arg("--home").arg(home)
            .arg("--owner-pid").arg(own.identity().pid.to_string())
            .arg("--owner-created").arg(created.to_string())
            .arg("--start-token").arg(&token)
            .env("LYCHEEDEV_NATIVE_SOURCE_ANALYSIS_START",&token)
            .stdin(std::process::Stdio::null()).output().unwrap();
        // Wrong creation and then correct identity/wrong parent executable must
        // both fail before a private Hello or any source analysis is emitted.
        assert_eq!(output.status.code(),Some(5));assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("actual owner differs"));
    }
}

fn captured(home: &Path, value: &Value) {
    let bytes = evidence::read_blob(home, &value["captures"][0]["blob"], 32 << 20).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        value["result"]
    );
}

#[test]
#[ignore = "requires verified release and whole-controller owned runner; real multi-CLI native session"]
fn public_native_session_reuses_exact_snapshot_and_retires_on_close() {
    let release = PathBuf::from(
        std::env::var_os("LYCHEEDEV_SOURCE_NATIVE_RELEASE")
            .or_else(|| std::env::var_os("LYCHEEDEV_SOURCE_FLOW_RELEASE"))
            .expect("verified release root"),
    );
    // Owned TEMP already separates controller runs. A short fixture basename
    // leaves room for Git's long API-documentation path on Windows.
    let root = std::env::temp_dir().join(format!("ns-{}", std::process::id()));
    // Keep the original short ns root: Windows Git needs the remaining path.
    // One authenticated synthetic Data fixture owns the same investigation/home.
    let data_fixture = data_supervisor::fixture_at(root.clone(), 512, &"x".repeat(4096));
    let home = data_fixture.home.clone();
    let _cleanup = Fixture {
        root: root.clone(),
        home: home.clone(),
    };
    let repository = root.join("repository");
    let docs = repository.join("Interface/AddOns/Blizzard_APIDocumentationGenerated");
    fs::create_dir_all(&docs).unwrap();
    let main = format!(
        "local function Helper(value)\n return value\nend\nfunction Demo()\n local before = Helper(1)\n local value = C_Native.Value()\n return Helper(value)\nend\n--{}\n",
        "full original source ".repeat(4000)
    );
    assert!(main.len() > 65536);
    fs::write(repository.join("main.lua"), &main).unwrap();
    let mut toc = "## Interface: 120100\n".to_owned();
    for number in 0..38 {
        let path = format!("part{number}.lua");
        fs::write(
            repository.join(&path),
            format!("local part{number} = {number}\n"),
        )
        .unwrap();
        toc.push_str(&format!("{path}\n"));
    }
    toc.push_str("main.lua\n");
    fs::write(repository.join("Main.toc"), toc).unwrap();
    fs::write(docs.join("NativeDocumentation.lua"), r#"local API={Name="Native",Type="System",Namespace="C_Native",Functions={{Name="Value",Type="Function",Returns={{Name="value",Type="number"}}}}}
APIDocumentation:AddDocumentationTable(API)
"#).unwrap();
    git_run(&repository, &["init"]);
    git_run(&repository, &["add", "."]);
    git_run(
        &repository,
        &["commit", "-m", "fixed native session source"],
    );
    let first_commit = git_run(&repository, &["rev-parse", "HEAD"]);
    fs::write(
        repository.join("main.lua"),
        format!("{main}-- changed snapshot identity\n"),
    )
    .unwrap();
    git_run(&repository, &["add", "main.lua"]);
    git_run(&repository, &["commit", "-m", "second fixed snapshot"]);
    let second_commit = git_run(&repository, &["rev-parse", "HEAD"]);
    let mirror = git::mirror(&home, "wow-ui-source");
    fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    git_run(
        &repository,
        &["clone", "--bare", ".", mirror.to_str().unwrap()],
    );
    let first_pin = pin(&home, first_commit);
    let second_pin = pin(&home, second_commit);
    rejects_unowned_private_analysis_entry(&home);
    let before = cli(&home, &["source", "session", "status"]);
    assert_eq!(before["result"]["state"], "closed");
    assert_eq!(before["result"]["backend"], "native");
    let refs = cli(
        &home,
        &[
            "source",
            "refs",
            "--snapshot",
            &first_pin.id,
            "--symbol",
            "Demo",
            "--direction",
            "outgoing",
            "--limit",
            "200",
            "--session-reuse",
            "--release",
            release.to_str().unwrap(),
        ],
    );
    let first = &refs["result"]["nativeResearch"];
    assert_eq!(first["schema"], "lycheedev.source-native-research.v1");
    assert_eq!(first["analysis"]["sourceFiles"], 40);
    assert_eq!(first["analysis"]["cacheHit"], false);
    assert_eq!(first["nativeSession"]["snapshotTransfer"], "full");
    assert!(
        first.get("relations").is_none(),
        "evidence must not bypass outer pagination"
    );
    let pid = first["nativeSession"]["workerPid"].as_u64().unwrap() as u32;
    let owner = lycheedev_windows::Process::open(pid).unwrap();
    let created = owner.identity().created;
    let first_analysis = analysis_child(&home,pid);
    let first_analysis_identity = first_analysis.identity().clone();
    let helper = refs["result"]["relations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["basis"] == json!(["native_common_source_facts"])
                && row["kind"] == "call"
                && row["target"] == "Helper"
                && row["line"] == 5
        })
        .expect("native Helper call");
    assert_eq!(helper["state"], "resolved", "{helper}");
    assert!(helper["observations"].as_array().unwrap().iter().any(|observation| {
        observation["occurrence"]["kind"] == "entry"
            && observation["selected"]["state"] == "known"
    }));
    let late_helper = refs["result"]["relations"].as_array().unwrap().iter().find(|row| {
        row["basis"] == json!(["native_common_source_facts"])
            && row["kind"] == "call" && row["target"] == "Helper" && row["line"] == 7
    }).expect("native Helper after unknown host effect");
    assert_eq!(late_helper["lexical"]["state"], "known");
    assert_eq!(late_helper["state"], "static_candidate", "{late_helper}");
    assert!(late_helper["observations"].as_array().unwrap().iter().any(|observation| {
        observation["occurrence"]["kind"] == "entry"
            && observation["selected"]["state"] == "unavailable"
    }), "{late_helper}");
    assert_eq!(first["entryResolution"]["state"], "unique");
    assert_eq!(first["entryEvidence"]["sourceExecution"], false);
    assert_eq!(first["entryEvidence"]["entries"][0]["request"]["arguments"]["kind"], "unspecified");
    let site = &helper["site"];
    assert_eq!(
        &main[site["start"].as_u64().unwrap() as usize..site["end"].as_u64().unwrap() as usize],
        "Helper"
    );
    assert!(
        first["apiDeclarations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|api| api["name"] == "C_Native.Value" && api["returns"][0]["type"] == "number")
    );
    captured(&home, &refs);
    let independent_data_failure = independent_owners::data_failure(&data_fixture);
    owner.verify().unwrap();
    first_analysis.verify().unwrap();
    assert_eq!(lycheedev_windows::process::observe_lifetime(owner.identity(), || Ok(())).unwrap(),
        lycheedev_windows::process::ProcessLifetime::Running, "Data failure must leave the retained Source owner alive");
    // The existing context request below must reuse this same owner, original
    // creation time and same real analysis child with an identity-only warm hit.
    let context = cli(
        &home,
        &[
            "source",
            "context",
            "--snapshot",
            &first_pin.id,
            "--symbol",
            "Demo",
            "--limit",
            "200",
            "--session-reuse",
            "--release",
            release.to_str().unwrap(),
        ],
    );
    let second = &context["result"]["nativeResearch"];
    assert_eq!(analysis_child(&home,pid).identity(),&first_analysis_identity,"warm cache must stay in the same real child");
    first_analysis.verify().unwrap();
    assert_eq!(second["nativeSession"]["workerPid"], pid);
    assert_eq!(second["analysis"]["cacheHit"], true);
    assert_eq!(second["nativeSession"]["snapshotTransfer"], "identity_only");
    assert_eq!(second["inputIdentity"], first["inputIdentity"]);
    assert_eq!(
        lycheedev_windows::Process::open(pid)
            .unwrap()
            .identity()
            .created,
        created
    );
    captured(&home, &context);
    let status = cli(&home, &["source", "session", "status"]);
    assert_eq!(status["result"]["state"], "ready");
    assert_eq!(status["result"]["workerPid"], pid);
    assert_eq!(status["result"]["ownerPid"], pid);
    let changed = cli(
        &home,
        &[
            "source",
            "refs",
            "--snapshot",
            &second_pin.id,
            "--symbol",
            "Demo",
            "--direction",
            "outgoing",
            "--session-reuse",
            "--release",
            release.to_str().unwrap(),
        ],
    );
    let third = &changed["result"]["nativeResearch"];
    assert_eq!(third["nativeSession"]["workerPid"], pid);
    assert_eq!(third["analysis"]["cacheHit"], false);
    assert_eq!(third["nativeSession"]["snapshotTransfer"], "full");
    assert_ne!(third["inputIdentity"], first["inputIdentity"]);
    captured(&home, &changed);
    first_analysis.wait_exit_with_control(Instant::now()+Duration::from_secs(2),||Ok(())).unwrap();
    assert_ne!(analysis_child(&home,pid).identity(),&first_analysis_identity);
    let source_context = SourceContext { home: home.clone(), cwd: repository.clone(), control: OperationControl::default() };
    let source = first_pin.source.as_ref().unwrap();
    let indexed = index::build_with_control(&home, source, &source_context.control).unwrap();
    let environment = environment::from_pin(&home, source, &source_context.control).unwrap();
    let manifest: luac::Manifest =
        serde_json::from_slice(&fs::read(release.join("payload/tool/luac/runtime.json")).unwrap())
            .unwrap();
    let runtime = luac::Runtime::verified(&release.join("payload/tool/luac"), manifest).unwrap();
    let snapshot = native_research::snapshot_request(
        &source_context,
        source,
        &indexed,
        Some(&environment),
        &runtime,
    )
    .unwrap();
    let query = native_research::QueryRequest {
        symbol: indexed
            .symbols
            .iter()
            .find(|symbol| symbol.kind == "declaration" && symbol.name == "Demo")
            .unwrap()
            .clone(),
        direction: "outgoing".into(),
    };
    let request = native_research::WorkerRequest { snapshot, query };
    let faults = native_session_faults::exercise(&home, serde_json::to_value(&request).unwrap());
    let recovered = cli(
        &home,
        &[
            "source",
            "refs",
            "--snapshot",
            &first_pin.id,
            "--symbol",
            "Demo",
            "--direction",
            "outgoing",
            "--session-reuse",
            "--release",
            release.to_str().unwrap(),
        ],
    );
    let recovered_native = &recovered["result"]["nativeResearch"];
    assert_eq!(recovered_native["nativeSession"]["workerPid"], pid);
    assert_eq!(recovered_native["analysis"]["cacheHit"], false);
    assert_eq!(recovered_native["nativeSession"]["snapshotTransfer"], "full");
    assert_eq!(recovered_native["inputIdentity"], first["inputIdentity"]);
    captured(&home, &recovered);
    let recovered_analysis = analysis_child(&home,pid);
    let closed = cli(&home, &["source", "session", "close"]);
    assert_eq!(closed["result"]["state"], "closed");
    assert_ne!(lycheedev_windows::process::observe_lifetime(recovered_analysis.identity(),||Ok(())).unwrap(),lycheedev_windows::process::ProcessLifetime::Running);
    let worker_exit = owner
        .wait_exit_with_control(
            Instant::now() + Duration::from_secs(5),
            ||Ok(()),
        )
        .unwrap();
    assert_eq!(worker_exit, 0);
    let after = cli(&home, &["source", "session", "status"]);
    assert_eq!(after["result"]["state"], "closed");
    // A new uncached symbol forces the real cold-start handshake again after
    // verified close. The owner must be admitted while its analysis is idle;
    // workerPid is legitimately zero in the startup Status response.
    let reopened = cli(&home, &[
        "source", "refs", "--snapshot", &first_pin.id, "--symbol", "Helper",
        "--direction", "outgoing", "--session-reuse", "--release", release.to_str().unwrap(),
    ]);
    let reopened_native = &reopened["result"]["nativeResearch"];
    assert_eq!(reopened_native["analysis"]["cacheHit"], false);
    assert_eq!(reopened_native["nativeSession"]["snapshotTransfer"], "full");
    let reopened_pid = reopened_native["nativeSession"]["workerPid"].as_u64().unwrap() as u32;
    let reopened_owner = lycheedev_windows::Process::open(reopened_pid).unwrap();
    assert_ne!((reopened_pid, reopened_owner.identity().created), (pid, created));
    let idle_analysis = analysis_child(&home,reopened_pid);
    let reopened_status = cli(&home, &["source", "session", "status"]);
    assert_eq!(reopened_status["result"]["ownerPid"], reopened_pid);
    assert_eq!(reopened_status["result"]["state"], "ready");
    // Real warm analysis must retire on the original 60-second idle trigger.
    // Waiting observes the fixed owner lifecycle; it grants no request grace.
    let idle_begun=Instant::now();
    let idle_exit=reopened_owner.wait_exit_with_control(idle_begun+Duration::from_secs(65),||Ok(())).unwrap();
    let idle_elapsed=idle_begun.elapsed();
    assert_eq!(idle_exit,0);
    assert!(idle_elapsed>=Duration::from_secs(58),"warm owner retired before its idle boundary");
    assert_ne!(lycheedev_windows::process::observe_lifetime(idle_analysis.identity(),||Ok(())).unwrap(),lycheedev_windows::process::ProcessLifetime::Running);
    let final_status = cli(&home, &["source", "session", "status"]);
    assert_eq!(final_status["result"]["state"], "closed");
    if std::env::var_os("LYCHEEDEV_SOURCE_NATIVE_KEEP_FIXTURE").is_some() {
        let worker_request = root.join("worker-request.json");
        fs::write(&worker_request, serde_json::to_vec(&request).unwrap()).unwrap();
        fs::write(root.join("report.json"), serde_json::to_vec_pretty(&json!({"home":home,"pin":first_pin,"changedPin":second_pin,
            "release":release,"cli":env!("CARGO_BIN_EXE_lycheedev"),"workerRequestPath":worker_request,
            "workerPid":pid,"workerCreated":created,"analysisIdentity":first_analysis_identity,"closedAnalysisIdentity":recovered_analysis.identity(),"workerExit":worker_exit,"before":before,"refs":refs,"context":context,
            "status":status,"independentDataFailure":independent_data_failure,"changed":changed,"faults":faults,"recovered":recovered,"closed":closed,"after":after,"reopened":reopened,"reopenedStatus":reopened_status,"idleAnalysisIdentity":idle_analysis.identity(),"idleExit":idle_exit,"idleElapsedMs":idle_elapsed.as_millis(),"finalStatus":final_status})).unwrap()).unwrap();
        println!("{}", root.display());
    }
}
