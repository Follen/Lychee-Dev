//! Authenticated, runner-owned inputs only. No PATH lookup or user source execution.
#![forbid(unsafe_code)]
use lycheedev_core::{control::OperationControl, storage};
use lycheedev_live::go_mailbox_v1::{
    Identity, SCHEMA,
    driver::{Clock, TokenSource},
    durable_journal::{ChannelPaths, DurableChannelJournal, WindowOwner},
    journal::prepare_event,
    observations::ChannelResult,
    state::{CHANNEL_SCHEMA, DEFAULT_CONNECT_MILLIS, DurableBudget, State},
};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

pub const ORACLE: &str = "29bdbdaff5f064ec212ff2064380b1ccac652eda";
pub const MANIFEST_SHA256: &str =
    "fb48cb3d55f22b553881bece10be49d451418cdc137814ec05e2964aae93baf6";
pub const RELEASE_FIELDS_SHA256: &str =
    "c459550c8f467a1978303aa401d6f3aba5f5ed5f43e77111802d7e78bc3060e4";
const RELEASE_FIELDS: &[u8] =
    include_bytes!("../../../../tests/rust-parity/formal-lua-3.2.4/release-fields.json");
const MANIFEST: &[u8] =
    include_bytes!("../../../../tests/rust-parity/formal-lua-3.2.4/manifest.json");
pub const CODE: &str = "return { message = \"荔枝\\n\\000123\", quoted = [=[x\\y]=] }";
pub const BROKEN_CODE: &str = "return { broken =";
// Exact finite-number set from Go324 TestSlotDriverLuaReportPreservesFiniteNumbers.
// This is the only extra business source admitted by the host parity fixtures.
pub const NUMERIC_CODE: &str = concat!(
    "return {0,-1 / math.huge,",
    "0.1,-0.1,0.5,-0.5,1,-1,1.2345678901234567,-1.2345678901234567,",
    "100000000000000.125,-100000000000000.125,",
    "9007199254740991,-9007199254740991,9007199254740992,-9007199254740992,",
    "9007199254740994,-9007199254740994,",
    "0.9999999999999999,-0.9999999999999999,1.0000000000000002,-1.0000000000000002,",
    "1e-200,-1e-200,2.2250738585072014e-308,-2.2250738585072014e-308,",
    "5e-324,-5e-324,1.7976931348623157e+308,-1.7976931348623157e+308}"
);

// A closed test-only input set, never arbitrary caller-supplied Lua. These
// fixtures execute in the authenticated offline Lua peer, never in a client.
pub const PERFORMANCE_RESULT_STRING_BYTES: [usize; 3] = [256, 16384, 131072];
pub const PERFORMANCE_HISTORY_EVENTS: [usize; 3] = [1, 255, 256];
pub const PERFORMANCE_REPEATS: usize = 3;
pub fn performance_code(bytes: usize) -> String {
    assert!(PERFORMANCE_RESULT_STRING_BYTES.contains(&bytes));
    format!(
        "return {{ message = \"荔枝\\n\\000123\", quoted = [=[x\\y]=], payload = string.rep(\"x\", {bytes}) }}"
    )
}

pub fn ordinary(path: &Path) {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).expect("input path must exist");
        assert!(
            !metadata.file_type().is_symlink(),
            "fixture reparse boundary"
        );
        // Reject Windows junctions as well as symlinks, without introducing FFI.
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            assert_eq!(
                metadata.file_attributes() & 0x400,
                0,
                "fixture reparse point"
            );
        }
    }
}
pub fn read(path: &Path, cap: usize) -> Vec<u8> {
    ordinary(path);
    let metadata = fs::metadata(path).unwrap();
    assert!(
        metadata.is_file() && metadata.len() <= cap as u64,
        "fixture file cap"
    );
    let bytes = fs::read(path).unwrap();
    assert_eq!(bytes.len() as u64, metadata.len(), "fixture file changed");
    bytes
}
fn json(path: &Path, cap: usize) -> Value {
    serde_json::from_slice(&read(path, cap)).unwrap()
}
fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().expect(key)
}
fn descendant(root: &Path, path: &Path) -> PathBuf {
    ordinary(root);
    ordinary(path);
    let root = fs::canonicalize(root).unwrap();
    let path = fs::canonicalize(path).unwrap();
    assert!(
        path.starts_with(&root) && path != root,
        "fixture outside owner"
    );
    path
}
fn row_path(root: &Path, value: &Value) -> PathBuf {
    let relative = Path::new(string(value, "path"));
    assert!(
        relative
            .components()
            .all(|part| matches!(part, Component::Normal(_))),
        "fixture relative path"
    );
    descendant(root, &root.join(relative))
}
/// Lua 5.1 uses legacy C file APIs and appends slash-separated suffixes.
/// Keep canonical verbatim paths for Rust custody; convert only at the Lua
/// argument/framing boundary and prove it still names the authenticated input.
pub fn lua_path(path: &Path) -> Result<String, String> {
    ordinary(path);
    let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
    canonical.to_str().ok_or("Lua path is not Unicode")?;
    let ordinary = lycheedev_core::client::display_path(&canonical);
    if fs::canonicalize(&ordinary).map_err(|e| e.to_string())? != canonical {
        return Err("Lua path conversion changed fixture identity".into());
    }
    let text = ordinary.replace('\\', "/");
    if text.contains(['\r', '\n', '|']) {
        return Err("Lua path framing".into());
    }
    Ok(text)
}

pub fn write_new(path: &Path, bytes: &[u8]) {
    ordinary(path.parent().unwrap());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    lycheedev_windows::filesystem::close_file(file).unwrap();
}

/// Written by the existing formal-Lua controller after verifyInterpreter,
/// authenticate and materialize; every field is independently rechecked here.
/// These tests fail if admission is absent. They never silently skip Lua.
#[derive(Clone)]
pub struct Inputs {
    pub fixture: PathBuf,
    pub cases: PathBuf,
    pub lua: PathBuf,
    admission_path: PathBuf,
    admission_bytes: Vec<u8>,
    report_path: PathBuf,
    report_hash: String,
    receipt_path: PathBuf,
    receipt_hash: String,
    lua_hash: String,
}
impl Inputs {
    pub fn load() -> Self {
        let path = PathBuf::from(
            std::env::var_os("LYCHEEDEV_FORMAL_LUA_BRIDGE_ADMISSION")
                .expect("formal bridge requires the existing owned formal-Lua runner admission"),
        );
        let bytes = read(&path, 64 << 10);
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["fixture"]["oracle"], ORACLE);
        assert_eq!(value["fixture"]["manifestSHA256"], MANIFEST_SHA256);
        assert_eq!(
            value["fixture"]["releaseFieldsSHA256"],
            RELEASE_FIELDS_SHA256
        );
        let owner_path = PathBuf::from(string(&value["scratch"], "receipt"));
        let owner = json(&owner_path, 1 << 20);
        assert_eq!(owner["schema"], "lycheedev.disk-owner.v1");
        assert_eq!(owner["category"], "scratch");
        assert_eq!(owner["state"], "active");
        assert!(owner["activePid"].as_u64().is_some_and(|pid| pid > 0));
        let owner_root = PathBuf::from(string(&owner, "directory"));
        descendant(&owner_root, &path);
        let fixture = descendant(&owner_root, Path::new(string(&value["fixture"], "root")));
        let cases = descendant(&owner_root, Path::new(string(&value["scratch"], "cases")));
        assert!(!cases.starts_with(&fixture) && !fixture.starts_with(&cases));
        let report_path = PathBuf::from(string(&value["interpreter"], "report"));
        let report_bytes = read(&report_path, 1 << 20);
        let report_hash = string(&value["interpreter"], "reportSHA256").to_owned();
        assert_eq!(storage::sha256(&report_bytes), report_hash);
        let report: Value = serde_json::from_slice(&report_bytes).unwrap();
        assert_eq!(report["schema"], "lycheedev.formal-lua-interpreter.v1");
        assert_eq!(report["oracle"], ORACLE);
        assert_eq!(report["manifestSHA256"], MANIFEST_SHA256);
        assert_eq!(report["version"], "5.1.5");
        assert_eq!(report["platform"], "windows-amd64");
        assert_eq!(
            report["source"]["sha256"],
            "2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333"
        );
        assert_eq!(report["interpreterProbe"], "formal-lua-interpreter-ok");
        let receipt_path = PathBuf::from(string(&report, "buildEvidence"));
        let receipt_hash = string(&value["interpreter"], "buildReceiptSHA256").to_owned();
        let receipt_bytes = read(&receipt_path, 1 << 20);
        assert_eq!(storage::sha256(&receipt_bytes), receipt_hash);
        let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
        assert_eq!(receipt["schema"], "lycheedev.disk-owner.v1");
        assert_eq!(receipt["owner"], "formal-lua-build");
        assert_eq!(receipt["category"], "scratch");
        assert!(matches!(
            receipt["state"].as_str(),
            Some("succeeded" | "retired")
        ));
        assert_eq!(receipt["handlesReaped"], true);
        assert!(receipt["activePid"].is_null());
        // Full source/compiler/process custody belongs to the existing Node
        // verifyInterpreter. This narrow check binds its exact sealed report;
        // it intentionally does not reimplement that verifier's policy.
        let sealed = receipt_path
            .parent()
            .unwrap()
            .join("interpreter-report.json");
        assert_eq!(read(&sealed, 1 << 20), report_bytes);
        assert!(
            receipt["sealedLogs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["path"].as_str() == sealed.to_str() && row["sha256"] == report_hash)
        );
        let lua = PathBuf::from(string(&report, "executable"));
        let lua_hash = string(&report, "executableSHA256").to_owned();
        assert_eq!(storage::sha256(&read(&lua, 16 << 20)), lua_hash);
        let inputs = Self {
            fixture,
            cases,
            lua,
            admission_path: path,
            admission_bytes: bytes,
            report_path,
            report_hash,
            receipt_path,
            receipt_hash,
            lua_hash,
        };
        inputs.verify();
        inputs
    }
    pub fn verify(&self) {
        assert_eq!(read(&self.admission_path, 64 << 10), self.admission_bytes);
        assert_eq!(
            storage::sha256(&read(&self.report_path, 1 << 20)),
            self.report_hash
        );
        assert_eq!(
            storage::sha256(&read(&self.receipt_path, 1 << 20)),
            self.receipt_hash
        );
        assert_eq!(storage::sha256(&read(&self.lua, 16 << 20)), self.lua_hash);
        assert_eq!(storage::sha256(MANIFEST), MANIFEST_SHA256);
        let manifest: Value = serde_json::from_slice(MANIFEST).unwrap();
        assert_eq!(manifest["oracle"]["commit"], ORACLE);
        // The complete current Go manifest and exact two-field candidate
        // overlay are pinned separately. Historical manifests stay unchanged;
        // inspecting these JSON bytes never executes studied source.
        assert_eq!(storage::sha256(RELEASE_FIELDS), RELEASE_FIELDS_SHA256);
        let retained: Vec<Value> = serde_json::from_slice(RELEASE_FIELDS).unwrap();
        assert_eq!(retained.len(), 2);
        assert_eq!(retained[0]["path"], "addon/Core/Runtime.lua");
        assert_eq!(retained[1]["path"], "addon/Lychee Dev.toc");
        let mut count = 0;
        let mut total = 0;
        for key in ["fixtures", "candidateAddon"] {
            for row in manifest[key].as_array().unwrap() {
                let bytes = read(&row_path(&self.fixture, row), 2 << 20);
                total += bytes.len();
                count += 1;
                assert!(count <= 256 && total <= 16 << 20);
                if let Some(field) = retained.iter().find(|field| field["path"] == row["path"]) {
                    assert_eq!(field["expected"]["bytes"], row["bytes"]);
                    assert_eq!(field["expected"]["sha256"], row["sha256"]);
                    assert_eq!(
                        bytes.len() as u64,
                        field["retained"]["bytes"].as_u64().unwrap()
                    );
                    assert_eq!(storage::sha256(&bytes), field["retained"]["sha256"]);
                    let text = std::str::from_utf8(&bytes).unwrap();
                    let from = string(field, "retainedText");
                    assert_eq!(
                        text.matches(from).count(),
                        1,
                        "release substitution cardinality"
                    );
                    let restored = text.replacen(from, string(field, "expectedText"), 1);
                    assert_eq!(
                        storage::sha256(restored.as_bytes()),
                        row["sha256"],
                        "only approved release label may differ from Go"
                    );
                } else {
                    assert_eq!(bytes.len() as u64, row["bytes"].as_u64().unwrap());
                    assert_eq!(storage::sha256(&bytes), row["sha256"]);
                }
            }
        }
        assert_eq!(count, 113);
    }
    pub fn case(&self, name: &str) -> Case {
        assert!(matches!(
            name,
            "normal"
                | "bind"
                | "prepare"
                | "commit"
                | "confirm"
                | "release"
                | "reload_bind"
                | "compile_error"
                | "cleanup_error"
                | "memory"
                | "numeric"
        ));
        let root = self.cases.join(name);
        fs::create_dir(&root).expect("exclusive case directory, no stale fixture reuse");
        Case::new(root)
    }
    pub fn performance_case(&self, bytes: usize, events: usize, repeat: usize) -> Case {
        assert!(PERFORMANCE_RESULT_STRING_BYTES.contains(&bytes));
        assert!(PERFORMANCE_HISTORY_EVENTS.contains(&events));
        assert!(repeat < PERFORMANCE_REPEATS);
        let root = self
            .cases
            .join(format!("string-{bytes}-events-{events}-repeat-{repeat}"));
        fs::create_dir(&root).expect("exclusive performance fixture, no stale reuse");
        Case::new(root)
    }
}

pub struct Case {
    pub root: PathBuf,
    pub records: PathBuf,
    pub paths: ChannelPaths,
    pub addon_parent: PathBuf,
    pub owner: WindowOwner,
}
impl Case {
    fn new(root: PathBuf) -> Self {
        let records = root.join("peer");
        fs::create_dir(&records).unwrap();
        let addon_parent = root.join("addon-parent");
        fs::create_dir(&addon_parent).unwrap();
        let paths = ChannelPaths::new(&root, &format!("CON-{}", "a".repeat(32))).unwrap();
        let owner = WindowOwner {
            schema: "lycheedev.window-owner.v1".into(),
            workspace_id: paths.workspace_id().unwrap(),
            resource: "window/17/18/19".into(),
            operation_id: paths.connection_id().into(),
            intent_sha256: "b".repeat(64),
        };
        // Fixture-only initial admission, identical to existing durable-journal
        // tests. Every subsequent business transition uses the real journal.
        let state = State {
            schema: CHANNEL_SCHEMA.into(),
            id: paths.connection_id().into(),
            owner: "a".repeat(32),
            identity: Identity {
                schema: SCHEMA.into(),
                runtime: "1".repeat(32),
                next_slot: 1,
                slots: 500,
                guid: "Player-1-123".into(),
                character: "Tester".into(),
                realm: "Realm".into(),
                build: "70000".into(),
                product: "retail".into(),
                release: "2.5.1".into(),
                ..Identity::default()
            },
            connect_budget: Some(DurableBudget {
                started_at_ms: 1000,
                deadline_ms: 1000 + DEFAULT_CONNECT_MILLIS,
                last_observed_ms: 1000,
                ..DurableBudget::default()
            }),
            ..State::default()
        };
        state.validate().unwrap();
        fs::create_dir_all(paths.log_path().parent().unwrap()).unwrap();
        write_new(
            paths.log_path(),
            &prepare_event(&[], "created", &state).unwrap(),
        );
        let scope = addon_parent.join(".lycheedev-window-owners");
        fs::create_dir(&scope).unwrap();
        write_new(
            &scope.join(format!(
                "{}.json",
                storage::sha256(owner.resource.as_bytes())
            )),
            &serde_json::to_vec(&owner).unwrap(),
        );
        Self {
            root,
            records,
            paths,
            addon_parent,
            owner,
        }
    }
    pub fn open(&self, control: &OperationControl) -> DurableChannelJournal {
        DurableChannelJournal::acquire(
            self.paths.clone(),
            &self.addon_parent,
            self.owner.clone(),
            control,
        )
        .unwrap()
    }
}
#[derive(Clone, Copy)]
pub struct FixtureClock;
impl Clock for FixtureClock {
    fn unix_millis(&self) -> i64 {
        1000
    }
}
pub struct Tokens(pub u64);
impl TokenSource for Tokens {
    fn token(&mut self) -> ChannelResult<String> {
        self.0 += 1;
        Ok(format!("{:032x}", self.0))
    }
}
