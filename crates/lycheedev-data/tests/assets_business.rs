//! Product Assets chains, using task-owned synthetic inputs and external outputs.
//! No game installation, input, third-party encoder or network is exercised.
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
    evidence,
    selection::{self, DataPin, SelectionSpec},
    storage, workspace,
};
use lycheedev_data::assets::{listfile, source};
use lycheedev_data::{assets::{self, request::{DemuxInput, DemuxRequest, ExportRequest, SearchMode, SearchRequest}}, request::{DataContext, SourceOptions}};

use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-assets-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let home = root.join("home");
        workspace::initialize(&home).unwrap();
        Self { root, home }
    }
    fn context(&self) -> DataContext {
        DataContext { home: self.home.clone(), cwd: self.root.clone(), control: Default::default(), source: Default::default() }
    }
    fn pin(&self) -> String {
        selection::pin_selection(
            &self.home,
            SelectionSpec {
                data: Some(DataPin {
                    product: "retail".into(),
                    region: "cn".into(),
                    language: "zhCN".into(),
                    full_build: "12.1.0.69933".into(),
                    build_config: "a".repeat(32),
                    cdn_config: "b".repeat(32),
                    definition_commit: "c".repeat(40),
                }),
                ..Default::default()
            },
        )
        .unwrap()
        .id
    }
    fn verify_captures(&self, captures: &[Value]) {
        for capture in captures {
            let archived =
                evidence::read_capture(&self.home, capture["id"].as_str().unwrap()).unwrap();
            assert_eq!(&archived, capture);
            let bytes = evidence::read_blob_bounded(
                &self.home,
                &archived["blob"],
                1 << 20,
                &OperationControl::default(),
            )
            .unwrap();
            assert_eq!(storage::sha256(&bytes), archived["blob"]["sha256"]);
            assert_eq!(
                bytes.len() as u64,
                archived["blob"]["bytes"].as_u64().unwrap()
            );
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("retained failed Assets fixture: {}", self.root.display());
        } else {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
}
struct Fetch {
    bodies: BTreeMap<String, Vec<u8>>,
    calls: AtomicUsize,
}
impl source::Fetcher for Fetch {
    fn fetch(&self, url: &str, maximum: usize, control: &OperationControl) -> Result<Vec<u8>> {
        control.check()?;
        self.calls.fetch_add(1, Ordering::Relaxed);
        let bytes = self
            .bodies
            .get(url)
            .ok_or_else(|| ToolkitError::invalid("unrecognized fixture origin"))?;
        assert!(bytes.len() <= maximum);
        Ok(bytes.clone())
    }
}
fn fetch(kind: &str) -> Fetch {
    let text = b"1;Creature\\Wolf.M2\n2;A.blp\n1;Other.BLP\n3;A.blp\n4;\xc4\xb0\xce\xa3.BLP\n0;bad\ninvalid\n";
    let bodies = if kind == "wowexport-binary" {
        let strings = b"Creature\\Wolf.M2\0A.blp\0Other.BLP\0A.blp\0\xc4\xb0\xce\xa3.BLP\0";
        let offsets = [0u32, 17, 23, 33, 39];
        let mut index = Vec::new();
        for (id, offset) in [1u32, 2, 1, 3, 4].into_iter().zip(offsets) {
            index.extend_from_slice(&id.to_be_bytes());
            index.extend_from_slice(&offset.to_be_bytes());
            index.push(0);
        }
        listfile::COMPONENTS
            .iter()
            .enumerate()
            .map(|(n, component)| {
                (
                    listfile::source_url(kind, component).unwrap(),
                    if n == 0 {
                        index.clone()
                    } else if n == 1 {
                        strings.to_vec()
                    } else {
                        vec![]
                    },
                )
            })
            .collect()
    } else {
        BTreeMap::from([(
            listfile::source_url(kind, "listfile").unwrap(),
            text.to_vec(),
        )])
    };
    Fetch {
        bodies,
        calls: AtomicUsize::new(0),
    }
}
fn record(name: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = name.to_vec();
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend_from_slice(body);
    if body.len() & 1 != 0 {
        bytes.push(0);
    }
    bytes
}
fn avi() -> Vec<u8> {
    let mut frames = b"movi".to_vec();
    frames.extend(record(b"00dc", &[1, 2, 3, 4, 5, 0xc1, 2, 3, 0xc1]));
    frames.extend(record(b"JUNK", &[0]));
    frames.extend(record(b"00db", &[4, 5]));
    let mut body = b"AVI ".to_vec();
    body.extend(record(b"avih", &40000u32.to_le_bytes()));
    body.extend(record(b"LIST", &frames));
    record(b"RIFF", &body)
}

#[test]
fn search_uses_exact_authenticated_cache_for_all_sources_and_modes() {
    let fixture = Fixture::new();
    let pin = fixture.pin();
    for kind in ["community-csv", "wowexport-text", "wowexport-binary"] {
        let provider = fetch(kind);
        let prepared = source::prepare(
            &fixture.home,
            kind,
            false,
            false,
            listfile::Limits::default(),
            &Default::default(),
            &provider,
        )
        .unwrap();
        let calls = provider.calls.load(Ordering::Relaxed);
        assert_eq!(prepared.index.entries.len(), 5);
        assert_eq!(calls, if kind == "wowexport-binary" { 8 } else { 1 });
        let mut ctx = fixture.context();
        ctx.source = SourceOptions { snapshot: Some(pin.clone()), offline: true, ..Default::default() };
        let mut request = SearchRequest::new(kind.into(), SearchMode::Query("BLP".into()));
        request.limit = 1;
        let found = assets::search::run(&ctx, &request).unwrap();
        assert_eq!(found.data["total"], 4);
        assert_eq!(found.data["returned"], 1);
        assert_eq!(found.data["truncated"], true);
        assert_eq!(found.data["files"][0]["fileDataID"], 2);
        assert_eq!(found.data["context"]["listfile"]["cache"], "offline-cache");
        assert_eq!(
            found.warnings,
            vec![json!("File-name search was truncated by --limit.")]
        );
        request.mode = SearchMode::Extension(" .BLP ".into());
        request.limit = 2;
        let extension = assets::search::run(&ctx, &request).unwrap();
        assert_eq!(
            extension.data["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["fileDataID"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [2, 3]
        );
        request.mode = SearchMode::Name("CREATURE/wolf.mdx".into());
        let name = assets::search::run(&ctx, &request).unwrap();
        assert_eq!(name.data["matches"][0]["matchedVia"], "m2-alias");
        request.mode = SearchMode::Name("unknown.blp".into());
        let absent = assets::search::run(&ctx, &request).unwrap();
        assert_eq!(absent.data["resolved"], false);
        assert_eq!(absent.data["matches"], Value::Null);
        request.mode = SearchMode::FileId(1);
        let lookup = assets::search::run(&ctx, &request).unwrap();
        assert_eq!(
            lookup.data["entries"][0]["names"].as_array().unwrap().len(),
            2
        );
        request.mode = SearchMode::FileId(99);
        let absent = assets::search::run(&ctx, &request).unwrap();
        assert_eq!(
            absent.data["entries"],
            json!([{"fileDataID":99,"fileName":"unknown"}])
        );
        assert_eq!(provider.calls.load(Ordering::Relaxed), calls);
        let document: Value =
            lycheedev_core::metadata::DataCacheMetadata::read_controlled_with_check(
                &fixture.home,
                &format!("listfile/{kind}"),
                &Default::default(),
                || Ok(()),
            )
            .unwrap();
        let digest = document["provenance"]["files"][0]["blob"]["sha256"]
            .as_str()
            .unwrap();
        let path = fixture
            .home
            .join("blobs")
            .join(&digest[..2])
            .join(&digest[2..]);
        let original = fs::read(&path).unwrap();
        let mut altered = original.clone();
        altered[0] ^= 1;
        fs::write(&path, &altered).unwrap();
        assert!(assets::search::run(&ctx, &request).is_err());
        assert_eq!(fs::read(&path).unwrap(), altered);
        fs::write(path, original).unwrap();
    }
}

#[test]
fn demux_publishes_exact_frames_and_manifest_last_with_verified_captures() {
    let fixture = Fixture::new();
    let input = fixture.root.join("input.avi");
    fs::write(&input, avi()).unwrap();
    let output = fixture.root.join("output");
    fs::create_dir(&output).unwrap();
    let ctx = fixture.context();
    let mut request = DemuxRequest::new(DemuxInput::Path(input.clone()), output.clone());
    let result = assets::demux::run(&ctx, &request).unwrap();
    fixture.verify_captures(&result.provenance);
    assert_eq!(result.provenance.len(), 4);
    assert_eq!(result.data["frameCount"], 2);
    assert_eq!(result.data["frames"][0]["subFrames"], json!([2, 3]));
    assert_eq!(result.data["frames"][1]["timestamp"], 40000.0);
    assert_eq!(
        fs::read(output.join("frame-000001.00dc")).unwrap(),
        [1, 2, 3, 4, 5, 0xc1, 2, 3, 0xc1]
    );
    assert_eq!(fs::read(output.join("frame-000002.00db")).unwrap(), [4, 5]);
    let manifest = fs::read(output.join("demux-manifest.json")).unwrap();
    scoped_consumers::assert_manifest_value(&manifest, &result.data);
    fs::write(&input, b"broken input").unwrap();
    let conflict = assets::demux::run(&ctx, &request).unwrap_err();
    assert_eq!(conflict.code, "records.export_conflict");
    assert_eq!(
        fs::read(output.join("demux-manifest.json")).unwrap(),
        manifest
    );
    fs::write(&input, avi()).unwrap();
    request.overwrite = true;
    request.max_frames = 1;
    assert_eq!(
        assets::demux::run(&ctx, &request).unwrap_err().code,
        "records.frame_limit_exceeded"
    );
    let partial = fixture.root.join("partial");
    fs::create_dir(&partial).unwrap();
    request.output = partial.clone();
    request.allow_partial = true;
    let result = assets::demux::run(&ctx, &request).unwrap();
    fixture.verify_captures(&result.provenance);
    assert_eq!(result.data["frameCount"], 1);
    assert_eq!(result.data["enumeratedFrames"], 2);
    assert_eq!(result.data["complete"], false);
    assert_eq!(result.data["truncationReason"], "frame_limit");
    assert!(!partial.join("frame-000002.00db").exists());
}

#[test]
fn demux_all_name_conflicts_precede_any_external_frame_write_and_preserve_hardlinks() {
    let fixture = Fixture::new();
    let input = fixture.root.join("input.avi");
    fs::write(&input, avi()).unwrap();
    let output = fixture.root.join("output");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("frame-000002.00db"), b"previous").unwrap();
    let ctx = fixture.context();
    let mut request = DemuxRequest::new(DemuxInput::Path(input.clone()), output.clone());
    assert_eq!(
        assets::demux::run(&ctx, &request).unwrap_err().code,
        "records.export_conflict"
    );
    assert!(!output.join("frame-000001.00dc").exists());
    assert!(!output.join("demux-manifest.json").exists());
    let alias = fixture.root.join("alias");
    fs::hard_link(output.join("frame-000002.00db"), &alias).unwrap();
    let reader = OpenOptions::new()
        .read(true)
        .share_mode(1 | 2 | 4)
        .open(&alias)
        .unwrap();
    request.overwrite = true;
    assets::demux::run(&ctx, &request).unwrap();
    assert_eq!(fs::read(&alias).unwrap(), b"previous");
    assert_eq!(fs::read(output.join("frame-000002.00db")).unwrap(), [4, 5]);
    drop(reader);
    ctx.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(assets::demux::run(&ctx, &request).unwrap_err().exit_code, 7);
}

// Staged only; append to Assets integration target after Root releases edits.
#[test]
#[ignore = "explicit genuine synthetic CASC fixture required; no game installation"]
fn authenticated_local_casc_raw_export_captures_exact_bytes_and_preserves_conflicting_output() {
    let original = PathBuf::from(
        std::env::var("LYCHEEDEV_RUST_ASSET_CASC_FIXTURE")
            .expect("explicit synthetic fixture directory"),
    );
    let identity: Value =
        serde_json::from_slice(&fs::read(original.join("input-identity.json")).unwrap()).unwrap();
    assert_eq!(identity["schema"], "lycheedev.local-domain-fixture.v1");
    assert_eq!(
        identity["source"],
        "synthetic authenticated local installation; not game acceptance"
    );
    let fixture = Fixture::new();
    let pin = selection::pin_selection(
        &fixture.home,
        SelectionSpec {
            data: Some(serde_json::from_value(identity["data"].clone()).unwrap()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(pin.id, identity["snapshot"]);
    let external = fixture.root.join("original.bin");
    let installation = identity["installation"].as_str().unwrap();
    let mut ctx = fixture.context();
    ctx.source = SourceOptions { snapshot: Some(pin.id.clone()), installation: Some(installation.into()), offline: true, ..Default::default() };
    let mut request = ExportRequest::raw(303, external.clone());
    let result = assets::export::run(&ctx, &request).unwrap();
    fixture.verify_captures(&result.provenance);
    let raw = fs::read(&external).unwrap();
    assert_eq!(storage::sha256(&raw), identity["wdc"]["sha256"]);
    assert_eq!(raw.len() as u64, identity["wdc"]["bytes"].as_u64().unwrap());
    assert_eq!(result.data["content"], result.provenance[0]["blob"]);
    assert_eq!(result.data["sourceCapture"], result.provenance[0]);
    assert_eq!(result.provenance.len(), 2);
    let captured =
        evidence::read_blob(&fixture.home, &result.provenance[1]["blob"], 1 << 20).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&captured).unwrap(),
        result.data
    );
    ctx.source.installation = Some(fixture.root.join("missing-client"));
    assert_eq!(
        assets::export::run(&ctx, &request).unwrap_err().code,
        "records.export_conflict"
    );
    assert_eq!(fs::read(&external).unwrap(), raw);
    ctx.source.installation = Some(installation.into());
    let alias = fixture.root.join("original-alias.bin");
    fs::hard_link(&external, &alias).unwrap();
    request.overwrite = true;
    let repeated = assets::export::run(&ctx, &request).unwrap();
    fixture.verify_captures(&repeated.provenance);
    assert_eq!(fs::read(&alias).unwrap(), raw);
    assert_eq!(fs::read(&external).unwrap(), raw);
    ctx.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(assets::export::run(&ctx, &request).unwrap_err().exit_code, 7);
    assert_eq!(fs::read(&external).unwrap(), raw);
}

#[path = "support/assets_scoped.rs"]
mod scoped_consumers;
