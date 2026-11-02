use super::*;
use std::sync::Arc;

#[test]
fn shared_indexed_keeps_original_enum_size_and_alignment() {
    // Freeze the previous physical representation. A new target must fail this
    // gate rather than silently increase every producer's accounting unit.
    #[allow(dead_code)]
    enum OriginalLayout {
        Verbatim,
        Binary64,
        Exact(JsonNumber),
        Object(BTreeMap<String, OriginalLayout>),
        Optional(Box<OriginalLayout>),
        Array(Box<OriginalLayout>),
        Indexed(BTreeMap<usize, OriginalLayout>),
    }
    assert_eq!(size_of::<NumberLayout>(), size_of::<OriginalLayout>());
    assert_eq!(align_of::<NumberLayout>(), align_of::<OriginalLayout>());
}

fn render(value: &Value, layout: &NumberLayout, pretty: bool) -> (Vec<u8>, Option<String>) {
    let mut out = Vec::new();
    let result = if pretty {
        write_pretty_line(&mut out, value, layout)
    } else {
        write_line(&mut out, value, layout)
    };
    (out, result.err().map(|error| error.to_string()))
}

#[test]
fn shared_indexed_matches_owned_bytes_bounds_and_error_precedence() {
    let cells = BTreeMap::from([(1, NumberLayout::Binary64), (3, NumberLayout::Binary64)]);
    for raw in [
        "[[9007199254740993,-0.0,18446744073709551615,1e21]]",
        "[[null,1.0,\"text\",1e-7],[true,2.0]]",
        "[[0,\"wrong\",0,1e400]]",
        "[[0,1.0]]",
        "[[0,null,0,1.0]]",
        "[]",
        "{}",
        "null",
    ] {
        let value: Value = serde_json::from_str(raw).unwrap();
        for marked_row in [0, 2] {
            let owned = NumberLayout::Indexed(BTreeMap::from([(
                marked_row,
                NumberLayout::Indexed(cells.clone()),
            )]));
            let shared = NumberLayout::Indexed(BTreeMap::from([(
                marked_row,
                NumberLayout::SharedIndexed(Arc::new(cells.clone())),
            )]));
            for pretty in [false, true] {
                let old = render(&value, &owned, pretty);
                let new = render(&value, &shared, pretty);
                assert_eq!(new, old, "{raw}, row {marked_row}, pretty {pretty}");
                if new.1.is_some() {
                    assert!(new.0.is_empty(), "preflight must reject before writing");
                }
            }
        }
    }
    let value = serde_json::json!([["wrong"]]);
    let both_invalid = NumberLayout::Indexed(BTreeMap::from([
        (
            0,
            NumberLayout::SharedIndexed(Arc::new(BTreeMap::from([(0, NumberLayout::Binary64)]))),
        ),
        (
            2,
            NumberLayout::SharedIndexed(Arc::new(BTreeMap::from([(99, NumberLayout::Binary64)]))),
        ),
    ]));
    let (out, error) = render(&value, &both_invalid, false);
    assert!(out.is_empty());
    assert!(
        error
            .unwrap()
            .contains("public number layout index exceeds result length")
    );
}

#[test]
fn shared_indexed_clone_keeps_owner_until_last_layout_drops() {
    let cells = Arc::new(BTreeMap::from([(0, NumberLayout::Binary64)]));
    let weak = Arc::downgrade(&cells);
    let first = NumberLayout::SharedIndexed(cells.clone());
    let second = first.clone();
    assert_eq!(Arc::strong_count(&cells), 3);
    drop(cells);
    drop(first);
    assert!(weak.upgrade().is_some());
    drop(second);
    assert!(weak.upgrade().is_none());
}
