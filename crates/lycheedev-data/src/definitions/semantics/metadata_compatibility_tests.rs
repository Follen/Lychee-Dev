use super::*;
use std::time::Duration;

fn budget(limits: Cost) -> Budget {
    Budget::new(limits, Instant::now() + Duration::from_secs(30), None)
}

fn rows(raw: &[u8], build: &str) -> Vec<Vec<Scalar>> {
    parse_values(raw, build, &budget(Cost::defaults())).unwrap()
}

fn named(value: u64, name: &str) -> Vec<Scalar> {
    vec![
        Scalar::UInt(value),
        Scalar::String(name.into()),
        Scalar::String(String::new()),
    ]
}

#[test]
fn metadata_names_preserve_internal_whitespace_comments_and_optional_names() {
    assert_eq!(
        rows(
            " 1 Human Male \r\n2\tNight  Elf\tFemale?//near // tail\n3 // unnamed\n\
             4   \n-1 Signed value\n0x8000000000000000 High bit\n5 多词 名称?\n\
             1 Duplicate value\n"
                .as_bytes(),
            "12.1.0.69933",
        ),
        vec![
            named(1, "Human Male"),
            vec![
                Scalar::UInt(2),
                Scalar::String("Night  Elf\tFemale?".into()),
                Scalar::String("near // tail".into())
            ],
            vec![
                Scalar::UInt(3),
                Scalar::Null,
                Scalar::String("unnamed".into())
            ],
            vec![Scalar::UInt(4), Scalar::Null, Scalar::String(String::new())],
            vec![
                Scalar::Int(-1),
                Scalar::String("Signed value".into()),
                Scalar::String(String::new())
            ],
            named(1 << 63, "High bit"),
            named(5, "多词 名称?"),
            named(1, "Duplicate value"),
        ]
    );
}

#[test]
fn metadata_all_fixed_official_presets_keep_inclusive_component_boundaries() {
    // Independent fixed official BuildRange.cs presetRanges, not output produced
    // by matches_metadata_build. Contains checks dimensions 0, 1 and 3 only.
    let presets: [(&str, [u32; 4], [u32; 4]); 12] = [
        ("Pre", [0, 3, 4, 2953], [0, 12, 0, 3988]),
        ("Vanilla", [1, 0, 0, 3980], [1, 12, 3, 6141]),
        ("TBC", [2, 0, 0, 5610], [2, 4, 3, 8606]),
        ("WotLK", [3, 0, 0, 8120], [3, 3, 5, 12340]),
        ("Cata", [4, 0, 0, 11927], [4, 3, 4, 15595]),
        ("MoP", [5, 0, 1, 15464], [5, 4, 8, 18414]),
        ("WoD", [6, 0, 1, 18125], [6, 2, 4, 21742]),
        ("Legion", [7, 0, 1, 20740], [7, 3, 5, 26972]),
        ("BfA", [8, 0, 1, 25703], [8, 3, 7, 35662]),
        ("SL", [9, 0, 1, 33411], [9, 2, 7, 45745]),
        ("DF", [10, 0, 0, 43342], [10, 2, 7, 55664]),
        ("TWW", [11, 0, 0, 54311], [11, 2, 7, 65299]),
    ];
    for (preset, low, high) in presets {
        let raw = format!("(BUILD {preset}) 7 Named value\n");
        let mut cases = vec![(low, true), (high, true)];
        cases.extend([
            ([low[0], low[1], low[2], low[3] - 1], false),
            ([high[0], high[1], high[2], high[3] + 1], false),
            ([low[0], low[1], u32::MAX, low[3]], true),
            ([high[0], high[1] + 1, high[2], high[3]], false),
            ([high[0] + 1, high[1], high[2], high[3]], false),
        ]);
        if low[1] > 0 {
            cases.push(([low[0], low[1] - 1, low[2], low[3]], false));
        }
        for (version, expected) in cases {
            let build = format!(
                "{}.{}.{}.{}",
                version[0], version[1], version[2], version[3]
            );
            assert_eq!(
                rows(raw.as_bytes(), &build),
                if expected {
                    vec![named(7, "Named value")]
                } else {
                    vec![]
                },
                "{preset}: {build}",
            );
        }
    }
}

#[test]
fn metadata_presets_are_not_products_and_do_not_change_numeric_dbd_selectors() {
    for unknown in [
        "tbc",
        "Tbc",
        "WOTLK",
        "Midnight",
        "TLT",
        "retail",
        "classic",
        "anniversary",
        "titan",
        "forever",
        "wow_classic_beta",
        "wow_cn_beta",
    ] {
        let raw = format!("(BUILD {unknown}) 1 Valid name");
        assert_eq!(
            parse_values(raw.as_bytes(), "12.1.0.69933", &budget(Cost::defaults()))
                .unwrap_err()
                .code,
            "schema.invalid_definition",
            "{unknown}",
        );
    }
    assert_eq!(
        schema::matches_build("2.4.3.8606", "TBC").unwrap_err().code,
        "schema.invalid_definition",
    );
    // Official preset Contains is not a four-part lexicographic range. Keep
    // the established ordinary numeric range behavior instead of changing DBD.
    for (build, preset_expected, numeric_expected) in [
        ("2.2.0.9000", false, true),
        ("2.4.99.8606", true, false),
        ("2.2.0.5000", false, true),
        ("2.5.0.8000", false, false),
    ] {
        assert_eq!(
            matches_metadata_build(build, "TBC").unwrap(),
            preset_expected
        );
        assert_eq!(
            matches_metadata_build(build, "2.0.0.5610-2.4.3.8606").unwrap(),
            numeric_expected,
        );
        assert_eq!(
            schema::matches_build(build, "2.0.0.5610-2.4.3.8606").unwrap(),
            numeric_expected,
        );
    }
}

#[test]
fn metadata_build_lists_validate_every_item_and_keep_unselected_body_filtering() {
    assert_eq!(
        rows(
            b"(BUILD TBC, 12.1.0.69933, DF) 1 Current name\n\
              (BUILD TBC) invalid skipped tokens\n\
              (BUILD 12.1.0.69932-12.1.0.69934, WotLK) 2 Ranged name\n",
            "12.1.0.69933",
        ),
        vec![named(1, "Current name"), named(2, "Ranged name")],
    );
    for constraints in [
        "",
        "Unknown",
        "TBC,",
        ",TBC",
        "TBC,,DF",
        "TBC, Unknown",
        "Unknown, TBC",
        "TBC-WotLK",
        "TBC, 12.1.0",
        "12.1.0.69933-",
        "12.1.0.69934-12.1.0.69933",
        "12.1.0.69933-12.1.0.69934-12.1.0.69935",
        "12.1.0.69933, Unknown",
    ] {
        let raw = format!("(BUILD {constraints}) 1 Name");
        for build in ["2.4.3.8606", "12.1.0.69933"] {
            assert_eq!(
                parse_values(raw.as_bytes(), build, &budget(Cost::defaults()))
                    .unwrap_err()
                    .code,
                "schema.invalid_definition",
                "{build}: {constraints}",
            );
        }
    }
    for raw in [
        "(BOGUS TBC) 1 Name",
        "(BUILD TBC",
        "(BUILD TBC)",
        "(BUILD TBC) not-a-number Valid Name",
        "(BUILD TBC) 0x Bad Name",
        "(BUILD TBC) 18446744073709551616 Too Big",
    ] {
        assert_eq!(
            parse_values(raw.as_bytes(), "2.4.3.8606", &budget(Cost::defaults()))
                .unwrap_err()
                .code,
            "records.definition_identity",
            "{raw}",
        );
    }
    // Unqualified values never consulted build syntax in the old contract.
    assert_eq!(
        rows(b"1 Named value", "not a full build"),
        vec![named(1, "Named value")]
    );
}

#[test]
fn metadata_official_weather_preserves_historical_duplicates_and_source_order() {
    let weather =
        include_bytes!("../../../tests/fixtures/metadata-official-b9a6309/WeatherType.dbde");
    let current = vec![
        named(0, "Clear"),
        named(1, "Rain"),
        named(2, "Snow"),
        named(3, "Sandstorm"),
        named(4, "Miscellaneous"),
        named(5, "Fire"),
    ];
    let mut historical = vec![
        named(0, "None"),
        named(1, "Clear"),
        named(2, "Rain"),
        named(3, "Snow"),
        named(4, "Sandstorm"),
        named(5, "Miscellaneous"),
    ];
    historical.extend(current.clone());
    for build in [
        "2.0.0.5610",
        "2.4.3.8606",
        "3.3.5.12340",
        "10.2.7.55664",
        "11.0.0.54311",
        "11.1.7.61967",
    ] {
        assert_eq!(rows(weather, build), historical, "{build}");
    }
    for build in ["1.12.3.6141", "11.1.7.61968", "12.1.0.69933"] {
        assert_eq!(rows(weather, build), current, "{build}");
    }
}

#[test]
fn metadata_extended_syntax_keeps_budget_and_independent_control_admission() {
    let raw = b"(BUILD TBC, 12.1.0.69933) 1 First name\n2 Second name\n";
    let first_owned = "First name".len() as u64;
    let ledger = budget(Cost {
        retained_bytes: raw.len() as u64 + 160 + first_owned,
        ..Cost::defaults()
    });
    assert_eq!(
        parse_values(raw, "12.1.0.69933", &ledger).unwrap_err().code,
        "records.query_resource_budget",
    );
    assert_eq!(ledger.snapshot().unwrap().decode_work, 1);
    assert_eq!(
        ledger.snapshot().unwrap().retained_bytes,
        raw.len() as u64 + 160 + first_owned
    );
    let ledger = budget(Cost::defaults());
    let mut checks = 0;
    let error = parse_values_with_check(raw, "12.1.0.69933", &ledger, || {
        checks += 1;
        if checks == 3 {
            Err(fault("fixture.invocation", 7, "invocation stopped"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "fixture.invocation");
    assert_eq!(checks, 3);
    assert_eq!(ledger.snapshot().unwrap().decode_work, 1);
    assert!(ledger.check().is_ok());
    assert_eq!(
        rows(raw, "12.1.0.69933"),
        vec![named(1, "First name"), named(2, "Second name")]
    );
}

#[test]
fn metadata_owned_name_and_comment_utf8_bytes_are_admitted_before_allocation() {
    let raw = "1 多词 名称? // 注释 中文\n".as_bytes();
    // These independent literal strings exclude delimiters and trimmed space;
    // byte counts deliberately differ from character counts.
    let owned = "多词 名称?".len() as u64 + "注释 中文".len() as u64;
    let exact = raw.len() as u64 + 160 + owned;
    for limit in [raw.len() as u64 + 160, exact - 1] {
        let ledger = budget(Cost {
            retained_bytes: limit,
            ..Cost::defaults()
        });
        assert_eq!(
            parse_values(raw, "12.1.0.69933", &ledger).unwrap_err().code,
            "records.query_resource_budget",
        );
        assert_eq!(ledger.snapshot().unwrap().decode_work, 0);
        assert_eq!(ledger.snapshot().unwrap().retained_bytes, raw.len() as u64);
        assert_eq!(ledger.logical_snapshot().unwrap().decode_work, 0);
    }
    let ledger = budget(Cost {
        retained_bytes: exact,
        ..Cost::defaults()
    });
    assert_eq!(
        parse_values(raw, "12.1.0.69933", &ledger).unwrap(),
        vec![vec![
            Scalar::UInt(1),
            Scalar::String("多词 名称?".into()),
            Scalar::String("注释 中文".into()),
        ]],
    );
    assert_eq!(ledger.snapshot().unwrap().retained_bytes, exact);
    assert_eq!(
        ledger.logical_snapshot().unwrap().retained_bytes,
        raw.len() as u64 + 160
    );
    assert_eq!(
        ledger.snapshot().unwrap().metadata_bytes,
        raw.len() as u64 + 160
    );
    assert_eq!(ledger.snapshot().unwrap().decode_work, 1);
}
