// The graph retains the exact lexical observation, not an eager JSON object
// for every name. SemanticId owns immutable interned keys; its derived Debug
// representation does not consult an analysis/model or a mutable symbol table.
// A file path is shared only within this harvest. No engine lifetime is borrowed.
#[derive(Clone, Debug)]
pub struct RawEmmyEvidence {
    file: Arc<str>,
    start: u32,
    end: u32,
    identity: Option<emmylua_code_analysis::SemanticId>,
}
impl RawEmmyEvidence {
    fn new(
        file: Arc<str>,
        site: &Site,
        identity: Option<emmylua_code_analysis::SemanticId>,
    ) -> Self {
        Self {
            file,
            start: site.start,
            end: site.end,
            identity,
        }
    }
}
impl Serialize for RawEmmyEvidence {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        struct Identity<'a>(&'a Option<emmylua_code_analysis::SemanticId>);
        impl Serialize for Identity<'_> {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                // collect_str writes one Debug value, rather than retaining its
                // formatted String for every name for the graph's lifetime.
                serializer.collect_str(&format_args!("{:?}", self.0))
            }
        }
        #[derive(Serialize)]
        struct OriginalSite<'a> {
            end: u32,
            file: &'a str,
            start: u32,
        }
        // Preserve serde_json::Value's canonical key order, including the
        // nested Site, not the declaration order of the public Site struct.
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("rawIdentity", &Identity(&self.identity))?;
        map.serialize_entry(
            "site",
            &OriginalSite {
                end: self.end,
                file: &self.file,
                start: self.start,
            },
        )?;
        map.serialize_entry(
            "usedFor",
            "lexical-local-identity-only; global/value selection excluded",
        )?;
        map.end()
    }
}
