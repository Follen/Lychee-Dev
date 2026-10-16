//! Product Assets chains, using task-owned synthetic inputs and external outputs.
//! No game installation, input, third-party encoder or network is exercised.
use lycheedev_core::{
    Invocation, Result, ToolkitError, control::OperationControl,
    evidence, metadata::Metadata, selection::{self, DataPin, SelectionSpec},
    storage, workspace,
};
use lycheedev_data::assets::{listfile, output, source};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap, fs::{self, File, OpenOptions}, io,
    os::windows::{ffi::OsStrExt, fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle},
    path::{Path, PathBuf}, sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture { root: PathBuf, home: PathBuf }
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lycheedev-assets-{}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let home = root.join("home");
        workspace::initialize(&home).unwrap();
        Self { root, home }
    }
    fn invocation(&self, command: &str, flags: &[(&str, &str)]) -> Invocation {
        Invocation { command: command.into(), positional: None,
            flags: flags.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect(),
            home: self.home.clone(), cwd: self.root.clone(), control: Default::default() }
    }
    fn pin(&self) -> String {
        selection::pin_selection(&self.home, SelectionSpec { data: Some(DataPin {
            product: "retail".into(), region: "cn".into(), language: "zhCN".into(), full_build: "12.1.0.69933".into(),
            build_config: "a".repeat(32), cdn_config: "b".repeat(32), definition_commit: "c".repeat(40),
        }), ..Default::default() }).unwrap().id
    }
    fn verify_captures(&self, captures: &[Value]) {
        for capture in captures {
            let archived = evidence::read_capture(&self.home, capture["id"].as_str().unwrap()).unwrap();
            assert_eq!(&archived, capture);
            let bytes = evidence::read_blob_bounded(&self.home, &archived["blob"], 1 << 20, &OperationControl::default()).unwrap();
            assert_eq!(storage::sha256(&bytes), archived["blob"]["sha256"]);
            assert_eq!(bytes.len() as u64, archived["blob"]["bytes"].as_u64().unwrap());
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() { eprintln!("retained failed Assets fixture: {}", self.root.display()); }
        else { fs::remove_dir_all(&self.root).unwrap(); }
    }
}
struct Fetch { bodies: BTreeMap<String, Vec<u8>>, calls: AtomicUsize }
impl source::Fetcher for Fetch {
    fn fetch(&self, url: &str, maximum: usize, control: &OperationControl) -> Result<Vec<u8>> {
        control.check()?; self.calls.fetch_add(1, Ordering::Relaxed);
        let bytes = self.bodies.get(url).ok_or_else(|| ToolkitError::invalid("unrecognized fixture origin"))?;
        assert!(bytes.len() <= maximum); Ok(bytes.clone())
    }
}
fn fetch(kind: &str) -> Fetch {
    let text = b"1;Creature\\Wolf.M2\n2;A.blp\n1;Other.BLP\n3;A.blp\n4;\xc4\xb0\xce\xa3.BLP\n0;bad\ninvalid\n";
    let bodies = if kind == "wowexport-binary" {
        let strings = b"Creature\\Wolf.M2\0A.blp\0Other.BLP\0A.blp\0\xc4\xb0\xce\xa3.BLP\0";
        let offsets = [0u32, 17, 23, 33, 39];
        let mut index = Vec::new();
        for (id, offset) in [1u32, 2, 1, 3, 4].into_iter().zip(offsets) {
            index.extend_from_slice(&id.to_be_bytes()); index.extend_from_slice(&offset.to_be_bytes()); index.push(0);
        }
        listfile::COMPONENTS.iter().enumerate().map(|(n, component)|
            (listfile::source_url(kind, component).unwrap(), if n == 0 { index.clone() } else if n == 1 { strings.to_vec() } else { vec![] })).collect()
    } else { BTreeMap::from([(listfile::source_url(kind, "listfile").unwrap(), text.to_vec())]) };
    Fetch { bodies, calls: AtomicUsize::new(0) }
}
fn record(name: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = name.to_vec(); bytes.extend_from_slice(&(body.len() as u32).to_le_bytes()); bytes.extend_from_slice(body);
    if body.len() & 1 != 0 { bytes.push(0); } bytes
}
fn avi() -> Vec<u8> {
    let mut frames = b"movi".to_vec(); frames.extend(record(b"00dc", &[1, 2, 3, 4, 5, 0xc1, 2, 3, 0xc1]));
    frames.extend(record(b"JUNK", &[0])); frames.extend(record(b"00db", &[4, 5]));
    let mut body = b"AVI ".to_vec(); body.extend(record(b"avih", &40000u32.to_le_bytes())); body.extend(record(b"LIST", &frames)); record(b"RIFF", &body)
}

#[test]
fn registered_search_uses_exact_authenticated_cache_for_all_sources_and_modes() {
    let fixture = Fixture::new(); let pin = fixture.pin();
    assert!(["asset search", "asset export", "asset demux"].iter().all(|command| lycheedev_data::commands().contains(command)));
    for kind in ["community-csv", "wowexport-text", "wowexport-binary"] {
        let provider = fetch(kind);
        let prepared = source::prepare(&fixture.home, kind, false, false, listfile::Limits::default(), &Default::default(), &provider).unwrap();
        let calls = provider.calls.load(Ordering::Relaxed);
        assert_eq!(prepared.index.entries.len(), 5);
        assert_eq!(calls, if kind == "wowexport-binary" { 8 } else { 1 });
        let mut inv = fixture.invocation("asset search", &[("--snapshot", &pin), ("--listfile", kind), ("--offline", "true"), ("--query", "BLP"), ("--limit", "1")]);
        let found = lycheedev_data::dispatch(&inv).unwrap();
        assert_eq!(found.data["total"], 4); assert_eq!(found.data["returned"], 1); assert_eq!(found.data["truncated"], true);
        assert_eq!(found.data["files"][0]["fileDataID"], 2); assert_eq!(found.data["context"]["listfile"]["cache"], "offline-cache");
        assert_eq!(found.warnings, vec![json!("File-name search was truncated by --limit.")]);
        inv.flags.remove("--query"); inv.flags.insert("--extension".into(), " .BLP ".into()); inv.flags.insert("--limit".into(), "2".into());
        let extension = lycheedev_data::dispatch(&inv).unwrap();
        assert_eq!(extension.data["files"].as_array().unwrap().iter().map(|row| row["fileDataID"].as_u64().unwrap()).collect::<Vec<_>>(), [2, 3]);
        inv.flags.remove("--extension"); inv.flags.insert("--name".into(), "CREATURE/wolf.mdx".into());
        let name = lycheedev_data::dispatch(&inv).unwrap(); assert_eq!(name.data["matches"][0]["matchedVia"], "m2-alias");
        inv.flags.insert("--name".into(), "unknown.blp".into()); let absent = lycheedev_data::dispatch(&inv).unwrap();
        assert_eq!(absent.data["resolved"], false); assert_eq!(absent.data["matches"], Value::Null);
        inv.flags.remove("--name"); inv.flags.insert("--file-id".into(), "1".into()); let lookup = lycheedev_data::dispatch(&inv).unwrap();
        assert_eq!(lookup.data["entries"][0]["names"].as_array().unwrap().len(), 2);
        inv.flags.insert("--file-id".into(), "99".into()); let absent = lycheedev_data::dispatch(&inv).unwrap();
        assert_eq!(absent.data["entries"], json!([{"fileDataID":99,"fileName":"unknown"}]));
        assert_eq!(provider.calls.load(Ordering::Relaxed), calls);
        let document: Value = Metadata::open(&fixture.home, false).unwrap().read(&format!("listfile/{kind}")).unwrap();
        let digest = document["provenance"]["files"][0]["blob"]["sha256"].as_str().unwrap();
        let path = fixture.home.join("blobs").join(&digest[..2]).join(&digest[2..]);
        let original = fs::read(&path).unwrap(); let mut altered = original.clone(); altered[0] ^= 1; fs::write(&path, &altered).unwrap();
        assert!(lycheedev_data::dispatch(&inv).is_err()); assert_eq!(fs::read(&path).unwrap(), altered);
        fs::write(path, original).unwrap();
    }
}

#[test]
fn registered_demux_publishes_exact_frames_and_manifest_last_with_verified_captures() {
    let fixture = Fixture::new(); let input = fixture.root.join("input.avi"); fs::write(&input, avi()).unwrap();
    let output = fixture.root.join("output"); fs::create_dir(&output).unwrap();
    let mut inv = fixture.invocation("asset demux", &[("--path", input.to_str().unwrap()), ("--output", output.to_str().unwrap())]);
    let result = lycheedev_data::dispatch(&inv).unwrap(); fixture.verify_captures(&result.provenance);
    assert_eq!(result.provenance.len(), 4); assert_eq!(result.data["frameCount"], 2); assert_eq!(result.data["frames"][0]["subFrames"], json!([2, 3]));
    assert_eq!(result.data["frames"][1]["timestamp"], 40000.0);
    assert_eq!(fs::read(output.join("frame-000001.00dc")).unwrap(), [1, 2, 3, 4, 5, 0xc1, 2, 3, 0xc1]);
    assert_eq!(fs::read(output.join("frame-000002.00db")).unwrap(), [4, 5]);
    let manifest = fs::read(output.join("demux-manifest.json")).unwrap(); scoped_consumers::assert_manifest_value(&manifest, &result.data);
    fs::write(&input, b"broken input").unwrap();
    let conflict = lycheedev_data::dispatch(&inv).unwrap_err(); assert_eq!(conflict.code, "records.export_conflict");
    assert_eq!(fs::read(output.join("demux-manifest.json")).unwrap(), manifest);
    fs::write(&input, avi()).unwrap(); inv.flags.insert("--overwrite".into(), "true".into()); inv.flags.insert("--max-frames".into(), "1".into());
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code, "records.frame_limit_exceeded");
    let partial = fixture.root.join("partial"); fs::create_dir(&partial).unwrap(); inv.flags.insert("--output".into(), partial.to_str().unwrap().into()); inv.flags.insert("--allow-partial".into(), "true".into());
    let result = lycheedev_data::dispatch(&inv).unwrap(); fixture.verify_captures(&result.provenance);
    assert_eq!(result.data["frameCount"], 1); assert_eq!(result.data["enumeratedFrames"], 2); assert_eq!(result.data["complete"], false); assert_eq!(result.data["truncationReason"], "frame_limit");
    assert!(!partial.join("frame-000002.00db").exists());
}

#[test]
fn demux_all_name_conflicts_precede_any_external_frame_write_and_preserve_hardlinks() {
    let fixture = Fixture::new(); let input = fixture.root.join("input.avi"); fs::write(&input, avi()).unwrap();
    let output = fixture.root.join("output"); fs::create_dir(&output).unwrap(); fs::write(output.join("frame-000002.00db"), b"previous").unwrap();
    let mut inv = fixture.invocation("asset demux", &[("--path", input.to_str().unwrap()), ("--output", output.to_str().unwrap())]);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code, "records.export_conflict");
    assert!(!output.join("frame-000001.00dc").exists()); assert!(!output.join("demux-manifest.json").exists());
    let alias = fixture.root.join("alias"); fs::hard_link(output.join("frame-000002.00db"), &alias).unwrap();
    let reader = OpenOptions::new().read(true).share_mode(1 | 2 | 4).open(&alias).unwrap();
    inv.flags.insert("--overwrite".into(), "true".into()); lycheedev_data::dispatch(&inv).unwrap();
    assert_eq!(fs::read(&alias).unwrap(), b"previous"); assert_eq!(fs::read(output.join("frame-000002.00db")).unwrap(), [4, 5]); drop(reader);
    inv.control.cancel_handle().store(true, Ordering::Release); assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code, 7);
}

// A real Windows junction writer, without a fixture child or a new dependency.
#[link(name = "kernel32")]
unsafe extern "system" {
    fn DeviceIoControl(handle: *mut std::ffi::c_void, code: u32, input: *const std::ffi::c_void, input_bytes: u32,
        output: *mut std::ffi::c_void, output_bytes: u32, returned: *mut u32, overlapped: *mut std::ffi::c_void) -> i32;
}
fn reparse_writer(path: &Path) -> io::Result<File> { OpenOptions::new().write(true).share_mode(7).custom_flags(0x02200000).open(path) }
fn junction(file: &File, target: &Path) -> io::Result<()> {
    let absolute = std::path::absolute(target)?; let print: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let substitute: Vec<u16> = std::ffi::OsString::from(format!("\\??\\{}", absolute.display())).encode_wide().collect();
    let size = 16 + (substitute.len() + print.len() + 2) * 2; let mut bytes = vec![0u8; size];
    bytes[..4].copy_from_slice(&0xa0000003u32.to_le_bytes()); bytes[4..6].copy_from_slice(&((size-8) as u16).to_le_bytes());
    bytes[10..12].copy_from_slice(&((substitute.len()*2) as u16).to_le_bytes()); bytes[12..14].copy_from_slice(&((substitute.len()*2+2) as u16).to_le_bytes()); bytes[14..16].copy_from_slice(&((print.len()*2) as u16).to_le_bytes());
    let mut at = 16; for unit in substitute.into_iter().chain(Some(0)).chain(print).chain(Some(0)) { bytes[at..at+2].copy_from_slice(&unit.to_le_bytes()); at += 2; }
    let mut returned = 0;
    if unsafe { DeviceIoControl(file.as_raw_handle(), 0x000900a4, bytes.as_ptr().cast(), size as u32, std::ptr::null_mut(), 0, &mut returned, std::ptr::null_mut()) } == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

#[test]
fn product_output_directory_pins_parent_and_refuses_nonempty_ancestor_reparse() {
    let fixture = Fixture::new(); let ancestor = fixture.root.join("ancestor"); let parent = ancestor.join("parent"); fs::create_dir_all(&parent).unwrap();
    let outside = fixture.root.join("outside"); fs::create_dir(&outside).unwrap(); fs::create_dir(outside.join("parent")).unwrap();
    fs::write(outside.join("parent").join("body"), b"outside original").unwrap();
    let inv = fixture.invocation("asset demux", &[]);
    let selected = output::OutputDirectory::open(&fixture.home, &parent, true).unwrap(); selected.preflight("body").unwrap();
    assert!(reparse_writer(&parent).is_err()); assert!(fs::rename(&parent, ancestor.join("moved")).is_err());
    // Prove the real junction payload/writer works on an empty ordinary directory.
    // The held parent below is an existing child of ancestor, so that ancestor
    // cannot be made empty by renaming/deleting the child while its handle lives.
    let baseline = fixture.root.join("empty-junction-baseline"); fs::create_dir(&baseline).unwrap();
    junction(&reparse_writer(&baseline).unwrap(), &outside).unwrap();
    assert_ne!(fs::symlink_metadata(&baseline).unwrap().file_attributes() & 0x400, 0);
    assert_eq!(fs::read(baseline.join("parent").join("body")).unwrap(), b"outside original");
    let mutation = junction(&reparse_writer(&ancestor).unwrap(), &outside).unwrap_err();
    assert_eq!(mutation.raw_os_error(), Some(145)); // ERROR_DIR_NOT_EMPTY, not an installed junction.
    assert_eq!(fs::symlink_metadata(&ancestor).unwrap().file_attributes() & 0x400, 0);
    selected.preflight("body").unwrap(); selected.publish(&inv, "body", b"selected parent bytes", 64).unwrap();
    assert_eq!(fs::read(parent.join("body")).unwrap(), b"selected parent bytes");
    assert_eq!(fs::read(outside.join("parent").join("body")).unwrap(), b"outside original"); assert_eq!(fs::read_dir(outside.join("parent")).unwrap().count(), 1);
    drop(selected); fs::remove_dir(&baseline).unwrap();
}
// Staged only; append to Assets integration target after Root releases edits.
#[test]
#[ignore = "explicit genuine synthetic CASC fixture required; no game installation"]
fn authenticated_local_casc_raw_export_captures_exact_bytes_and_preserves_conflicting_output() {
    let original = PathBuf::from(std::env::var("LYCHEEDEV_RUST_ASSET_CASC_FIXTURE").expect("explicit synthetic fixture directory"));
    let identity: Value = serde_json::from_slice(&fs::read(original.join("input-identity.json")).unwrap()).unwrap();
    assert_eq!(identity["schema"], "lycheedev.local-domain-fixture.v1");
    assert_eq!(identity["source"], "synthetic authenticated local installation; not game acceptance");
    let fixture = Fixture::new();
    let pin = selection::pin_selection(&fixture.home, SelectionSpec { data: Some(serde_json::from_value(identity["data"].clone()).unwrap()), ..Default::default() }).unwrap();
    assert_eq!(pin.id, identity["snapshot"]);
    let external = fixture.root.join("original.bin");
    let installation = identity["installation"].as_str().unwrap();
    let mut inv = fixture.invocation("asset export", &[("--snapshot", &pin.id), ("--installation", installation), ("--file-id", "303"), ("--output", external.to_str().unwrap()), ("--offline", "true")]);
    let result = lycheedev_data::dispatch(&inv).unwrap();
    fixture.verify_captures(&result.provenance);
    let raw = fs::read(&external).unwrap();
    assert_eq!(storage::sha256(&raw), identity["wdc"]["sha256"]);
    assert_eq!(raw.len() as u64, identity["wdc"]["bytes"].as_u64().unwrap());
    assert_eq!(result.data["content"], result.provenance[0]["blob"]);
    assert_eq!(result.data["sourceCapture"], result.provenance[0]);
    assert_eq!(result.provenance.len(), 2);
    let captured = evidence::read_blob(&fixture.home, &result.provenance[1]["blob"], 1 << 20).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&captured).unwrap(), result.data);
    inv.flags.insert("--installation".into(), fixture.root.join("missing-client").to_string_lossy().into_owned());
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code, "records.export_conflict");
    assert_eq!(fs::read(&external).unwrap(), raw);
    inv.flags.insert("--installation".into(), installation.into());
    let alias = fixture.root.join("original-alias.bin");
    fs::hard_link(&external, &alias).unwrap();
    inv.flags.insert("--overwrite".into(), "true".into());
    let repeated = lycheedev_data::dispatch(&inv).unwrap();
    fixture.verify_captures(&repeated.provenance);
    assert_eq!(fs::read(&alias).unwrap(), raw);
    assert_eq!(fs::read(&external).unwrap(), raw);
    inv.control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code, 7);
    assert_eq!(fs::read(&external).unwrap(), raw);
}


#[path = "support/assets_scoped.rs"]
mod scoped_consumers;
