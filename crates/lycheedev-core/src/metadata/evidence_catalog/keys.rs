//! One reserved-key classifier shared by reads, pages and every mutation path.
//! Provenance text is not a storage-domain capability.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EvidenceKey<'a> {
    Capture(&'a str),
    Keep(&'a str),
    Removal(&'a str),
}
impl<'a> EvidenceKey<'a> {
    pub(crate) fn parse(key: &'a str) -> Option<Self> {
        key.strip_prefix("capture/")
            .map(Self::Capture)
            .or_else(|| key.strip_prefix("evidence/keep/").map(Self::Keep))
            .or_else(|| key.strip_prefix("evidence/remove/").map(Self::Removal))
    }
    pub(crate) fn capture_id(self) -> &'a str {
        match self {
            Self::Capture(id) | Self::Keep(id) | Self::Removal(id) => id,
        }
    }
}
/// Empty and broad prefixes intersect evidence too. A narrow starts_with test
/// would silently omit new-store records from generic Metadata::list("").
pub(crate) fn intersects(prefix: &str) -> bool {
    ["capture/", "evidence/keep/", "evidence/remove/"]
        .iter()
        .any(|reserved| reserved.starts_with(prefix) || prefix.starts_with(reserved))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classification_is_structural_not_provenance_or_substring() {
        assert_eq!(
            EvidenceKey::parse("capture/CAP-a"),
            Some(EvidenceKey::Capture("CAP-a"))
        );
        assert_eq!(
            EvidenceKey::parse("evidence/keep/CAP-a"),
            Some(EvidenceKey::Keep("CAP-a"))
        );
        assert_eq!(
            EvidenceKey::parse("evidence/remove/CAP-a"),
            Some(EvidenceKey::Removal("CAP-a"))
        );
        for key in [
            "capture",
            "Capture/CAP-a",
            "x/capture/CAP-a",
            "data/capture/CAP-a",
        ] {
            assert_eq!(EvidenceKey::parse(key), None);
        }
    }
    #[test]
    fn broad_prefixes_include_new_store() {
        for prefix in [
            "",
            "c",
            "capture",
            "capture/",
            "capture/CAP-",
            "e",
            "evidence/",
            "evidence/keep/",
        ] {
            assert!(intersects(prefix), "{prefix}");
        }
        for prefix in [
            "decoded-v1/",
            "source/",
            "capturex",
            "evidence/keeps/",
            "Evidence/",
        ] {
            assert!(!intersects(prefix), "{prefix}");
        }
    }
}
