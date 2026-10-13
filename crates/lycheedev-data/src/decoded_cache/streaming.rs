//! Selected assets use immutable verified blobs rather than full decoded Vecs.
use super::*;
use lycheedev_core::control::OperationControl;
use std::io::Read;

#[allow(clippy::too_many_arguments)]
pub(crate) fn read_verified_content(
    home: &Path,
    root: &Path,
    index: &Encoding<'_>,
    record: &EncodingRecord,
    limits: casc::Limits,
    maximum: usize,
    keys: &mut keys::Context,
    stats: &mut Stats,
    budget: &Budget,
    control: &OperationControl,
) -> Result<(String, evidence::VerifiedBlob)> {
    let expected_key = casc::key(&record.content_key)?;
    let decoded = usize::try_from(record.decoded_bytes)
        .map_err(|_| fault("records.metadata_limit", 3, "Decoded content size exceeds platform bound"))?;
    let _io_buffers = budget.reserve_retained(4 * 65536)?;
    let mut unavailable = None;
    for encoding_key in &record.encoding_keys {
        let physical = index.find_encoding(encoding_key, limits, budget.token())?;
        let encoded = usize::try_from(physical.encoded_bytes)
            .map_err(|_| fault("records.metadata_limit", 3, "Encoded content size exceeds platform bound"))?;
        if encoded > maximum {
            return Err(fault("records.metadata_limit", 3, "Selected encoded content exceeds byte budget"));
        }
        let mut object = match casc::open_selected(root, encoding_key, encoded, limits, budget) {
            Ok(object) => object,
            Err(error) if error.code == "records.local_object_unavailable" => {
                unavailable = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };
        let mut identity = Sha256::new();
        identity.update(format!("decoded-v1\0{}\0{}\0{}\0", record.content_key, keys.source.kind, keys.source.sha256).as_bytes());
        stats.content_hash_calls += 1;
        let mut buffer = [0; 65536];
        loop {
            let n = object.read(&mut buffer).map_err(blte::stream_io_error)?;
            if n == 0 { break; }
            budget.charge(Cost::work(1))?;
            identity.update(&buffer[..n]);
            stats.content_hash_bytes += n as u64;
        }
        let cache_key = format!("decoded-v1/{}", casc::hex(&identity.finalize()));
        let mut metadata = Metadata::open(home, true)?;
        let existing = match metadata.read::<Entry>(&cache_key) {
            Ok(entry) => Some(entry),
            Err(error) if error.code == "vault.record_missing" => None,
            Err(error) => return Err(error),
        };
        if let Some(entry) = &existing {
            if entry.content_key != record.content_key
                || entry.blob["bytes"].as_u64() != Some(record.decoded_bytes)
                || entry.keys.kind != keys.source.kind || entry.keys.sha256 != keys.source.sha256
            { return Err(integrity()); }
            if entry.keys.kind == "public" && entry.keys.state == "loaded" { keys.ensure_public()?; }
            let path = evidence::blob_path(home, entry.blob["sha256"].as_str().ok_or_else(integrity)?)?;
            storage::guard_path(home, &path)?;
            if path.try_exists()? {
                let mut ckey = Md5::new();
                let blob = evidence::open_verified_blob_bounded(home, &entry.blob, maximum as u64, control, |block| {
                    budget.charge(Cost::work(2))?;
                    ckey.update(block);
                    Ok(())
                })?;
                if ckey.finalize().as_slice() != expected_key { return Err(integrity()); }
                stats.blob_reads += 1;
                stats.blob_read_bytes += record.decoded_bytes;
                stats.hits += 1;
                stats.reused_bytes += record.decoded_bytes;
                return Ok((encoding_key.clone(), blob));
            }
        }
        // The first bounded decode establishes the content-addressed filename.
        // The second writes to Destination's private stage and verifies the same
        // CKey/SHA256 before rename. The source handle remains immutable across
        // both passes; no temporary 512 MiB body or unverified blob is exposed.
        let mut decode_hash = |keys: &keys::Context| -> Result<String> {
            object.rewind()?;
            let mut reader = blte::StreamingReader::new(&mut object, encoded, decoded, expected_key, keys.keys(), budget)?;
            stats.content_decode_calls += 1;
            let mut sha = Sha256::new();
            loop {
                let n = reader.read(&mut buffer).map_err(blte::stream_io_error)?;
                if n == 0 { break; }
                budget.charge(Cost::work(1))?;
                sha.update(&buffer[..n]);
            }
            Ok(casc::hex(&sha.finalize()))
        };
        let sha256 = match decode_hash(keys) {
            Ok(digest) => digest,
            Err(error) if error.code == "container.key_unavailable"
                && error.details["missingKey"] == true
                && keys.source.kind == "public" && keys.source.state != "loaded" => {
                keys.ensure_public()?;
                decode_hash(keys)?
            }
            Err(error) => return Err(error),
        };
        object.rewind()?;
        let mut reader = blte::StreamingReader::new(&mut object, encoded, decoded, expected_key, keys.keys(), budget)?;
        stats.content_decode_calls += 1;
        budget.charge(Cost::work(decoded.div_ceil(65536)))?;
        let blob = evidence::publish_blob_reader_bounded(
            home, &mut reader, &evidence::BlobRef { sha256, bytes: record.decoded_bytes }, maximum as u64, control,
        )?;
        stats.extracted_bytes += record.decoded_bytes;
        stats.blob_reads += 1;
        stats.blob_read_bytes += record.decoded_bytes;
        let entry = Entry { content_key: record.content_key.clone(), blob: serde_json::to_value(blob.reference())?, keys: keys.source.clone() };
        match metadata.put(&cache_key, &entry, None) {
            Ok(()) => {}
            Err(error) if error.code == "vault.generation_conflict" => {
                let other: Entry = metadata.read(&cache_key)?;
                if other.content_key != entry.content_key || other.blob != entry.blob
                    || other.keys.kind != entry.keys.kind || other.keys.sha256 != entry.keys.sha256
                { return Err(integrity()); }
            }
            Err(error) => return Err(error),
        }
        budget.check()?;
        return Ok((encoding_key.clone(), blob));
    }
    Err(unavailable.unwrap_or_else(|| fault("records.local_object_unavailable", 3, "No physical encoding alternative is available")))
}
