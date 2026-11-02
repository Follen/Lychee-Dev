use super::*;
use base64::Engine;
use serde_json::Value;
pub(in crate::dd::channel) struct Fixture {
    pub(in crate::dd::channel) base: u64,
    labels: Value,
    blocks: BTreeMap<u64, Vec<u8>>,
    reads: Vec<(u64, usize)>,
}
impl Fixture {
    pub(in crate::dd::channel) fn new() -> Self {
        let value: Value = serde_json::from_str(include_str!("fixtures.json")).unwrap();
        let blocks = value["regions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["base"].as_u64().unwrap(),
                    base64::engine::general_purpose::STANDARD
                        .decode(row["bytes"].as_str().unwrap())
                        .unwrap(),
                )
            })
            .collect();
        Self {
            base: value["base"].as_u64().unwrap(),
            labels: value["labels"].clone(),
            blocks,
            reads: Vec::new(),
        }
    }
    pub(in crate::dd::channel) fn label(&self, name: &str) -> u64 {
        self.labels[name].as_u64().unwrap()
    }
    fn write(&mut self, address: u64, bytes: &[u8]) {
        let (&base, block) = self.blocks.range_mut(..=address).next_back().unwrap();
        let offset = (address - base) as usize;
        block[offset..offset + bytes.len()].copy_from_slice(bytes);
    }
    pub(in crate::dd::channel) fn pointer(&mut self, address: u64, value: u64) {
        self.write(address, &value.to_le_bytes());
    }
    fn read_secret(&self) -> bool {
        let at = self.label("cached_data");
        self.reads
            .iter()
            .any(|&(start, size)| start <= at && at < start + size as u64)
    }
}
impl Memory for Fixture {
    fn read(&mut self, address: u64, size: usize) -> Result<Vec<u8>> {
        self.reads.push((address, size));
        let (&base, block) = self
            .blocks
            .range(..=address)
            .next_back()
            .ok_or_else(|| fault("synthetic unmapped region"))?;
        let start = (address - base) as usize;
        block
            .get(start..start + size)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| fault("synthetic unmapped range"))
    }
}
#[test]
fn independent_gc_dict_and_unicode_bytes_produce_the_original_session_without_id_rounding() {
    let mut memory = Fixture::new();
    let base = memory.base;
    let control = OperationControl::default();
    let labels = memory.labels.clone();
    let mut reader = Reader::new(&mut memory, &control);
    let snapshot = reader.snapshot(base).unwrap();
    assert_eq!(snapshot.uid, "18446744073709551615");
    assert_eq!(snapshot.eid, "100000000000000000000000000001");
    assert_eq!(snapshot.ccid, "1234");
    assert_eq!(snapshot.delta, -3.5);
    assert_eq!(snapshot.session_value, "会话😀 é");
    assert_eq!(snapshot.login_token, "synthetic-private-login-token");
    assert!(snapshot.cached.ends_with(".fixtureSignature"));
    reader.verify(&snapshot).unwrap();
    for (label, expected) in [
        ("latin", "café"),
        ("utf16", "中文"),
        ("wide", "😀"),
        ("noncompact", "非紧凑"),
    ] {
        assert_eq!(
            reader.text(labels[label].as_u64().unwrap()).unwrap(),
            expected
        );
    }
}
#[test]
fn replaced_objects_and_reused_addresses_cannot_keep_the_previous_session_or_identity_authorized() {
    for change in 0..7 {
        let mut memory = Fixture::new();
        let base = memory.base;
        let labels = memory.labels.clone();
        let control = OperationControl::default();
        let mut reader = Reader::new(&mut memory, &control);
        let snapshot = reader.snapshot(base).unwrap();
        let at = |name: &str| labels[name].as_u64().unwrap();
        match change {
            0 => reader
                .memory
                .pointer(at("helper_cached_slot"), at("null_token")),
            1 => reader.memory.write(at("cached_data"), b"x"),
            2 => reader
                .memory
                .write(at("session_data"), &u32::from('二').to_le_bytes()),
            3 => reader.memory.write(at("delta") + 16, &0f64.to_le_bytes()),
            4 => reader.memory.write(at("uid") + 24, &0u32.to_le_bytes()),
            5 => reader.memory.pointer(at("state_name_slot"), at("latin")),
            6 => reader
                .memory
                .pointer(at("helper_login_slot"), at("null_token")),
            _ => unreachable!(),
        }
        let error = reader.verify(&snapshot).unwrap_err();
        assert_eq!(error.exit_code, 3);
        assert!(!error.to_string().contains(&snapshot.cached));
        assert!(!error.to_string().contains(&snapshot.login_token));
    }
}
#[test]
fn incomplete_or_ambiguous_gc_roots_stop_before_reading_credentials() {
    for change in 0..4 {
        let mut memory = Fixture::new();
        let base = memory.base;
        match change {
            0 => memory.write(base + RUNTIME_RVA, &0i32.to_le_bytes()),
            1 => memory.pointer(base + RUNTIME_RVA + 432, base + RUNTIME_RVA + 368),
            2 => {
                let node = memory.label("controller_node");
                memory.pointer(node, node);
            }
            3 => {
                let slot = memory.label("official_module_slot");
                let value = u64::from_le_bytes(memory.read(slot, 8).unwrap().try_into().unwrap());
                let other = memory.label("other_module_slot");
                memory.pointer(other, value);
            }
            _ => unreachable!(),
        }
        assert!(
            Reader::new(&mut memory, &OperationControl::default())
                .snapshot(base)
                .is_err()
        );
        assert!(!memory.read_secret());
    }
}
#[test]
fn variable_instances_split_dicts_and_absent_cache_work_but_limits_and_original_cancel_are_preserved()
 {
    let mut memory = Fixture::new();
    let base = memory.base;
    let typ = memory.label("controller_type");
    let object = memory.label("controller");
    let dict = memory.label("controller_dictionary");
    memory.write(typ + 288, &(-8i64).to_le_bytes());
    memory.pointer(typ + 40, 8);
    memory.pointer(object + 16, 2);
    memory.pointer(object + 40, dict);
    memory.pointer(
        memory.label("helper_cached_slot"),
        memory.label("null_token"),
    );
    let control = OperationControl::default();
    let snapshot = Reader::new(&mut memory, &control).snapshot(base).unwrap();
    assert!(snapshot.cached.is_empty());
    assert_eq!(snapshot.uid, "18446744073709551615");
    let mut memory = Fixture::new();
    let base = memory.base;
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        Reader::new(&mut memory, &control)
            .snapshot(base)
            .err()
            .unwrap()
            .exit_code,
        7
    );
    assert!(memory.reads.is_empty());
    for change in 0..3 {
        let mut memory = Fixture::new();
        let base = memory.base;
        match change {
            0 => {
                let keys = memory.label("controller_dictionary_keys");
                memory.write(keys + 32, &16385i64.to_le_bytes());
            }
            1 => {
                let uid = memory.label("uid");
                memory.write(uid + 24, &(1u32 << 30).to_le_bytes());
            }
            2 => {
                let delta = memory.label("delta");
                memory.write(delta + 16, &f64::NAN.to_le_bytes());
            }
            _ => unreachable!(),
        }
        assert!(
            Reader::new(&mut memory, &OperationControl::default())
                .snapshot(base)
                .is_err()
        );
    }
}
