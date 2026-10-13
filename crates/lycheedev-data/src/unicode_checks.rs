use std::collections::BTreeMap;
#[test]
fn every_unicode_scalar_matches_frozen_go17_simple_case() {
    let raw = include_bytes!("../../../tests/rust-source-data/unicode-simple-go17.json");
    let expected: serde_json::Value =
        serde_json::from_slice(raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw)).unwrap();
    assert_eq!(expected["version"], "17.0.0");
    let rows: BTreeMap<u32, (u32, u32)> = expected["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row[0].as_u64().unwrap() as u32,
                (
                    row[1].as_u64().unwrap() as u32,
                    row[2].as_u64().unwrap() as u32,
                ),
            )
        })
        .collect();
    let mut checked = 0;
    for cp in 0..=0x10ffff {
        let Some(c) = char::from_u32(cp) else {
            continue;
        };
        let (upper, lower) = rows.get(&cp).copied().unwrap_or((cp, cp));
        assert_eq!(
            super::unicode_simple::map(c, true) as u32,
            upper,
            "upper {cp:x}"
        );
        assert_eq!(
            super::unicode_simple::map(c, false) as u32,
            lower,
            "lower {cp:x}"
        );
        checked += 1;
    }
    assert_eq!(checked, 1_112_064);
}
