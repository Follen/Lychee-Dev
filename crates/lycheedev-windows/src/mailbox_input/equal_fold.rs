//! Pinned Go strings.EqualFold semantics for Windows UTF-16 path observations.
#![forbid(unsafe_code)]
#[path = "equal_fold_data.rs"]
mod data;

fn representative(value: char) -> u32 {
    let code = value as u32;
    match data::FOLD_REPRESENTATIVES.binary_search_by_key(&code, |entry| entry.0) {
        Ok(index) => data::FOLD_REPRESENTATIVES[index].1,
        Err(_) => code,
    }
}
/// Compare already-delimited UTF-16 slices using the pinned Go simple-fold
/// relation. Callers supply string contents without a trailing NUL terminator.
/// This applies neither full case folding nor Unicode normalization.
pub fn equal_fold_utf16(left: &[u16], right: &[u16]) -> bool {
    // Go windows.UTF16ToString decodes ill-formed units as RuneError too.
    // Length-changing full folds and canonical normalization are not applied.
    char::decode_utf16(left.iter().copied())
        .map(|value| representative(value.unwrap_or(char::REPLACEMENT_CHARACTER)))
        .eq(char::decode_utf16(right.iter().copied())
            .map(|value| representative(value.unwrap_or(char::REPLACEMENT_CHARACTER))))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn same(left: &str, right: &str) -> bool {
        equal_fold_utf16(
            &left.encode_utf16().collect::<Vec<_>>(),
            &right.encode_utf16().collect::<Vec<_>>(),
        )
    }
    #[test]
    fn simple_folds_match_unicode_special_orbits_without_normalizing() {
        for (a, b) in [
            ("C:\\WOW.EXE", "c:\\wow.exe"),
            ("K", "K"),
            ("S", "ſ"),
            ("Σ", "ς"),
            ("σ", "ς"),
            ("ß", "ẞ"),
            ("Μ", "µ"),
            ("Å", "Å"),
            ("魔兽世界", "魔兽世界"),
        ] {
            assert!(same(a, b), "{a:?}/{b:?}");
            assert!(same(b, a), "reverse {a:?}/{b:?}");
        }
        for (a, b) in [
            ("ß", "SS"),
            ("İ", "i"),
            ("I", "ı"),
            ("İ", "I"),
            ("é", "e\u{301}"),
            ("ﬀ", "ff"),
            ("a", "aa"),
            ("a", ""),
        ] {
            assert!(!same(a, b), "{a:?}/{b:?}");
            assert!(!same(b, a), "reverse {a:?}/{b:?}");
        }
    }
    #[test]
    fn invalid_utf16_matches_rune_error_and_generated_table_is_canonical() {
        assert!(equal_fold_utf16(&[0xd800], &[0xfffd]));
        assert!(!equal_fold_utf16(&[0xd800, 0xd800], &[0xfffd]));
        assert!(
            data::FOLD_REPRESENTATIVES
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0)
        );
        for &(source, canonical) in data::FOLD_REPRESENTATIVES {
            let source = char::from_u32(source).unwrap();
            let canonical = char::from_u32(canonical).unwrap();
            assert_eq!(representative(source), representative(canonical));
            assert!(canonical < source);
        }
    }
}
