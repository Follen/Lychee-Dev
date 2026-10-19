use super::*;
use lycheedev_core::control::OperationControl;

// Keep the entire old producer here. In particular, neither expected input
// bytes nor expected IDs may come from SymbolIdentity or the public helper.
fn historical_id(pin: &SourcePin, symbol: &Symbol) -> String {
    format!(
        "SYM-{}",
        &storage::sha256(
            format!(
                "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
                pin.repository,
                pin.exact_commit,
                symbol.path,
                symbol.line,
                symbol.end_line,
                symbol.name,
                symbol.category,
                symbol.scope,
                symbol.ordinal
            )
            .as_bytes()
        )[..32]
    )
}

fn sample() -> (SourcePin, Symbol) {
    (
        SourcePin {
            repository: "elvui".into(),
            product: "main".into(),
            requested_ref: "fixture".into(),
            exact_commit: "a".repeat(40),
            parser_revision: git::PARSER.into(),
        },
        Symbol {
            id: String::new(),
            kind: "declaration".into(),
            name: "Run".into(),
            target: String::new(),
            category: "function".into(),
            confidence: "exact".into(),
            path: "Addon/Main.lua".into(),
            line: 12,
            end_line: 34,
            signature: "Run(value)".into(),
            scope: "Widget".into(),
            ordinal: 5,
        },
    )
}

fn assert_identity(pin: &SourcePin, symbol: &Symbol) -> String {
    let expected = historical_id(pin, symbol);
    assert_eq!(stable_id(pin, symbol), expected);
    assert_eq!(
        SymbolIdentity::for_document(pin, &symbol.path).stable_id(symbol),
        expected
    );
    assert_eq!(expected.len(), 36);
    assert!(expected.starts_with("SYM-"));
    assert!(
        expected[4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    expected
}

#[test]
fn stable_id_matches_complete_historical_format_and_fixed_vectors() {
    let (mut pin, mut symbol) = sample();
    // Independently calculated SHA-256 vectors also fix the truncation end,
    // hexadecimal alphabet and literal prefix, not only the shared hash API.
    assert_eq!(
        assert_identity(&pin, &symbol),
        "SYM-c0efdd111d1017aff6d2cd73a4511fa0"
    );
    pin.repository.clear();
    pin.exact_commit.clear();
    symbol.path.clear();
    symbol.name.clear();
    symbol.category.clear();
    symbol.scope.clear();
    symbol.line = 0;
    symbol.end_line = 0;
    symbol.ordinal = 0;
    assert_eq!(
        assert_identity(&pin, &symbol),
        "SYM-629aa26d2881abb0e4b61370935865c3"
    );
}

#[test]
fn stable_id_preserves_each_identity_field_and_field_order() {
    let (pin, symbol) = sample();
    let original = historical_id(&pin, &symbol);
    for field in 0..9 {
        let (mut changed_pin, mut changed) = (pin.clone(), symbol.clone());
        match field {
            0 => changed_pin.repository.push('!'),
            1 => changed_pin.exact_commit.push('!'),
            2 => changed.path.push('!'),
            3 => changed.line += 1,
            4 => changed.end_line += 1,
            5 => changed.name.push('!'),
            6 => changed.category.push('!'),
            7 => changed.scope.push('!'),
            8 => changed.ordinal += 1,
            _ => unreachable!(),
        }
        assert_ne!(
            assert_identity(&changed_pin, &changed),
            original,
            "field {field}"
        );
    }
    let mut swapped_pin = pin.clone();
    std::mem::swap(&mut swapped_pin.repository, &mut swapped_pin.exact_commit);
    assert_ne!(assert_identity(&swapped_pin, &symbol), original);
    let mut swapped = symbol.clone();
    std::mem::swap(&mut swapped.line, &mut swapped.end_line);
    assert_ne!(assert_identity(&pin, &swapped), original);
    let mut swapped = symbol.clone();
    std::mem::swap(&mut swapped.name, &mut swapped.category);
    assert_ne!(assert_identity(&pin, &swapped), original);
    let mut swapped = symbol;
    std::mem::swap(&mut swapped.category, &mut swapped.scope);
    assert_ne!(assert_identity(&pin, &swapped), original);
}

#[test]
fn stable_id_excludes_every_nonidentity_pin_and_symbol_field() {
    let (pin, symbol) = sample();
    let original = historical_id(&pin, &symbol);
    for field in 0..8 {
        let (mut changed_pin, mut changed) = (pin.clone(), symbol.clone());
        let replacement = "雪\0different\n".to_owned();
        match field {
            0 => changed_pin.product = replacement,
            1 => changed_pin.requested_ref = replacement,
            2 => changed_pin.parser_revision = replacement,
            3 => changed.id = replacement,
            4 => changed.kind = replacement,
            5 => changed.target = replacement,
            6 => changed.confidence = replacement,
            7 => changed.signature = replacement,
            _ => unreachable!(),
        }
        assert_eq!(
            assert_identity(&changed_pin, &changed),
            original,
            "field {field}"
        );
    }
}

#[test]
fn stable_id_preserves_utf8_embedded_nul_and_historical_delimiter_ambiguity() {
    let (pin, symbol) = sample();
    for value in ["", "雪é😀", "\0", "before\0after", "\n\r\t\\\"", "e\u{301}"] {
        for field in 0..6 {
            let (mut changed_pin, mut changed) = (pin.clone(), symbol.clone());
            match field {
                0 => changed_pin.repository = value.into(),
                1 => changed_pin.exact_commit = value.into(),
                2 => changed.path = value.into(),
                3 => changed.name = value.into(),
                4 => changed.category = value.into(),
                5 => changed.scope = value.into(),
                _ => unreachable!(),
            }
            assert_identity(&changed_pin, &changed);
        }
    }
    let mut composed = symbol.clone();
    composed.name = "é".into();
    let mut decomposed = composed.clone();
    decomposed.name = "e\u{301}".into();
    assert_ne!(
        assert_identity(&pin, &composed),
        assert_identity(&pin, &decomposed)
    );
    let mut changed_path = symbol.clone();
    changed_path.path = "addon\\Main.lua".into();
    assert_ne!(
        assert_identity(&pin, &changed_path),
        historical_id(&pin, &symbol)
    );

    // NUL is a raw delimiter, not escaping or length framing. These existing
    // equal byte streams must remain equal; this is no new uniqueness claim.
    let (mut first_pin, mut second_pin) = (pin.clone(), pin.clone());
    first_pin.repository = "repo\0part".into();
    first_pin.exact_commit = "commit".into();
    second_pin.repository = "repo".into();
    second_pin.exact_commit = "part\0commit".into();
    assert_eq!(
        assert_identity(&first_pin, &symbol),
        assert_identity(&second_pin, &symbol)
    );
    let (mut first, mut second) = (symbol.clone(), symbol);
    first.name = "name\0part".into();
    first.category = "category".into();
    second.name = "name".into();
    second.category = "part\0category".into();
    assert_eq!(
        assert_identity(&pin, &first),
        assert_identity(&pin, &second)
    );
}

#[test]
fn stable_id_formats_usize_decimal_at_every_width_boundary() {
    let (pin, symbol) = sample();
    let mut values = BTreeSet::from([0, 1, usize::MAX - 1, usize::MAX]);
    let mut power = 10usize;
    loop {
        values.extend([power - 1, power, power + 1]);
        let Some(next) = power.checked_mul(10) else {
            break;
        };
        power = next;
    }
    for value in values {
        for field in 0..3 {
            let mut changed = symbol.clone();
            match field {
                0 => changed.line = value,
                1 => changed.end_line = value,
                2 => changed.ordinal = value,
                _ => unreachable!(),
            }
            assert_identity(&pin, &changed);
        }
        let mut changed = symbol.clone();
        changed.line = value;
        changed.end_line = usize::MAX - value;
        changed.ordinal = value;
        assert_identity(&pin, &changed);
    }
}

#[test]
fn symbol_identity_reuses_prefix_across_hash_blocks_order_and_duplicates() {
    for prefix_bytes in [5, 55, 56, 63, 64, 65, 119, 120, 127, 128, 129, 1024] {
        let (mut pin, mut symbol) = sample();
        pin.repository = "r".into();
        pin.exact_commit = "c".into();
        symbol.path = "p".repeat(prefix_bytes - 5);
        assert_eq!(
            format!(
                "{}\0{}\0{}\0",
                pin.repository, pin.exact_commit, symbol.path
            )
            .len(),
            prefix_bytes
        );
        let identity = SymbolIdentity::for_document(&pin, &symbol.path);
        let mut other_document = symbol.clone();
        other_document.path.push('q');
        let other_identity = SymbolIdentity::for_document(&pin, &other_document.path);
        for suffix_bytes in [0, 1, 55, 56, 63, 64, 65, 127, 128, 129, 1024] {
            symbol.name = "n".repeat(suffix_bytes);
            let expected = historical_id(&pin, &symbol);
            let mut second = symbol.clone();
            second.scope = "雪\0scope".into();
            second.ordinal = usize::MAX;
            let second_expected = historical_id(&pin, &second);
            // Revisit exact duplicates after different symbols and an unrelated
            // document. A mutable accumulated suffix would fail this ordering.
            for (row, wanted) in [
                (&symbol, &expected),
                (&second, &second_expected),
                (&symbol, &expected),
                (&second, &second_expected),
                (&symbol, &expected),
            ] {
                assert_eq!(identity.stable_id(row), *wanted);
                assert_eq!(stable_id(&pin, row), *wanted);
                assert_eq!(
                    other_identity.stable_id(&other_document),
                    historical_id(&pin, &other_document)
                );
            }
        }
    }
    // Independently target complete-message lengths, including the 56-byte
    // padding transition. Prefix boundaries alone do not prove final padding.
    let (mut pin, mut symbol) = sample();
    pin.repository = "r".into();
    pin.exact_commit = "c".into();
    symbol.path.clear();
    symbol.line = 0;
    symbol.end_line = 0;
    symbol.ordinal = 0;
    symbol.category.clear();
    symbol.scope.clear();
    let identity = SymbolIdentity::for_document(&pin, &symbol.path);
    for total_bytes in [55, 56, 63, 64, 65, 119, 120, 127, 128, 129, 1024] {
        symbol.name = "n".repeat(total_bytes - 13);
        let complete = format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
            pin.repository,
            pin.exact_commit,
            symbol.path,
            symbol.line,
            symbol.end_line,
            symbol.name,
            symbol.category,
            symbol.scope,
            symbol.ordinal
        );
        assert_eq!(complete.len(), total_bytes);
        assert_eq!(identity.stable_id(&symbol), historical_id(&pin, &symbol));
        assert_identity(&pin, &symbol);
    }
}

// Rebuild complete ordered public rows from the source bytes and ordinary
// parser, never from the candidate Index or its precomputed IDs. This is the
// pre-optimization assembly, including empty IDs for non-declarations.
fn historical_symbols(pin: &SourcePin, sources: &BTreeMap<String, String>) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for (path, raw) in sources {
        let parsed = facts::ParseMode::for_path(path).parse(raw.as_bytes());
        assert!(
            parsed.diagnostics.is_empty(),
            "fixture diagnostics in {path}"
        );
        for (ordinal, declaration) in parsed.declarations.into_iter().enumerate() {
            let mut symbol = Symbol {
                id: String::new(),
                kind: "declaration".into(),
                name: declaration.name,
                target: String::new(),
                category: declaration.category,
                confidence: "exact".into(),
                path: path.clone(),
                line: declaration.line,
                end_line: declaration.end_line,
                signature: declaration.signature,
                scope: declaration.scope,
                ordinal,
            };
            symbol.id = historical_id(pin, &symbol);
            symbols.push(symbol);
        }
        for relationship in parsed.relationships {
            symbols.push(Symbol {
                id: String::new(),
                kind: "relationship".into(),
                name: relationship.from,
                target: relationship.to,
                category: relationship.category,
                confidence: relationship.confidence,
                path: path.clone(),
                line: relationship.line,
                end_line: relationship.line,
                signature: String::new(),
                scope: String::new(),
                ordinal: 0,
            });
        }
        for load in parsed.loads {
            symbols.push(Symbol {
                id: String::new(),
                kind: "load".into(),
                name: path.clone(),
                target: load.path,
                category: load.kind,
                confidence: "exact".into(),
                path: path.clone(),
                line: load.line,
                end_line: load.line,
                signature: String::new(),
                scope: String::new(),
                ordinal: 0,
            });
        }
        for header in parsed.headers {
            symbols.push(Symbol {
                id: String::new(),
                kind: "header".into(),
                name: header.key,
                target: header.value,
                category: "toc-field".into(),
                confidence: "exact".into(),
                path: path.clone(),
                line: header.line,
                end_line: header.line,
                signature: String::new(),
                scope: String::new(),
                ordinal: 0,
            });
        }
    }
    symbols
}

fn assert_aggregate(index: &Index, pin: &SourcePin, sources: &BTreeMap<String, String>) {
    let expected = historical_symbols(pin, sources);
    assert_eq!(
        serde_json::to_value(&index.symbols).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(
        index
            .documents
            .iter()
            .map(|document| &document.path)
            .collect::<Vec<_>>(),
        sources.keys().collect::<Vec<_>>()
    );
    assert_eq!(index.summary.complete, true);
    assert!(!expected.is_empty());
    let declarations = expected
        .iter()
        .filter(|symbol| symbol.kind == "declaration")
        .count();
    assert!(declarations > 0);
    assert_eq!(index.summary.declarations, declarations);
    for document in &index.documents {
        let rows = index
            .symbols
            .iter()
            .filter(|symbol| symbol.path == document.path && symbol.kind == "declaration");
        for (ordinal, symbol) in rows.enumerate() {
            assert_eq!(symbol.ordinal, ordinal);
            assert_eq!(symbol.id, historical_id(pin, symbol));
        }
    }
    assert!(
        index
            .symbols
            .iter()
            .filter(|symbol| symbol.kind != "declaration")
            .all(|symbol| symbol.id.is_empty())
    );
}

#[test]
fn build_uncached_cold_and_facts_hit_match_historical_symbol_aggregation() {
    let fixture = facts_cache_tests::Fixture::new();
    let control = OperationControl::default();
    let sources = BTreeMap::from([
        ("AEmpty.lua".into(), String::new()),
        ("Main.lua".into(), "function Run() Other() end\nfunction Run() end\n".into()),
        ("Main.toc".into(), "## Interface: 120100\n## Title: 雪é\nMain.lua\nView.xml\n".into()),
        ("View.xml".into(), "<Ui><Frame name=\"Repeated\"/><Frame name=\"Repeated\"/><Script file=\"Main.lua\"/></Ui>\n".into()),
        ("目录/雪.xml".into(), "<Ui><Frame name=\"雪é\" inherits=\"Base\"/></Ui>\n".into()),
    ]);
    let pin = fixture.pin(&sources);
    let cold = build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(cold.summary.io["parsedDocuments"], sources.len());
    assert_eq!(cold.summary.io["cachedFacts"], 0);
    assert_aggregate(&cold, &pin, &sources);
    for kind in ["declaration", "relationship", "load", "header"] {
        assert!(
            cold.symbols.iter().any(|symbol| symbol.kind == kind),
            "missing fixture {kind}"
        );
    }
    let repeated = cold
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.path == "View.xml" && symbol.kind == "declaration" && symbol.name == "Repeated"
        })
        .collect::<Vec<_>>();
    assert_eq!(repeated.len(), 2);
    assert_eq!(repeated[0].line, repeated[1].line);
    assert_eq!(repeated[0].end_line, repeated[1].end_line);
    assert_ne!(repeated[0].ordinal, repeated[1].ordinal);
    assert_ne!(repeated[0].id, repeated[1].id);
    let hit = build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(hit.summary.io["parsedDocuments"], 0);
    assert_eq!(hit.summary.io["cachedFacts"], sources.len());
    assert_aggregate(&hit, &pin, &sources);
    assert_eq!(
        serde_json::to_value(&cold.symbols).unwrap(),
        serde_json::to_value(&hit.symbols).unwrap()
    );
}

#[test]
fn build_uncached_incremental_mutations_rekey_cached_and_fresh_symbols() {
    let fixture = facts_cache_tests::Fixture::new();
    let control = OperationControl::default();
    // One complete immutable 64-document cohort must really hit after every
    // change in the tail cohort. A tiny fixture could reparse its only pack and
    // would not establish that cached Facts receive the new commit's IDs.
    let mut sources: BTreeMap<String, String> = (0..64)
        .map(|ordinal| {
            (
                format!("A{ordinal:03}.xml"),
                "<Ui><Frame name=\"Repeated\"/><Frame name=\"Repeated\"/></Ui>\n".into(),
            )
        })
        .collect();
    sources.insert("ZMutable.lua".into(), "function Before() end\n".into());
    let mut pin = fixture.pin(&sources);
    let mut previous = build_uncached(&fixture.home, &pin, &control).unwrap();
    assert_eq!(previous.summary.io["parsedDocuments"], 65);
    assert_eq!(previous.summary.io["cachedFacts"], 0);
    assert_aggregate(&previous, &pin, &sources);
    for change in ["modify", "insert", "delete", "rename"] {
        match change {
            "modify" => {
                sources.insert(
                    "ZMutable.lua".into(),
                    "function After() end\nfunction Extra() end\n".into(),
                );
            }
            "insert" => {
                sources.insert(
                    "ZInserted.xml".into(),
                    "<Ui><Frame name=\"Inserted\"/></Ui>\n".into(),
                );
            }
            "delete" => {
                sources.remove("ZMutable.lua").unwrap();
            }
            "rename" => {
                let raw = sources.remove("ZInserted.xml").unwrap();
                sources.insert("ZRenamed.xml".into(), raw);
            }
            _ => unreachable!(),
        }
        let next_pin = fixture.pin(&sources);
        assert_ne!(next_pin.exact_commit, pin.exact_commit, "{change}");
        let next = build_uncached(&fixture.home, &next_pin, &control).unwrap();
        assert_eq!(next.summary.io["cachedFacts"], 64, "{change}");
        assert_eq!(
            next.summary.io["parsedDocuments"],
            sources.len() - 64,
            "{change}"
        );
        assert_aggregate(&next, &next_pin, &sources);
        let old_rows = previous
            .symbols
            .iter()
            .filter(|symbol| symbol.path == "A000.xml" && symbol.kind == "declaration")
            .collect::<Vec<_>>();
        let new_rows = next
            .symbols
            .iter()
            .filter(|symbol| symbol.path == "A000.xml" && symbol.kind == "declaration")
            .collect::<Vec<_>>();
        assert_eq!(old_rows.len(), 2);
        assert_eq!(new_rows.len(), 2);
        for (old, new) in old_rows.iter().zip(new_rows) {
            assert_eq!(old.name, new.name);
            assert_eq!(old.ordinal, new.ordinal);
            assert_ne!(
                old.id, new.id,
                "cached Facts retained prior commit ID after {change}"
            );
        }
        previous = next;
        pin = next_pin;
    }
}
