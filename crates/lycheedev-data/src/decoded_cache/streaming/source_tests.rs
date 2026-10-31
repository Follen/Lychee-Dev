//! Independent cache/key vectors through the production streamed CASC reader.
use super::*;
use crate::metadata_format::{EncodedRecord, EncodingLookup};
use lycheedev_core::workspace;
use std::{collections::BTreeMap, fs, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    encoded: Vec<u8>,
    record: EncodingRecord,
}
impl Fixture {
    fn new(encoded: Vec<u8>, body: &[u8]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!("lychee-stream-cache-{}-{}-{}",
            std::process::id(), storage::utc_now().replace([':', '.'], "-"), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&root).unwrap();
        let home=root.join("home"); workspace::initialize(&home).unwrap();
        let header=u32::from_be_bytes(encoded[4..8].try_into().unwrap()) as usize;
        let key=Md5::digest(if header==0 { &encoded } else { &encoded[..header] });
        // One independent idx entry: archive 0, offset 0, physical prefix 30.
        let mut index=vec![0;58];index[..4].copy_from_slice(&16u32.to_le_bytes());
        index[8..16].copy_from_slice(&[7,0,0,0,4,5,9,30]);
        index[16..24].copy_from_slice(&(1u64<<30).to_le_bytes());
        let sum=casc::guard_sum(&index[8..24],0,0).0;index[4..8].copy_from_slice(&sum.to_le_bytes());
        index[32..36].copy_from_slice(&18u32.to_le_bytes());index[40..49].copy_from_slice(&key[..9]);
        index[54..58].copy_from_slice(&((encoded.len()+30) as u32).to_le_bytes());
        let sum=casc::guard_sum(&index[40..],0,0).0;index[36..40].copy_from_slice(&sum.to_le_bytes());
        fs::create_dir_all(root.join("Data/data")).unwrap();
        fs::write(root.join("Data/data/0000000001.idx"),index).unwrap();
        let f=Self{root,home,encoded,record:EncodingRecord{content_key:casc::hex(&Md5::digest(body)),decoded_bytes:body.len() as u64,encoding_keys:vec![casc::hex(&key)],page:0}};
        f.write_source(&f.encoded); f
    }
    fn write_source(&self, encoded: &[u8]) {
        let mut archive=vec![0;30];archive.extend(encoded);
        fs::write(self.root.join("Data/data/data.000"),archive).unwrap();
    }
    fn keys(&self, control: &OperationControl, private: bool) -> keys::Context {
        let path=self.root.join("private.json");
        if private {fs::write(&path,br#"{"000000000000002b":"000102030405060708090a0b0c0d0e0f"}"#).unwrap();}
        keys::Context::prepare_scoped(&self.home,private.then(||path.to_str().unwrap()),true,control,control.deadline(),None).unwrap()
    }
    fn read(&self, keys: &mut keys::Context, stats: &mut Stats, budget: &Budget, control: &OperationControl) -> Result<evidence::VerifiedBlob> {
        read_verified_content(&self.home,&self.root,self,&self.record,casc::Limits{deadline:control.deadline(),..Default::default()},1<<20,keys,stats,budget,control).map(|(_,blob)|blob)
    }
}
// The selected Encoding record is fixed input; physical idx/archive opening,
// encoded hashing, BLTE, cache validation/publication and close are production.
impl EncodingLookup for Fixture {
    fn physical(&self, _: &BTreeMap<u64,Vec<u8>>, key: &str, _: casc::Limits, check: &mut dyn FnMut()->Result<()>) -> Result<EncodedRecord> {
        check()?;assert_eq!(key,self.record.encoding_keys[0]);
        Ok(EncodedRecord{encoding_key:key.into(),encoded_bytes:self.encoded.len() as u64,specification:0})
    }
}
impl Drop for Fixture {
    fn drop(&mut self){if !std::thread::panicking(){fs::remove_dir_all(&self.root).unwrap();}}
}
fn frame(payload: &[u8], decoded: u32) -> Vec<u8> {
    let mut raw=b"BLTE".to_vec();raw.extend(36u32.to_be_bytes());raw.extend([15,0,0,1]);
    raw.extend((payload.len() as u32).to_be_bytes());raw.extend(decoded.to_be_bytes());raw.extend(Md5::digest(payload));raw.extend(payload);raw
}
fn body(blob: evidence::VerifiedBlob) -> Vec<u8> {
    let mut blob=blob;let mut bytes=Vec::new();let result=blob.read_to_end(&mut bytes).map(|_|bytes).map_err(blte::stream_io_error);blob.close_after(result).unwrap()
}

#[test]
fn nested_missing_key_and_corruption_release_stream_ownership_and_keep_cancellation() {
    let mut encrypted=vec![b'E',8];encrypted.extend(42u64.to_le_bytes());encrypted.push(4);encrypted.extend([0;4]);encrypted.push(b'S');encrypted.extend([0;6]);
    let mut nested=vec![b'F'];nested.extend(frame(&encrypted,5));
    let f=Fixture::new(frame(&nested,5),b"hello");
    let control=OperationControl::default();let budget=Budget::controlled(Cost::defaults(),control.deadline(),control.clone());
    let mut keys=f.keys(&control,false);let mut stats=Stats::default();
    let error=f.read(&mut keys,&mut stats,&budget,&control).err().unwrap();
    assert_eq!(error.code,"container.key_unavailable");assert_eq!(keys.source.state,"unavailable_offline");
    assert_eq!(budget.refundable_retained_bytes(),0,"failed stream and its scratch owners are released");
    assert!(budget.snapshot().unwrap().decode_work>0);assert!(stats.content_decode_calls>0);
    let mut corrupt=f.encoded.clone();*corrupt.last_mut().unwrap()^=1;f.write_source(&corrupt);
    let mut keys=f.keys(&control,true);
    assert_eq!(f.read(&mut keys,&mut stats,&budget,&control).err().unwrap().code,"container.integrity_mismatch");
    assert_eq!(budget.refundable_retained_bytes(),0);
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(f.read(&mut keys,&mut stats,&budget,&control).err().unwrap().exit_code,7);
}

#[test]
fn streamed_cache_rehashes_original_source_blob_and_alias_before_reuse() {
    let expected=b"authenticated fixture content";
    let mut encoded=b"BLTE\0\0\0\0N".to_vec();encoded.extend(expected);
    let f=Fixture::new(encoded,expected);let control=OperationControl::default();
    let budget=Budget::controlled(Cost::defaults(),control.deadline(),control.clone());
    let mut keys=f.keys(&control,false);let mut stats=Stats::default();
    assert_eq!(body(f.read(&mut keys,&mut stats,&budget,&control).unwrap()),expected);
    assert_eq!(stats.hits,0);assert_eq!(stats.content_decode_calls,1);
    let mut stats=Stats::default();
    assert_eq!(body(f.read(&mut keys,&mut stats,&budget,&control).unwrap()),expected);
    assert_eq!(stats.hits,1);assert_eq!(stats.content_decode_calls,0);assert_eq!(stats.content_hash_bytes,f.encoded.len() as u64);
    let mut changed=f.encoded.clone();changed[9]^=1;f.write_source(&changed);
    assert_eq!(f.read(&mut keys,&mut stats,&budget,&control).err().unwrap().code,"archive.index_integrity");
    f.write_source(&f.encoded);
    let blob=evidence::blob_path(&f.home,&storage::sha256(expected)).unwrap();fs::write(&blob,vec![b'x';expected.len()]).unwrap();
    assert_eq!(f.read(&mut keys,&mut stats,&budget,&control).err().unwrap().code,"vault.blob_integrity");
    fs::remove_file(&blob).unwrap();
    assert_eq!(body(f.read(&mut keys,&mut stats,&budget,&control).unwrap()),expected);
    let mut material=format!("decoded-v1\0{}\0public\0{}\0",f.record.content_key,keys::PUBLIC_SHA256).into_bytes();material.extend(&f.encoded);
    let alias=format!("decoded-v1/{}",storage::sha256(&material));
    let raw=DataCacheMetadata::read_document_raw_controlled_with_check(&f.home,&alias,&control,||Ok(())).unwrap();
    let mut value:Value=serde_json::from_slice(&raw.bytes).unwrap();value["contentKey"]=serde_json::json!("a".repeat(32));
    DataCacheMetadata::put_controlled_with_check(&f.home,&alias,&value,Some(raw.generation),&control,||Ok(())).unwrap();
    assert_eq!(f.read(&mut keys,&mut stats,&budget,&control).err().unwrap().code,"vault.blob_integrity");
}

#[test]
fn selected_archive_uses_newest_index_and_rejects_forged_full_identity() {
    let expected = b"decoded authenticated content";
    let mut encoded = b"BLTE\0\0\0\0N".to_vec();
    encoded.extend(expected);
    let f = Fixture::new(encoded, expected);
    fs::rename(f.root.join("Data/data/0000000001.idx"), f.root.join("Data/data/0000000002.idx")).unwrap();
    fs::write(f.root.join("Data/data/0000000001.idx"), b"damaged old generation").unwrap();
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let mut keys = f.keys(&control, false);
    assert_eq!(body(f.read(&mut keys, &mut Stats::default(), &budget, &control).unwrap()), expected);
    // The same nine-byte idx candidate never authorizes a different full EKey.
    let mut collision = casc::key(&f.record.encoding_keys[0]).unwrap();
    collision[15] ^= 1;
    assert_eq!(casc::open_selected(&f.root, &casc::hex(&collision), f.encoded.len(), casc::Limits::default(), &budget, &control).err().unwrap().code, "archive.index_integrity");
    fs::write(f.root.join("Data/data/0000000003.idx"), b"damaged newest generation").unwrap();
    assert_eq!(f.read(&mut keys, &mut Stats::default(), &budget, &control).err().unwrap().code, "archive.invalid_index");
}

#[test]
fn selected_framed_identity_authenticates_directory_then_stream_authenticates_payload_and_ckey() {
    let expected = b"hello";
    let mut f = Fixture::new(frame(b"Nhello", 5), expected);
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let mut keys = f.keys(&control, false);
    assert_eq!(body(f.read(&mut keys, &mut Stats::default(), &budget, &control).unwrap()), expected);
    let original_ckey = f.record.content_key.clone();
    f.record.content_key = "0".repeat(32);
    assert_eq!(f.read(&mut keys, &mut Stats::default(), &budget, &control).err().unwrap().code, "records.content_integrity");
    f.record.content_key = original_ckey;
    let mut damaged = f.encoded.clone();
    *damaged.last_mut().unwrap() ^= 1;
    f.write_source(&damaged);
    // A framed EKey covers only the directory: selected opening still succeeds.
    let selected = casc::open_selected(&f.root, &f.record.encoding_keys[0], f.encoded.len(), casc::Limits::default(), &budget, &control).unwrap();
    selected.close_after(Ok(())).unwrap();
    assert_eq!(f.read(&mut keys, &mut Stats::default(), &budget, &control).err().unwrap().code, "container.integrity_mismatch");
}
