//! Small real CASC/Encoding/Root/DBD/WDC inputs for item/decor navigation.
//! They are synthetic authenticated sources, never actual-client acceptance.
#[path = "data_cache.rs"]
mod data_cache;
use lycheedev_core::{
    Invocation, evidence,
    selection::{self, SelectionSpec},
    storage, workspace,
};
use lycheedev_data::{casc, installed};
use md5::{Digest, Md5};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const BUILD: &str = "12.1.0.69933";
pub const DEFS: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Clone)]
pub enum Cell {
    U(u32),
    I(i32),
    #[allow(dead_code)]
    I64(i64),
    F(f32),
    Text(String),
    BadString,
    #[allow(dead_code)]
    Array(Vec<Cell>),
}
pub struct Table {
    pub name: &'static str,
    pub id: u32,
    pub hash: u32,
    pub layout: u32,
    pub raw: Vec<u8>,
    pub dbd: String,
}
fn put(raw: &mut [u8], at: usize, value: u32) {
    raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub fn table(name: &'static str, id: u32, fields: &[(&str, &str)], rows: &[Vec<Cell>]) -> Table {
    let hash = 0xf00d0000 + id;
    let layout = 0xabcd0000 + id;
    let count = fields.len();
    let widths: Vec<_> = fields
        .iter()
        .map(|(_, kind)| match *kind {
            "array3" => 3,
            "array2signed" | "array2float" => 2,
            _ => 1,
        })
        .collect();
    let sizes: Vec<_> = widths
        .iter()
        .zip(fields)
        .map(|(width, (_, kind))| width * if *kind == "signed64" { 8 } else { 4 })
        .collect();
    let offsets: Vec<_> = sizes
        .iter()
        .scan(0, |at, size| {
            let old = *at;
            *at += size;
            Some(old)
        })
        .collect();
    let stride = sizes.iter().sum::<usize>();
    let metadata = 112 + count * 28;
    let mut raw = vec![0; metadata + rows.len() * stride];
    raw[..4].copy_from_slice(b"WDC3");
    let ids: Vec<_> = rows
        .iter()
        .map(|row| match row[0] {
            Cell::U(id) => id,
            _ => panic!("fixture ID"),
        })
        .collect();
    for (at, value) in [
        (4, rows.len() as u32),
        (8, count as u32),
        (12, stride as u32),
        (20, hash),
        (24, layout),
        (28, *ids.iter().min().unwrap_or(&0)),
        (32, *ids.iter().max().unwrap_or(&0)),
        (44, count as u32),
        (56, (count * 24) as u32),
        (68, 1),
        (80, metadata as u32),
        (84, rows.len() as u32),
    ] {
        put(&mut raw, at, value);
    }
    for index in 0..count {
        if fields[index].1 == "signed64" {
            raw[112 + index * 4..114 + index * 4].copy_from_slice(&(-32i16).to_le_bytes());
        }
        raw[114 + index * 4..116 + index * 4]
            .copy_from_slice(&(offsets[index] as u16).to_le_bytes());
        let codec = 112 + count * 4 + index * 24;
        raw[codec..codec + 2].copy_from_slice(&((offsets[index] * 8) as u16).to_le_bytes());
        raw[codec + 2..codec + 4].copy_from_slice(&((sizes[index] * 8) as u16).to_le_bytes());
    }
    let mut strings = vec![0];
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), count);
        for (column, cell) in row.iter().enumerate() {
            let values = match cell {
                Cell::Array(values) => values.iter().collect::<Vec<_>>(),
                cell => vec![cell],
            };
            assert_eq!(values.len(), widths[column]);
            for (slot, cell) in values.into_iter().enumerate() {
                let field_offset = offsets[column] + slot * 4;
                let at = metadata + index * stride + field_offset;
                if let Cell::I64(value) = cell {
                    assert_eq!(fields[column].1, "signed64");
                    raw[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    continue;
                }
                let value = match cell {
                    Cell::U(n) => *n,
                    Cell::I(n) => *n as u32,
                    Cell::I64(_) => unreachable!("64-bit scalar was written above"),
                    Cell::F(n) => n.to_bits(),
                    Cell::Text(text) if text.is_empty() => 0,
                    Cell::Text(text) => {
                        let relative = (rows.len() - index) * stride - field_offset + strings.len();
                        strings.extend(text.as_bytes());
                        strings.push(0);
                        relative as u32
                    }
                    Cell::BadString => u32::MAX / 2,
                    Cell::Array(_) => panic!("nested fixture array"),
                };
                put(&mut raw, at, value);
            }
        }
    }
    put(&mut raw, 16, strings.len() as u32);
    put(&mut raw, 88, strings.len() as u32);
    raw.extend(strings);
    let mut dbd = String::from("COLUMNS\n");
    for (field, kind) in fields {
        dbd.push_str(&format!(
            "{} {field}\n",
            match *kind {
                "text" => "locstring",
                "float" | "array2float" => "float",
                _ => "int",
            }
        ));
    }
    dbd.push_str(&format!("\nBUILD {BUILD}\nLAYOUT {layout:08X}\n"));
    for (field, kind) in fields {
        dbd.push_str(&format!(
            "{}{field}{}\n",
            if *field == "ID" { "$id$" } else { "" },
            match *kind {
                "text" => "",
                "signed" => "<32>",
                "signed64" => "<64>",
                "float" => "<32>",
                "array2signed" => "<32>[2]",
                "array2float" => "<32>[2]",
                "array3" => "<u32>[3]",
                _ => "<u32>",
            }
        ));
    }
    Table {
        name,
        id,
        hash,
        layout,
        raw,
        dbd,
    }
}
pub fn items() -> Vec<Table> {
    use Cell::{I, Text, U};
    vec![
        table(
            "Item",
            401,
            &[
                ("ID", "uint"),
                ("ClassID", "signed"),
                ("SubclassID", "signed"),
            ],
            &[vec![U(7), I(4), I(2)]],
        ),
        table(
            "ItemSparse",
            402,
            &[
                ("ID", "uint"),
                ("Display_lang", "text"),
                ("InventoryType", "signed"),
                ("OverallQualityID", "signed"),
            ],
            &[
                vec![U(9), Text("无分类🙂".into()), I(99), I(3)],
                vec![U(7), Text("腰带|cffaabbcc中文\r\n".into()), I(6), I(4)],
            ],
        ),
    ]
}
pub fn decors(rows: &[Vec<Cell>]) -> Table {
    table(
        "HouseDecor",
        403,
        &[
            ("ID", "uint"),
            ("Name_lang", "text"),
            ("ModelFileDataID", "uint"),
            ("ThumbnailFileDataID", "uint"),
            ("ItemID", "uint"),
            ("GameObjectID", "uint"),
            ("Type", "signed"),
            ("ModelType", "signed"),
        ],
        rows,
    )
}
pub fn decor_rows() -> Vec<Vec<Cell>> {
    use Cell::{I, Text, U};
    vec![
        vec![
            U(30),
            Text("第三".into()),
            U(501),
            U(601),
            U(42),
            U(701),
            I(2),
            I(3),
        ],
        vec![
            U(10),
            Text("无模型🙂".into()),
            U(0),
            U(600),
            U(41),
            U(700),
            I(-1),
            I(0),
        ],
        vec![
            U(20),
            Text("第二".into()),
            U(501),
            U(602),
            U(42),
            U(702),
            I(4),
            I(5),
        ],
    ]
}
fn md5(raw: &[u8]) -> [u8; 16] {
    Md5::digest(raw).into()
}
fn write(root: &Path, relative: impl AsRef<Path>, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
struct Object {
    ckey: [u8; 16],
    ekey: [u8; 16],
    decoded: usize,
    raw: Vec<u8>,
}
fn object(bytes: &[u8]) -> Object {
    let mut raw = b"BLTE\0\0\0\0N".to_vec();
    raw.extend(bytes);
    Object {
        ckey: md5(bytes),
        ekey: md5(&raw),
        decoded: bytes.len(),
        raw,
    }
}
fn encoding(objects: &[Object]) -> Object {
    assert!(objects.len() < 26);
    let mut ck: Vec<_> = objects.iter().collect();
    ck.sort_by_key(|o| o.ckey);
    let mut ek: Vec<_> = objects.iter().collect();
    ek.sort_by_key(|o| o.ekey);
    let mut cp = vec![0; 1024];
    let mut ep = vec![0; 1024];
    for (i, o) in ck.iter().enumerate() {
        let at = i * 38;
        cp[at] = 1;
        cp[at + 1..at + 6].copy_from_slice(&(o.decoded as u64).to_be_bytes()[3..]);
        cp[at + 6..at + 22].copy_from_slice(&o.ckey);
        cp[at + 22..at + 38].copy_from_slice(&o.ekey);
    }
    for (i, o) in ek.iter().enumerate() {
        let at = i * 25;
        ep[at..at + 16].copy_from_slice(&o.ekey);
        ep[at + 20..at + 25].copy_from_slice(&(o.raw.len() as u64).to_be_bytes()[3..]);
    }
    let mut logical = vec![0; 22];
    logical[..5].copy_from_slice(b"EN\x01\x10\x10");
    logical[5..7].copy_from_slice(&1u16.to_be_bytes());
    logical[7..9].copy_from_slice(&1u16.to_be_bytes());
    logical[9..13].copy_from_slice(&1u32.to_be_bytes());
    logical[13..17].copy_from_slice(&1u32.to_be_bytes());
    logical.extend(ck[0].ckey);
    logical.extend(md5(&cp));
    logical.extend(cp);
    logical.extend(ek[0].ekey);
    logical.extend(md5(&ep));
    logical.extend(ep);
    // Encoding needs a framed range-readable BLTE, with an EKey over its header.
    let blocks = [&logical[..54], &logical[54..]];
    let header = 12 + blocks.len() * 24;
    let mut raw = vec![0; header];
    raw[..4].copy_from_slice(b"BLTE");
    raw[4..8].copy_from_slice(&(header as u32).to_be_bytes());
    raw[8] = 15;
    raw[11] = blocks.len() as u8;
    for (i, block) in blocks.iter().enumerate() {
        let mut payload = vec![b'N'];
        payload.extend(*block);
        let at = 12 + i * 24;
        raw[at..at + 4].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        raw[at + 4..at + 8].copy_from_slice(&(block.len() as u32).to_be_bytes());
        raw[at + 8..at + 24].copy_from_slice(&md5(&payload));
        raw.extend(payload);
    }
    Object {
        ckey: md5(&logical),
        ekey: md5(&raw[..header]),
        decoded: logical.len(),
        raw,
    }
}
fn archive(root: &Path, objects: &[Object]) {
    let mut raw = vec![];
    let mut entries = vec![];
    for o in objects {
        entries.push((o.ekey, raw.len() as u32, (o.raw.len() + 30) as u32));
        raw.extend([0; 30]);
        raw.extend(&o.raw);
    }
    entries.sort_by_key(|e| e.0);
    let mut index = vec![0; 40 + entries.len() * 18];
    put(&mut index, 0, 16);
    index[8..16].copy_from_slice(&[7, 0, 0, 0, 4, 5, 9, 30]);
    index[16..24].copy_from_slice(&(1u64 << 30).to_le_bytes());
    let guard = casc::guard_sum(&index[8..24], 0, 0).0;
    put(&mut index, 4, guard);
    put(&mut index, 32, (entries.len() * 18) as u32);
    let (mut a, mut b) = (0, 0);
    for (i, (key, offset, size)) in entries.into_iter().enumerate() {
        let at = 40 + i * 18;
        index[at..at + 9].copy_from_slice(&key[..9]);
        index[at + 10..at + 14].copy_from_slice(&offset.to_be_bytes());
        index[at + 14..at + 18].copy_from_slice(&size.to_le_bytes());
        (a, b) = casc::guard_sum(&index[at..at + 18], a, b);
    }
    put(&mut index, 36, a);
    write(root, "Data/data/data.000", &raw);
    write(root, "Data/data/0000000001.idx", &index);
}

pub struct Fixture {
    pub root: PathBuf,
    pub home: PathBuf,
    pub client: PathBuf,
    pub pin: String,
}
impl Fixture {
    pub fn new(tables: &[Table]) -> Self {
        Self::locale(tables, "zhCN", 0x40)
    }
    pub fn locale(tables: &[Table], language: &str, mask: u32) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/rust-catalog-navigation-fixtures")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        Self::at(root, tables, language, mask)
    }
    pub fn at(root: PathBuf, tables: &[Table], language: &str, mask: u32) -> Self {
        Self::at_objects(root, tables, language, mask, None)
    }
    /// A real chunked BLTE object whose complete decoded body is generated and
    /// hashed incrementally. The fixture builder never materializes that body.
    pub fn compressed_at(root: PathBuf, decoded_bytes: usize) -> Self {
        Self::compressed_key_at(root, decoded_bytes, false)
    }
    pub fn compressed_with_wrong_ckey_at(root: PathBuf, decoded_bytes: usize) -> Self {
        Self::compressed_key_at(root, decoded_bytes, true)
    }
    fn compressed_key_at(root: PathBuf, decoded_bytes: usize, wrong_ckey: bool) -> Self {
        use flate2::{Compression, write::ZlibEncoder};
        use sha2::Sha256;
        use std::io::Write;
        assert!((1..=512 << 20).contains(&decoded_bytes));
        let block = [b'x'; 65536];
        let mut whole_md5 = Md5::new();
        let mut whole_sha = Sha256::new();
        let mut chunks = Vec::new();
        let mut remaining = decoded_bytes;
        while remaining != 0 {
            let length = remaining.min(8 << 20);
            let mut compressor = ZlibEncoder::new(Vec::new(), Compression::fast());
            let mut left = length;
            while left != 0 {
                let bytes = &block[..left.min(block.len())];
                compressor.write_all(bytes).unwrap();
                whole_md5.update(bytes);
                whole_sha.update(bytes);
                left -= bytes.len();
            }
            let mut encoded = vec![b'Z'];
            encoded.extend(compressor.finish().unwrap());
            chunks.push((length, encoded));
            remaining -= length;
        }
        let mut raw = b"BLTE".to_vec();
        raw.extend(((12 + chunks.len() * 24) as u32).to_be_bytes());
        raw.extend([15, 0, 0, chunks.len() as u8]);
        for (length, encoded) in &chunks {
            raw.extend((encoded.len() as u32).to_be_bytes());
            raw.extend((*length as u32).to_be_bytes());
            raw.extend(md5(encoded));
        }
        let ekey = md5(&raw);
        for (_, encoded) in chunks.iter() {
            raw.extend(encoded);
        }
        let encoded_bytes = raw.len();
        let mut ckey: [u8; 16] = whole_md5.finalize().into();
        if wrong_ckey {
            ckey[0] ^= 1;
        }
        let digest = casc::hex(&whole_sha.finalize());
        let selected = Object {
            ckey,
            ekey,
            decoded: decoded_bytes,
            raw,
        };
        let table = table(
            "CompressedContent",
            501,
            &[("ID", "uint")],
            &[vec![Cell::U(7)]],
        );
        let fixture = Self::at_objects(root, &[table], "zhCN", 0x40, Some(selected));
        let path = fixture.root.join("input-identity.json");
        let mut identity: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        identity["tables"][0]["bytes"] = json!(decoded_bytes);
        identity["tables"][0]["sha256"] = json!(digest);
        identity["content"] = json!({"fileDataID":501,"decodedBytes":decoded_bytes,
            "encodedBytes":encoded_bytes,"sha256":digest,"contentKey":casc::hex(&ckey),
            "encodingKey":casc::hex(&ekey),"chunks":chunks.len(),"fillByte":120});
        fs::write(path, serde_json::to_vec_pretty(&identity).unwrap()).unwrap();
        fixture
    }
    fn at_objects(
        root: PathBuf,
        tables: &[Table],
        language: &str,
        mask: u32,
        selected: Option<Object>,
    ) -> Self {
        assert!(!root.exists(), "fresh owned fixture root required");
        assert!(!tables.is_empty());
        let common = root.join("game");
        let client = common.join("不是身份 client");
        let home = root.join("home");
        let mut ordered: Vec<_> = tables.iter().collect();
        ordered.sort_by_key(|t| t.id);
        let mut objects: Vec<_> = ordered.iter().map(|t| object(&t.raw)).collect();
        if let Some(selected) = selected {
            assert_eq!(objects.len(), 1);
            objects[0] = selected;
        }
        let mut root_raw = b"TSFM".to_vec();
        for n in [
            tables.len() as u32,
            0,
            tables.len() as u32,
            0x10000000,
            mask,
        ] {
            root_raw.extend(n.to_le_bytes());
        }
        let mut next = 0;
        for t in &ordered {
            root_raw.extend((t.id - next).to_le_bytes());
            next = t.id + 1;
        }
        for object in &objects {
            root_raw.extend(object.ckey);
        }
        let root_object = object(&root_raw);
        let root_key = casc::hex(&root_object.ckey);
        objects.push(root_object);
        let encoded = encoding(&objects);
        let encoding_key = casc::hex(&encoded.ekey);
        let build = format!(
            "build-uid = wow\nroot = {root_key}\nencoding = {} {encoding_key}\nencoding-size = {} {}\n",
            casc::hex(&encoded.ckey),
            encoded.decoded,
            encoded.raw.len()
        );
        let cdn = b"archives =\n";
        let build_key = casc::hex(&md5(build.as_bytes()));
        let cdn_key = casc::hex(&md5(cdn));
        objects.push(encoded);
        archive(&common, &objects);
        for (key, bytes) in [(&build_key, build.as_bytes()), (&cdn_key, cdn.as_slice())] {
            write(
                &common,
                format!("Data/config/{}/{}/{key}", &key[..2], &key[2..4]),
                bytes,
            );
        }
        write(&common,".build.info",format!("Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|{BUILD}|{build_key}|{cdn_key}|1\n").as_bytes());
        write(&client, ".flavor.info", b"Product Flavor!STRING:0\nwow\n");
        write(&client, "version.txt", BUILD.as_bytes());
        workspace::initialize(&home).unwrap();
        let target = installed::resolve_target(&installed::LocalTargetRequest {
            installation: client.clone(),
            product: Some("retail".into()),
            full_build: Some(BUILD.into()),
            region: "cn".into(),
            language: language.into(),
            definition_commit: DEFS.into(),
        })
        .unwrap();
        let pin = selection::pin_selection(
            &home,
            SelectionSpec {
                data: Some(target.data),
                ..Default::default()
            },
        )
        .unwrap();
        let base = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{DEFS}/");
        let manifest:Vec<_>=tables.iter().map(|t|json!({"tableName":t.name,"tableHash":format!("{:08X}",t.hash),"db2FileDataID":t.id})).collect();
        let mut resources = vec![(
            "manifest.json".to_owned(),
            serde_json::to_vec(&manifest).unwrap(),
        )];
        resources.extend(tables.iter().map(|t| {
            (
                format!("definitions/{}.dbd", t.name),
                t.dbd.as_bytes().to_vec(),
            )
        }));
        let metadata = data_cache::Cache::new(&home);
        for (suffix, bytes) in resources {
            let url = format!("{base}{suffix}");
            let blob = evidence::publish_blob(&home, &bytes, 4 << 20).unwrap();
            metadata
                .put(
                    &format!("definition/{}", storage::sha256(url.as_bytes())),
                    &json!({"url":url,"blob":blob}),
                    None,
                )
                .unwrap();
        }
        let table_identity:Vec<_>=tables.iter().map(|t|json!({"name":t.name,"fileDataID":t.id,"layout":t.layout,"bytes":t.raw.len(),"sha256":storage::sha256(&t.raw),"dbdSHA256":storage::sha256(t.dbd.as_bytes())})).collect();
        write(&root,"input-identity.json",&serde_json::to_vec_pretty(&json!({"schema":"lycheedev.catalog-navigation-fixture.v1","scope":"synthetic authenticated local input; not real-client evidence","home":home,"installation":client,"snapshot":pin.id,"pin":pin.data,"tables":table_identity})).unwrap());
        Self {
            root,
            home,
            client,
            pin: pin.id,
        }
    }
    pub fn invocation(&self, command: &str, selectors: &[(&str, &str)]) -> Invocation {
        let mut flags = BTreeMap::from([
            ("--snapshot".into(), self.pin.clone()),
            (
                "--installation".into(),
                self.client.to_string_lossy().into_owned(),
            ),
            ("--offline".into(), "true".into()),
        ]);
        flags.extend(
            selectors
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into())),
        );
        Invocation {
            command: command.into(),
            positional: None,
            flags,
            home: self.home.clone(),
            cwd: self.root.clone(),
            control: Default::default(),
        }
    }
    pub fn verify(&self, outcome: &lycheedev_core::Outcome) {
        assert_eq!(outcome.provenance.len(), 1);
        let capture = &outcome.provenance[0];
        let id = capture["id"].as_str().unwrap();
        assert_eq!(evidence::read_capture(&self.home, id).unwrap(), *capture);
        let raw = evidence::read_blob(&self.home, &capture["blob"], 16 << 20).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&raw).unwrap(),
            outcome.data
        );
        assert_eq!(capture["complete"], outcome.complete);
        assert_eq!(capture["truncated"], outcome.data["truncated"]);
        let mut inv = self.invocation("evidence verify", &[]);
        inv.flags.clear();
        inv.positional = Some(id.into());
        let verified = evidence::dispatch(&inv).unwrap();
        assert_eq!(verified.data["verified"], true);
    }
}

/// Two real WDC3 sections: readable ID 1/42 and encrypted ID 2/99.
/// Ciphertext is the independent fixed Go323 Salsa20 fixture vector.
pub fn encrypted_sections(id: u32) -> Table {
    let mut spec = table(
        "Sample",
        id,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(42)]],
    );
    let mut one = spec.raw;
    one.pop();
    put(&mut one, 16, 0);
    put(&mut one, 88, 0);
    let start = u32::from_le_bytes(one[80..84].try_into().unwrap()) + 40;
    let mut raw = one[..112].to_vec();
    raw.extend([0; 40]);
    raw.extend_from_slice(&one[112..]);
    raw.extend(2u32.to_le_bytes());
    raw.extend(99u32.to_le_bytes());
    for (at, value) in [
        (4, 2),
        (32, 2),
        (68, 2),
        (80, start),
        (120, start + 8),
        (124, 1),
    ] {
        put(&mut raw, at, value);
    }
    raw[112..120].copy_from_slice(&0x1234567890abcdefu64.to_le_bytes());
    spec.raw = raw;
    spec
}
pub fn framed_chunks(chunks: &[(&[u8], usize)]) -> (String, Vec<u8>) {
    let mut raw = b"BLTE".to_vec();
    raw.extend(((12 + chunks.len() * 24) as u32).to_be_bytes());
    raw.extend([15, 0, 0, chunks.len() as u8]);
    for (chunk, decoded) in chunks {
        raw.extend((chunk.len() as u32).to_be_bytes());
        raw.extend((*decoded as u32).to_be_bytes());
        raw.extend(md5(chunk));
    }
    let key = casc::hex(&md5(&raw));
    for (chunk, _) in chunks {
        raw.extend(*chunk);
    }
    (key, raw)
}
pub fn encrypted_frame(raw: &[u8]) -> (String, Vec<u8>) {
    let end = raw.len() - 8;
    let first = [b"N".as_slice(), &raw[..end]].concat();
    let second = [
        0x45, 8, 0xef, 0xcd, 0xab, 0x90, 0x78, 0x56, 0x34, 0x12, 4, 0x11, 0x22, 0x33, 0x44, b'S',
        0xc6, 0xab, 0x63, 0xb7, 0xda, 0xa0, 0x5c, 0xd3, 0x83,
    ];
    framed_chunks(&[(&first, end), (&second, 8)])
}
impl Fixture {
    pub fn with_encoded_table(table: Table, encoded: Vec<u8>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/rust-partial-table-fixtures")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        Self::with_encoded_table_at(root, table, encoded)
    }
    pub fn with_encoded_table_at(root: PathBuf, table: Table, encoded: Vec<u8>) -> Self {
        match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => panic!("partial fixture root already exists: {}", root.display()),
            Err(error) => panic!("partial fixture root cannot be checked: {error}"),
        }
        let header = u32::from_be_bytes(encoded[4..8].try_into().unwrap()) as usize;
        let selected = Object {
            ckey: md5(&table.raw),
            ekey: md5(if header == 0 {
                &encoded
            } else {
                &encoded[..header]
            }),
            decoded: table.raw.len(),
            raw: encoded,
        };
        Self::at_objects(root, &[table], "zhCN", 0x40, Some(selected))
    }
}
