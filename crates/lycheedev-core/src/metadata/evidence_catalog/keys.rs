//! One reserved-key classifier shared by reads, pages and every mutation path.
//! Provenance text is not a storage-domain capability.
use crate::{Result, ToolkitError};

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
/// A batch may be a single-store mutation, never an accidental distributed
/// transaction. Determine its owning store before the first write.
pub(crate) fn one_owner<'a>(keys: impl IntoIterator<Item = &'a str>) -> Result<Option<&'a str>> {
    let mut owner = None;
    let mut ordinary = false;
    for key in keys {
        match EvidenceKey::parse(key) {
            Some(key) => {
                let id = key.capture_id();
                if id.is_empty() || owner.is_some_and(|previous| previous != id) || ordinary {
                    return Err(ToolkitError::invalid("mixed evidence mutation owners"));
                }
                owner = Some(id);
            }
            None => {
                if owner.is_some() {
                    return Err(ToolkitError::invalid(
                        "mixed evidence and metadata mutation",
                    ));
                }
                ordinary = true;
            }
        }
    }
    Ok(owner)
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
    #[test]
    fn mutation_owner_requires_one_capture_across_reserved_kinds() {
        assert_eq!(
            one_owner([
                "capture/CAP-a",
                "evidence/keep/CAP-a",
                "evidence/remove/CAP-a"
            ])
            .unwrap(),
            Some("CAP-a")
        );
        assert_eq!(one_owner(["ordinary/a", "ordinary/b"]).unwrap(), None);
        assert!(one_owner(["capture/CAP-a", "capture/CAP-b"]).is_err());
        assert!(one_owner(["capture/CAP-a", "ordinary/a"]).is_err());
        assert!(one_owner(["ordinary/a", "capture/CAP-a"]).is_err());
        assert!(one_owner(["capture/"]).is_err());
    }
}
