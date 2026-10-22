//! Lua source is byte-oriented. Archived bytes, their identity and their byte
//! positions stay unchanged; text views are explicit, separate transforms.
//!
//! - [`display`] is the public U+FFFD view used by query/inspect/context.
//! - [`analysis`] is the private view for engines that require `&str`. It
//!   replaces each invalid byte with one ASCII `?`, so the analysis text has
//!   exactly the original byte length and every byte/line position keeps its
//!   meaning. It is never a public value and never rewrites archived bytes.
//!
//! Invalid bytes in comments are irrelevant to Lua semantics. Invalid bytes in
//! a real token remain invalid syntax, and invalid bytes in a string literal
//! make that engine input unavailable (the caller must not publish a
//! substituted string value).
use std::borrow::Cow;

/// The first invalid UTF-8 byte, located in the original bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Warning {
    /// One-based line in the original bytes.
    pub(crate) line: usize,
    /// Zero-based byte offset in the original bytes.
    pub(crate) byte_offset: usize,
    pub(crate) invalid: u8,
}

impl Warning {
    pub(crate) fn message(&self) -> String {
        format!(
            "codebase.source_encoding: invalid UTF-8 byte 0x{:02x} at byte offset {} (zero-based); original bytes preserved; excerpts display invalid bytes as U+FFFD",
            self.invalid, self.byte_offset
        )
    }
    pub(crate) fn json(&self, path: &str) -> serde_json::Value {
        serde_json::json!({"path":path,"line":self.line,"byteOffset":self.byte_offset,"message":self.message()})
    }
}

/// Locate the first invalid UTF-8 byte without copying or rewriting `raw`.
pub(crate) fn warning(raw: &[u8]) -> Option<Warning> {
    let error = std::str::from_utf8(raw).err()?;
    let at = error.valid_up_to();
    Some(Warning {
        line: raw[..at].iter().filter(|byte| **byte == b'\n').count() + 1,
        byte_offset: at,
        invalid: raw[at],
    })
}

pub(crate) fn is_utf8(raw: &[u8]) -> bool {
    warning(raw).is_none()
}

/// Every invalid byte in `raw`, as zero-based offsets. Invalid sequences yield
/// one entry per byte, matching [`analysis`]'s one-replacement-per-byte view.
pub(crate) fn invalid_offsets(raw: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut rest = 0usize;
    while rest < raw.len() {
        match std::str::from_utf8(&raw[rest..]) {
            Ok(_) => break,
            Err(error) => {
                let at = rest + error.valid_up_to();
                offsets.push(at);
                rest = at + 1;
            }
        }
    }
    offsets
}

/// Public display view: invalid bytes become U+FFFD, every valid byte and line
/// terminator is untouched.
pub(crate) fn display(raw: &[u8]) -> Cow<'_, str> {
    String::from_utf8_lossy(raw)
}

/// Private engine view. One ASCII `?` per invalid byte keeps the byte length
/// identical, so line and byte coordinates of the analysis text are the same as
/// in the original archive.
pub(crate) fn analysis(raw: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(raw) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => {
            let mut bytes = raw.to_vec();
            for offset in invalid_offsets(raw) {
                bytes[offset] = b'?';
            }
            Cow::Owned(String::from_utf8(bytes).expect("every invalid byte was replaced"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_first_invalid_byte_with_line_and_zero_based_offset() {
        let raw = b"function Before() end\r\n-- Night Web\x92s Hollow\r\nfunction After() end\r\n";
        let warning = warning(raw).expect("invalid byte");
        assert_eq!(warning.invalid, 0x92);
        assert_eq!(
            warning.byte_offset,
            raw.iter().position(|byte| *byte == 0x92).unwrap()
        );
        assert_eq!(warning.line, 2);
        assert!(warning.message().contains("0x92"));
        assert!(!is_utf8(raw));
    }

    #[test]
    fn analysis_preserves_length_positions_and_valid_bytes() {
        let raw = b"-- a\x92b\xe2\x28\r\nlocal x = '\xf0\x9f\x99\x82'\n";
        let analysis = analysis(raw);
        assert_eq!(analysis.len(), raw.len());
        assert!(std::str::from_utf8(analysis.as_bytes()).is_ok());
        assert!(analysis.contains("-- a?b?(\r\nlocal x = '\u{1f642}'\n"));
        assert_eq!(
            invalid_offsets(raw).len(),
            analysis.as_bytes().iter().filter(|byte| **byte == b'?').count()
        );
    }

    #[test]
    fn display_uses_replacement_without_changing_valid_or_crlf_bytes() {
        let raw = b"-- a\x92b\r\nnext";
        assert_eq!(display(raw), "-- a\u{fffd}b\r\nnext");
        assert_eq!(display(b"plain\r\n"), "plain\r\n");
    }

    #[test]
    fn bom_and_multibyte_neighbours_keep_byte_positions_in_both_views() {
        let mut bytes = "\u{feff}-- 雪".as_bytes().to_vec();
        bytes.push(0x92);
        bytes.extend_from_slice("雪\r\n".as_bytes());
        let raw = bytes.as_slice();
        let offset = raw.iter().position(|byte| *byte == 0x92).unwrap();
        let warning = warning(raw).unwrap();
        assert_eq!(warning.line, 1);
        assert_eq!(warning.byte_offset, offset);
        // The BOM and multibyte bytes are untouched by the analysis view.
        let analysis = analysis(raw);
        assert_eq!(analysis.len(), raw.len());
        assert_eq!(&analysis.as_bytes()[..3], b"\xef\xbb\xbf");
        assert_eq!(&analysis.as_bytes()[offset..offset + 1], b"?");
        assert_eq!(analysis.matches("雪").count(), 2);
        // Display keeps every valid byte and marks only the invalid one.
        let display = display(raw);
        assert!(display.starts_with('\u{feff}'));
        assert_eq!(display, "\u{feff}-- 雪\u{fffd}雪\r\n");
    }

    #[test]
    fn analysis_is_borrowed_for_valid_input_and_not_used_as_identity() {
        let raw = b"function Ok() end\n";
        assert!(matches!(analysis(raw), Cow::Borrowed(_)));
        assert_eq!(analysis(raw).as_ref(), "function Ok() end\n");
    }
}
