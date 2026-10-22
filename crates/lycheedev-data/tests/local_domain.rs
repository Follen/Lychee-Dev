//! Genuine bounded local CASC/Encoding/Root/WDC/DBD input. No business provider
//! is replaced: the same installation and cached definitions can be used by
//! the frozen Go CLI for independent public differential tests.
#[path = "support/data_cache.rs"]
mod data_cache;
use lycheedev_core::{
    evidence,
    selection::{self, SelectionSpec},
    storage, workspace,
};
use lycheedev_data::{
    agent_session::{Session, Task},
    casc,
    engine::{DomainRequest, SourceMode},
    installed,
};
use md5::{Digest, Md5};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn auras(spell_id: u32) -> DomainRequest {
    DomainRequest::SpellAuras {
        spell_id,
        mode: SourceMode::Static,
        hotfix_captures: vec![],
    }
}
fn domain(
    inv: &lycheedev_data::request::DataContext,
    request: DomainRequest,
) -> lycheedev_core::Result<lycheedev_core::Outcome> {
    let mut session = Session::new(inv.clone(), Default::default())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let response =
        runtime.block_on(session.execute(Task::Navigate { request }, inv.control.clone()));
    let result = response.map(|response| {
        let value = serde_json::to_value(response).unwrap();
        assert!(!value["sources"].as_array().unwrap().is_empty());
        let mut result = lycheedev_core::Outcome::new(value["report"].clone());
        result.complete = value["report"]["complete"].as_bool().unwrap();
        result.provenance.push(value["capture"].clone());
        result
    });
    let closed = session.close();
    lycheedev_core::metadata::owned_write::finish_owned(result, closed)
}

const BUILD: &str = "12.1.0.69933";
const DEFS: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TABLE_HASH: u32 = 0xf00d0001;
const LAYOUT_HASH: u32 = 0xabcd0001;
const FDID: u32 = 303;

fn md5(raw: &[u8]) -> [u8; 16] {
    Md5::digest(raw).into()
}
fn write(root: &Path, path: impl AsRef<Path>, raw: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, raw).unwrap();
}
fn frame(raw: &[u8]) -> Vec<u8> {
    let mut out = b"BLTE\0\0\0\0N".to_vec();
    out.extend(raw);
    out
}
fn framed(blocks: &[&[u8]]) -> Vec<u8> {
    let header = 12 + blocks.len() * 24;
    let mut out = vec![0; header];
    out[..4].copy_from_slice(b"BLTE");
    out[4..8].copy_from_slice(&(header as u32).to_be_bytes());
    out[8] = 15;
    out[11] = blocks.len() as u8;
    for (i, payload) in blocks.iter().enumerate() {
        let at = 12 + i * 24;
        let mut chunk = vec![b'N'];
        chunk.extend(*payload);
        out[at..at + 4].copy_from_slice(&(chunk.len() as u32).to_be_bytes());
        out[at + 4..at + 8].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        out[at + 8..at + 24].copy_from_slice(&md5(&chunk));
        out.extend(chunk);
    }
    out
}
fn ekey(raw: &[u8]) -> [u8; 16] {
    let extent = u32::from_be_bytes(raw[4..8].try_into().unwrap()) as usize;
    md5(if extent == 0 { raw } else { &raw[..extent] })
}

fn spell_effect() -> (Vec<u8>, String) {
    // ID, SpellID, EffectIndex, Effect, EffectMiscValue[2], EffectBasePointsF.
    let widths = [4, 4, 4, 4, 8, 4];
    let rows = [
        (1u32, 100u32, 0u32, 64i32, [7922i32, 99i32], -230.989f32),
        (2, 100, 1, 6, [0, 0], -1000.5),
        (3, 300, 2, 28, [5000, 9000], 0.),
        (4, 300, 3, 28, [5001, 5000], -0.),
        (5, 300, 4, 28, [0, 5000], 1.),
        (6, 300, 5, 2, [5000, 0], 2.),
    ];
    let metadata = 72 + 40 + 4 * widths.len() + 24 * widths.len();
    let stride: usize = widths.iter().sum();
    let mut raw = vec![0; metadata];
    raw[..4].copy_from_slice(b"WDC3");
    for (at, word) in [
        (0, rows.len() as u32),
        (4, widths.len() as u32),
        (8, stride as u32),
        (16, TABLE_HASH),
        (20, LAYOUT_HASH),
        (24, 1),
        (28, rows.len() as u32),
        (40, widths.len() as u32),
        (52, (24 * widths.len()) as u32),
        (64, 1),
    ] {
        raw[4 + at..8 + at].copy_from_slice(&word.to_le_bytes());
    }
    raw[80..84].copy_from_slice(&(metadata as u32).to_le_bytes());
    raw[84..88].copy_from_slice(&(rows.len() as u32).to_le_bytes());
    let mut offset = 0;
    for (i, width) in widths.iter().enumerate() {
        let placement = 112 + i * 4;
        raw[placement + 2..placement + 4].copy_from_slice(&(offset as u16).to_le_bytes());
        let codec = 112 + widths.len() * 4 + i * 24;
        raw[codec..codec + 2].copy_from_slice(&((offset * 8) as u16).to_le_bytes());
        raw[codec + 2..codec + 4].copy_from_slice(&((width * 8) as u16).to_le_bytes());
        offset += width;
    }
    for (id, spell, index, effect, misc, points) in rows {
        raw.extend(id.to_le_bytes());
        raw.extend(spell.to_le_bytes());
        raw.extend(index.to_le_bytes());
        raw.extend(effect.to_le_bytes());
        for item in misc {
            raw.extend(item.to_le_bytes());
        }
        raw.extend(points.to_bits().to_le_bytes());
    }
    let dbd = format!(
        "COLUMNS\nint ID\nint SpellID\nint EffectIndex\nint Effect\nint EffectMiscValue\nfloat EffectBasePointsF\n\nBUILD {BUILD}\nLAYOUT {LAYOUT_HASH:08X}\n$id$ID<u32>\nSpellID<u32>\nEffectIndex<u32>\nEffect<32>\nEffectMiscValue<32>[2]\nEffectBasePointsF<32>\n"
    );
    (raw, dbd)
}

struct Object {
    ckey: [u8; 16],
    ekey: [u8; 16],
    decoded: usize,
    raw: Vec<u8>,
}
fn object(raw: &[u8]) -> Object {
    let encoded = frame(raw);
    Object {
        ckey: md5(raw),
        ekey: ekey(&encoded),
        decoded: raw.len(),
        raw: encoded,
    }
}
fn encoding(entries: &[Object]) -> Vec<u8> {
    assert!(entries.len() < 26);
    let mut ck = entries.iter().collect::<Vec<_>>();
    ck.sort_by_key(|o| o.ckey);
    let mut ek = entries.iter().collect::<Vec<_>>();
    ek.sort_by_key(|o| o.ekey);
    let mut ck_page = vec![0; 1024];
    let mut ek_page = vec![0; 1024];
    let size = |raw: &mut [u8], n: usize| {
        raw.copy_from_slice(&(n as u64).to_be_bytes()[3..]);
    };
    for (i, o) in ck.iter().enumerate() {
        let at = i * 38;
        ck_page[at] = 1;
        size(&mut ck_page[at + 1..at + 6], o.decoded);
        ck_page[at + 6..at + 22].copy_from_slice(&o.ckey);
        ck_page[at + 22..at + 38].copy_from_slice(&o.ekey);
    }
    for (i, o) in ek.iter().enumerate() {
        let at = i * 25;
        ek_page[at..at + 16].copy_from_slice(&o.ekey);
        size(&mut ek_page[at + 20..at + 25], o.raw.len());
    }
    let mut raw = vec![0; 22];
    raw[..5].copy_from_slice(b"EN\x01\x10\x10");
    raw[5..7].copy_from_slice(&1u16.to_be_bytes());
    raw[7..9].copy_from_slice(&1u16.to_be_bytes());
    raw[9..13].copy_from_slice(&1u32.to_be_bytes());
    raw[13..17].copy_from_slice(&1u32.to_be_bytes());
    raw.extend(ck[0].ckey);
    raw.extend(md5(&ck_page));
    raw.extend(ck_page);
    raw.extend(ek[0].ekey);
    raw.extend(md5(&ek_page));
    raw.extend(ek_page);
    raw
}
fn archive(root: &Path, entries: &[Object]) {
    let mut bytes = vec![];
    let mut spans = vec![];
    for o in entries {
        spans.push((o.ekey, bytes.len() as u32, (o.raw.len() + 30) as u32));
        bytes.extend([0; 30]);
        bytes.extend(&o.raw);
    }
    spans.sort_by_key(|s| s.0);
    let mut index = vec![0; 40 + spans.len() * 18];
    index[..4].copy_from_slice(&16u32.to_le_bytes());
    index[8..16].copy_from_slice(&[7, 0, 0, 0, 4, 5, 9, 30]);
    index[16..24].copy_from_slice(&(1u64 << 30).to_le_bytes());
    let guard = casc::guard_sum(&index[8..24], 0, 0).0;
    index[4..8].copy_from_slice(&guard.to_le_bytes());
    index[32..36].copy_from_slice(&((spans.len() * 18) as u32).to_le_bytes());
    let (mut first, mut second) = (0, 0);
    for (i, (key, offset, length)) in spans.into_iter().enumerate() {
        let at = 40 + i * 18;
        index[at..at + 9].copy_from_slice(&key[..9]);
        index[at + 10..at + 14].copy_from_slice(&offset.to_be_bytes());
        index[at + 14..at + 18].copy_from_slice(&length.to_le_bytes());
        (first, second) = casc::guard_sum(&index[at..at + 18], first, second);
    }
    index[36..40].copy_from_slice(&first.to_le_bytes());
    write(root, "Data/data/data.000", &bytes);
    write(root, "Data/data/0000000001.idx", &index);
}

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    client: PathBuf,
    pin: String,
}
impl Fixture {
    fn create(root: PathBuf) -> Self {
        Self::create_profile(root, false)
    }
    fn create_profile(root: PathBuf, missing_key: bool) -> Self {
        assert!(!root.exists(), "fixture must have a fresh owner root");
        let common = root.join("game");
        let client = common.join("非目录身份 client");
        let home = root.join("home");
        let (wdc, dbd) = spell_effect();
        let mut table = object(&wdc);
        if missing_key {
            let mut payload = vec![b'E', 8];
            payload.extend(42u64.to_le_bytes());
            payload.push(4);
            payload.extend([0; 4]);
            payload.push(b'S');
            payload.extend([0; 6]);
            let mut encoded = b"BLTE".to_vec();
            encoded.extend(36u32.to_be_bytes());
            encoded.extend([15, 0, 0, 1]);
            encoded.extend((payload.len() as u32).to_be_bytes());
            encoded.extend((wdc.len() as u32).to_be_bytes());
            encoded.extend(md5(&payload));
            encoded.extend(payload);
            table.ekey = ekey(&encoded);
            table.raw = encoded;
        }
        let mut root_raw = b"TSFM".to_vec();
        for word in [1u32, 0, 1, 0x10000000, 0x40, FDID] {
            root_raw.extend(word.to_le_bytes());
        }
        root_raw.extend(table.ckey);
        let root_object = object(&root_raw);
        let root_key = casc::hex(&root_object.ckey);
        let mut entries = vec![table, root_object];
        let logical = encoding(&entries);
        let encoded = framed(&[&logical[..54], &logical[54..]]);
        let encoding_key = casc::hex(&ekey(&encoded));
        let build = format!(
            "build-uid = wow\nroot = {root_key}\nencoding = {} {encoding_key}\nencoding-size = {} {}\n",
            casc::hex(&md5(&logical)),
            logical.len(),
            encoded.len()
        );
        let cdn = b"archives =\n";
        let build_key = casc::hex(&md5(build.as_bytes()));
        let cdn_key = casc::hex(&md5(cdn));
        entries.push(Object {
            ckey: md5(&logical),
            ekey: ekey(&encoded),
            decoded: logical.len(),
            raw: encoded,
        });
        archive(&common, &entries);
        for (key, raw) in [(&build_key, build.as_bytes()), (&cdn_key, cdn.as_slice())] {
            write(
                &common,
                format!("Data/config/{}/{}/{key}", &key[..2], &key[2..4]),
                raw,
            );
        }
        write(&common,".build.info",format!("Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|{BUILD}|{build_key}|{cdn_key}|1\n").as_bytes());
        write(&client, ".flavor.info", b"Product Flavor!STRING:0\nwow\n");
        write(&client, "version.txt", BUILD.as_bytes());
        workspace::initialize(&home).unwrap();
        let resolved = installed::resolve_target(&installed::LocalTargetRequest {
            installation: client.clone(),
            product: Some("retail".into()),
            full_build: Some(BUILD.into()),
            region: "cn".into(),
            language: "zhCN".into(),
            definition_commit: DEFS.into(),
        })
        .unwrap();
        let pin = selection::pin_selection(
            &home,
            SelectionSpec {
                data: Some(resolved.data),
                ..Default::default()
            },
        )
        .unwrap();
        let base = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{DEFS}/");
        let manifest = json!([{"tableName":"SpellEffect","tableHash":format!("{TABLE_HASH:08X}"),"db2FileDataID":FDID}]);
        let meta = data_cache::Cache::new(&home);
        for (suffix, raw) in [
            ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
            ("definitions/SpellEffect.dbd", dbd.into_bytes()),
        ] {
            let url = format!("{base}{suffix}");
            let blob = evidence::publish_blob(&home, &raw, 4 << 20).unwrap();
            meta.put(
                &format!("definition/{}", storage::sha256(url.as_bytes())),
                &json!({"url":url,"blob":blob}),
                None,
            )
            .unwrap();
        }
        let identity = json!({"schema":"lycheedev.local-domain-fixture.v1","source":"synthetic authenticated local installation; not game acceptance","home":home,"installation":client,"snapshot":pin.id,"data":pin.data,"wdc":{"bytes":wdc.len(),"sha256":storage::sha256(&wdc),"contentKey":casc::hex(&md5(&wdc))},"encoding":{"wholeDecodedCKeyClaim":false,"requestedPagesMD5":true},"expectedSpellIDs":[100,300,999]});
        write(
            &root,
            "input-identity.json",
            &serde_json::to_vec_pretty(&identity).unwrap(),
        );
        Self {
            root,
            home,
            client,
            pin: pin.id,
        }
    }
    fn context(&self) -> lycheedev_data::request::DataContext {
        lycheedev_data::request::DataContext {
            source: lycheedev_data::request::SourceOptions {
                snapshot: Some(self.pin.clone()),
                installation: Some(self.client.clone()),
                offline: true,
                ..Default::default()
            },
            home: self.home.clone(),
            cwd: self.root.clone(),
            control: Default::default(),
        }
    }
}

#[test]
#[ignore = "prepare actual encrypted CASC fixture and fixed authenticated public-key document"]
fn prepare_authenticated_missing_key_fixture() {
    use lycheedev_data::keys;
    let root = PathBuf::from(
        std::env::var_os("LYCHEEDEV_LOCAL_KEY_FIXTURE").expect("fresh owned fixture path"),
    );
    let document = PathBuf::from(
        std::env::var_os("LYCHEEDEV_PUBLIC_KEY_DOCUMENT").expect("fixed public-key document"),
    );
    let raw = fs::read(document).unwrap();
    assert_eq!(raw.len(), keys::PUBLIC_BYTES);
    assert_eq!(storage::sha256(&raw), keys::PUBLIC_SHA256);
    let parsed = keys::KeySet::parse(&raw, "text", keys::PUBLIC_SHA256).unwrap();
    assert!(!parsed.keys().contains_key(&42));
    let fixture = Fixture::create_profile(root, true);
    let blob = evidence::publish_blob(&fixture.home, &raw, 4 << 20).unwrap();
    assert_eq!(blob["sha256"], keys::PUBLIC_SHA256);
    write(&fixture.root, "public-keys.txt", &raw);
}

#[test]
fn authenticated_local_spell_navigation_success_and_corrupt_original_rejection() {
    let retained = std::env::var_os("LYCHEEDEV_LOCAL_DOMAIN_FIXTURE").map(PathBuf::from);
    let root = retained.clone().unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/rust-local-domain-fixture")
            .join(format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
    });
    let fx = Fixture::create(root);
    let mut results = vec![];
    for (spell, npc, has_aura, count) in [
        (100, None, Some(true), None),
        (300, None, Some(false), None),
        (999, None, Some(false), None),
        (300, None, None, Some(3)),
        (300, Some(5000), None, Some(1)),
        (300, Some(9000), None, Some(0)),
    ] {
        let request = if has_aura.is_some() {
            auras(spell)
        } else {
            DomainRequest::SpellSummons {
                spell_id: spell,
                npc_id: npc.unwrap_or(0),
                mode: SourceMode::Static,
                hotfix_captures: vec![],
            }
        };
        let task = serde_json::to_value(&request).unwrap();
        let result = domain(&fx.context(), request).unwrap();
        assert!(result.complete);
        assert_eq!(result.data["complete"], true);
        assert_eq!(result.data["partial"], false);
        if let Some(expected) = has_aura {
            assert_eq!(
                result.data["hasAura"],
                if expected { json!([spell]) } else { json!([]) }
            );
        }
        if let Some(expected) = count {
            assert_eq!(result.data["count"], expected);
        }
        results.push(json!({"request":task,"spellID":spell,"npcID":npc,"data":result.data,"capture":result.provenance}));
    }
    // A forged original framing is rejected even when a decoded cache exists.
    let path = fx.root.join("game/Data/data/data.000");
    let original = fs::read(&path).unwrap();
    let mut changed = original.clone();
    changed[39] ^= 1;
    fs::write(&path, changed).unwrap();
    let rejected = domain(&fx.context(), auras(100));
    // Restore the authenticated input before asserting so a retained fixture
    // remains usable by the independent frozen-Go public comparison on failure.
    fs::write(path, original).unwrap();
    let error = rejected.unwrap_err();
    assert_eq!(error.code, "archive.index_integrity");
    write(&fx.root,"sdk-report.json",&serde_json::to_vec_pretty(&json!({"passed":true,"scope":"six full-source local authenticated domain results and corrupted-original rejection; no partial/CDN/game or performance acceptance","results":results,"corruptedOriginal":{"code":error.code,"exitCode":error.exit_code,"restored":true}})).unwrap());
    if retained.is_none() {
        fs::remove_dir_all(fx.root).unwrap();
    }
}

#[test]
fn pinned_navigation_accepts_casc_root_and_recovers_client_conflicts() {
    // Go file_source.go uses dataInstallationRoot for existing pins. This
    // exercises the current Agent Session and Engine-backed domain access.
    let root = std::env::temp_dir().join(format!(
        "lychee-pinned-domain-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fixture = Fixture::create(root);
    let common = fixture.client.parent().unwrap();
    let exact = domain(&fixture.context(), auras(100)).unwrap();
    let mut from_root = fixture.context();
    from_root.source.installation = Some(common.to_owned());
    let root_result = domain(&from_root, auras(100)).unwrap();
    assert_eq!(root_result.data["hasAura"], exact.data["hasAura"]);
    std::fs::write(fixture.client.join("version.txt"), "12.1.0.69934").unwrap();
    let error = domain(&fixture.context(), auras(100)).unwrap_err();
    assert_eq!(error.code, "records.pinned_build_changed");
    std::fs::write(fixture.client.join("version.txt"), BUILD).unwrap();
    assert!(domain(&fixture.context(), auras(100)).unwrap().complete);
    // Read a fixed CASC pin with no installed client. Target preparation must
    // still refuse to invent one from catalog rows.
    std::fs::remove_dir_all(&fixture.client).unwrap();
    assert_eq!(
        installed::resolve_target(&installed::LocalTargetRequest {
            installation: common.to_owned(),
            product: Some("retail".into()),
            full_build: Some(BUILD.into()),
            region: "cn".into(),
            language: "zhCN".into(),
            definition_commit: DEFS.into(),
        })
        .unwrap_err()
        .code,
        "selection.installation_missing"
    );
    let catalog_only = domain(&from_root, auras(100)).unwrap();
    assert!(catalog_only.complete);
    assert_eq!(catalog_only.data["hasAura"], exact.data["hasAura"]);
    std::fs::remove_dir_all(fixture.root).unwrap();
}
