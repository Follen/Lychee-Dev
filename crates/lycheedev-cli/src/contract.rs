use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, sync::OnceLock};
#[derive(Clone, Deserialize, Serialize)]
pub struct Definition {
    pub path: String,
    pub summary: String,
    pub mutates: bool,
    /// Possible game interaction, including any option required to send input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects: Option<GameEffects>,
    pub arguments: Vec<String>,
    pub flags: Vec<Flag>,
    /// Flags that are always required for this command.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    /// Structured defaults shown to the agent; prose in `flags` may repeat them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defaults: BTreeMap<String, Value>,
    /// Groups where at most one flag may be present.
    #[serde(
        default,
        rename = "mutuallyExclusive",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub mutually_exclusive: Vec<Vec<String>>,
    /// Groups where at least one flag must be present.
    #[serde(
        default,
        rename = "oneOfRequired",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub one_of_required: Vec<Vec<String>>,
    /// Additional handler-owned constraints that are not a simple flag list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
    /// Minimal call templates. Input placeholders are explained by constraints
    /// and discovery prerequisites; parser checks do not prove real-client use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameEffects {
    pub game_process: bool,
    pub game_input: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub game_input_when: BTreeMap<String, bool>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Flag(pub String);
impl Flag {
    pub fn name(&self) -> &str {
        self.0.split_whitespace().next().unwrap_or("")
    }
    pub fn takes_value(&self) -> bool {
        // Explicit current flag arities, independent of help prose.
        // In particular, `--encoding raw|png|webp` takes a value, whereas
        // `--scan (dbcache: ...)` does not. Fixture tests cover every entry.
        match self.name() {
            "--allow-partial" | "--cdn" | "--data" | "--data-derived" | "--dry-run" | "--fix"
            | "--hard-reset" | "--flow" | "--fresh" | "--include-removed" | "--latest"
            | "--offline" | "--overwrite" | "--plan" | "--remote"
            | "--replace" | "--resume" | "--scan" | "--semantic" | "--session-reuse"
            | "--static-only" | "--uncommitted" => false,
            "--after-index"
            | "--budget-seconds"
            | "--build"
            | "--cache-max-bytes"
            | "--channels"
            | "--character"
            | "--content-variant"
            | "--count"
            | "--cursor"
            | "--dbcache"
            | "--definitions"
            | "--depth"
            | "--direction"
            | "--download-workers"
            | "--encoding"
            | "--environment"
            | "--extension"
            | "--file"
            | "--file-id"
            | "--format"
            | "--from"
            | "--home"
            | "--ids"
            | "--installation"
            | "--key-file"
            | "--limit"
            | "--line"
            | "--listfile"
            | "--locale"
            | "--matrix"
            | "--max-bytes"
            | "--max-frames"
            | "--max-lines"
            | "--max-objects"
            | "--max-pages"
            | "--max-pixels"
            | "--max-requests"
            | "--mipmap"
            | "--mode"
            | "--name"
            | "--output"
            | "--page"
            | "--path"
            | "--pid"
            | "--policy"
            | "--probe"
            | "--product"
            | "--project"
            | "--push"
            | "--query"
            | "--raidbots"
            | "--realm"
            | "--record"
            | "--ref"
            | "--region"
            | "--region-id"
            | "--result-ttl-seconds"
            | "--release"
            | "--request"
            | "--search"
            | "--session"
            | "--snapshot"
            | "--source"
            | "--status"
            | "--symbol"
            | "--symbol-id"
            | "--table"
            | "--table-hash"
            | "--target"
            | "--target-bytes"
            | "--target-path"
            | "--timeout-seconds"
            | "--to"
            | "--toc"
            | "--topic"
            | "--wait-seconds" => true,
            name => panic!("flag missing explicit contract arity: {name}"),
        }
    }
}
pub fn all() -> &'static [Definition] {
    // Parsing, dispatch and rendering read the same immutable embedded bytes.
    // Validate them once per process; this is not a persistent metadata cache.
    static DEFINITIONS: OnceLock<Vec<Definition>> = OnceLock::new();
    DEFINITIONS
        .get_or_init(|| {
            serde_json::from_str(include_str!("commands.json")).expect("checked command contract")
        })
        .as_slice()
}
pub fn implemented() -> Vec<&'static Definition> {
    all().iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn typed_catalog_preserves_all_raw_contract_fields() {
        assert_eq!(
            serde_json::to_value(all()).unwrap(),
            serde_json::from_str::<Value>(include_str!("commands.json")).unwrap()
        );
    }

    #[test]
    fn cached_describe_and_command_help_keep_all_fields_across_repeated_reads() {
        let expected = serde_json::from_str::<Value>(include_str!("commands.json")).unwrap();
        let describe = ["describe", "--format", "json"].map(str::to_owned);
        for _ in 0..2 {
            let result = crate::dispatch(&crate::parse(&describe).unwrap()).unwrap();
            assert_eq!(result.data["schema"], "lycheedev.cli-describe.v1");
            let rows = result.data["commands"].as_array().unwrap();
            let expected = expected.as_array().unwrap();
            assert_eq!(rows.len(), expected.len());
            for (row, raw) in rows.iter().zip(expected) {
                let mut contract_row = row.as_object().unwrap().clone();
                contract_row.remove("readOnly");
                contract_row.remove("effects");
                let mut raw = raw.as_object().unwrap().clone();
                raw.remove("effects");
                assert_eq!(Value::Object(contract_row), Value::Object(raw));
            }
            assert_eq!(
                result.data["formats"],
                serde_json::json!(["text", "json", "jsonl"])
            );
            assert_eq!(result.data["development"], crate::VERSION.contains('-'));
            assert_eq!(result.data["exitCodes"].as_array().unwrap().len(), 8);
            assert_eq!(result.data["products"].as_array().unwrap().len(), 8);
            assert_eq!(result.data["result"]["schema"], "lycheedev.result.v1");
            assert!(result.data["discovery"].as_array().unwrap().len() >= 2);
            let input = ["--help".to_owned()];
            let result = crate::dispatch(&crate::parse(&input).unwrap()).unwrap();
            assert_eq!(result.data["schema"], "lycheedev.cli-help.v1");
            assert_eq!(result.data["kind"], "group");
            assert!(result.data["commands"].is_null());
            assert_eq!(result.data["truncated"], false);
            assert!(result.data["children"].as_array().unwrap().len() <= 32);
            for definition in all() {
                let mut input: Vec<String> = definition
                    .path
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect();
                input.push("--help".into());
                let mut flags: Vec<&str> = definition
                    .flags
                    .iter()
                    .map(|flag| flag.0.as_str())
                    .collect();
                flags.push("--help");
                let result = crate::dispatch(&crate::parse(&input).unwrap()).unwrap();
                assert_eq!(result.data["schema"], "lycheedev.cli-help.v1");
                assert_eq!(result.data["kind"], "command");
                assert_eq!(result.data["commands"], serde_json::json!([definition]));
                assert_eq!(result.data["flags"], serde_json::json!(flags));
            }
        }
    }

    #[test]
    fn flag_arity_does_not_depend_on_help_prose() {
        assert!(Flag("--encoding raw|png|webp".into()).takes_value());
        assert!(Flag("--encoding (description without value syntax)".into()).takes_value());
        assert!(
            !Flag("--scan (dbcache: complete bounded scan; --cursor resumes its capture)".into())
                .takes_value()
        );
        assert!(!Flag("--session-reuse (reuse an explicit session)".into()).takes_value());
    }

    #[test]
    #[should_panic(expected = "flag missing explicit contract arity")]
    fn unknown_bundled_flag_fails_closed() {
        Flag("--unreviewed <value>".into()).takes_value();
    }

    #[test]
    fn parser_consumes_enum_value_and_not_explanatory_boolean_flags() {
        for encoding in ["raw", "png", "webp"] {
            let input = ["asset", "export", "--encoding", encoding].map(str::to_owned);
            let invocation = crate::parse(&input).unwrap();
            assert_eq!(invocation.flag("--encoding"), Some(encoding));
        }
        let input = ["data", "hotfix", "--scan", "--source", "dbcache"].map(str::to_owned);
        let invocation = crate::parse(&input).unwrap();
        assert_eq!(invocation.flag("--scan"), Some("true"));
        assert_eq!(invocation.flag("--source"), Some("dbcache"));
    }

    #[test]
    fn removed_legacy_paths_are_not_public_parser_routes() {
        for path in [
            "live ack",
            "live bind",
            "live finish",
            "live hide",
            "live probe load",
            "live reset",
            "live run",
        ] {
            let input: Vec<_> = path.split_whitespace().map(str::to_owned).collect();
            let error = crate::parse(&input).unwrap_err();
            assert_eq!(error.exit_code, 2, "{path}");
            assert!(
                error.code == "command.invalid_arguments" && error.details["parser"] == "clap",
                "{path}: {}",
                error.message
            );
        }
    }
    #[test]
    fn data_protection_inventory_is_an_explicit_boolean_status_option() {
        let arguments = ["cache", "status", "--data"].map(str::to_owned);
        let parsed = crate::parse(&arguments).unwrap();
        assert_eq!(parsed.command, "cache status");
        assert_eq!(parsed.flags.get("--data").map(String::as_str), Some("true"));
        assert!(crate::parse(&["cache", "prune", "--data"].map(str::to_owned)).is_err());
    }
}
