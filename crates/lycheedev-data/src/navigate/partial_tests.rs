//! Actual WDC3 missing-section proof through both independent spell profiles.
use super::*;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
fn partial_spell() -> fixture::Table {
    use fixture::Cell::U;
    let mut table = fixture::table(
        "SpellEffect",
        303,
        &[("ID", "uint"), ("SpellID", "uint"), ("Effect", "uint")],
        &[vec![U(1), U(42), U(6)]],
    );
    let mut first = table.raw;
    first.pop();
    first[16..20].fill(0);
    first[88..92].fill(0);
    let start = u32::from_le_bytes(first[80..84].try_into().unwrap()) + 40;
    let mut raw = first[..112].to_vec();
    raw.extend([0; 40]);
    raw.extend_from_slice(&first[112..]);
    raw.extend(2u32.to_le_bytes());
    // This suffix uses the independent encrypted N+2+99 vector.
    raw.extend(2u32.to_le_bytes());
    raw.extend(99u32.to_le_bytes());
    for (at, value) in [
        (4, 2),
        (32, 2),
        (68, 2),
        (80, start),
        (120, start + 12),
        (124, 1),
    ] {
        raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    raw[112..120].copy_from_slice(&0x1234567890abcdefu64.to_le_bytes());
    table.raw = raw;
    table
}
#[test]
fn spell_profiles_retain_readable_rows_and_partial_capture_proof() {
    let table = partial_spell();
    let encoded = fixture::encrypted_frame(&table.raw).1;
    let fx = fixture::Fixture::with_encoded_table(table, encoded);
    let keys = fx.root.join("keys.txt");
    std::fs::write(&keys, "0000000000000001 000102030405060708090a0b0c0d0e0f\n").unwrap();
    for command in ["data spell auras", "data spell summons"] {
        let inv = fx.invocation(
            command,
            &[("--spell-id", "42"), ("--key-file", keys.to_str().unwrap())],
        );
        let out = dispatch(&inv).unwrap();
        assert!(!out.complete);
        assert_eq!(out.data["partial"], true);
        assert_eq!(
            out.data["partialReasons"],
            json!(["encrypted_sections_unavailable"])
        );
        assert_eq!(out.data["truncated"], false);
        let proof = &out.data["tables"][0];
        assert_eq!(proof["logicalRows"], 1);
        assert_eq!(proof["contentVerified"], false);
        assert_eq!(proof["missing"][0]["bytes"], 8);
        assert_eq!(proof["unavailablePartitions"][0]["index"], 1);
        assert_eq!(
            proof["unavailablePartitions"][0]["keyID"],
            "1234567890abcdef"
        );
        assert!(proof["partialContent"]["sha256"].is_string());
        assert_eq!(
            out.data["tableVerification"][0]["contentWholeDecodedCKey"],
            false
        );
        if command == "data spell auras" {
            assert_eq!(out.data["hasAura"], json!([42]));
            assert_eq!(out.data["relations"][0]["fromID"], 1);
        } else {
            assert_eq!(out.data["count"], 0);
        }
        fx.verify(&out);
    }
    std::fs::remove_dir_all(fx.root).unwrap();
}
