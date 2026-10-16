use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
#[derive(Clone, Deserialize, Serialize)]
pub struct Definition {
    pub path: String,
    pub summary: String,
    pub mutates: bool,
    pub arguments: Vec<String>,
    pub flags: Vec<Flag>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Flag(pub String);
impl Flag {
    pub fn name(&self) -> &str {
        self.0.split_whitespace().next().unwrap_or("")
    }
    pub fn takes_value(&self) -> bool {
        // Explicit arities from Go v3.2.0 (3057a966), independent of help prose.
        // In particular, `--encoding raw|png|webp` takes a value, whereas
        // `--scan (dbcache: ...)` does not. Fixture tests cover every entry.
        match self.name() {
            "--allow-partial" | "--cdn" | "--dry-run" | "--fix" | "--flow" | "--fresh"
            | "--include-removed" | "--latest" | "--no-cache" | "--offline" | "--overwrite"
            | "--passive" | "--plan" | "--remote" | "--replace" | "--resume" | "--scan"
            | "--semantic" | "--session-reuse" | "--static-only" | "--stdin" | "--uncommitted" => {
                false
            }
            "--after-id"
            | "--after-index"
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
            | "--display-id"
            | "--download-workers"
            | "--encoding"
            | "--environment"
            | "--extension"
            | "--field"
            | "--fields"
            | "--file"
            | "--file-data-id"
            | "--file-id"
            | "--filter"
            | "--format"
            | "--from"
            | "--gender"
            | "--home"
            | "--id"
            | "--ids"
            | "--installation"
            | "--item-id"
            | "--journal-encounter-id"
            | "--key-file"
            | "--limit"
            | "--line"
            | "--listfile"
            | "--locale"
            | "--matrix"
            | "--max-bytes"
            | "--max-depth"
            | "--max-frames"
            | "--max-lines"
            | "--max-objects"
            | "--max-pages"
            | "--max-pixels"
            | "--max-requests"
            | "--mipmap"
            | "--mode"
            | "--model-file-data-id"
            | "--name"
            | "--npc-id"
            | "--output"
            | "--page"
            | "--param"
            | "--path"
            | "--pid"
            | "--policy"
            | "--probe"
            | "--product"
            | "--project"
            | "--push"
            | "--query"
            | "--race-id"
            | "--raidbots"
            | "--realm"
            | "--record"
            | "--ref"
            | "--region"
            | "--region-id"
            | "--release"
            | "--request"
            | "--search"
            | "--session"
            | "--snapshot"
            | "--source"
            | "--spell-id"
            | "--sql"
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
            | "--value"
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
    all()
        .iter()
        .filter(|d| match d.path.as_str() {
            "version" | "describe" | "project init" | "project status" | "project lock"
            | "target available" => true,
            // Each business crate publishes only completed handlers; inventory remains exhaustive separately.
            value if value.starts_with("source ") => lycheedev_source::commands().contains(&value),
            value if value.starts_with("data ") || value.starts_with("asset ") => {
                lycheedev_data::commands().contains(&value)
            }
            value if value.starts_with("live ") => lycheedev_live::commands().contains(&value),
            value
                if value.starts_with("addon ")
                    || value.starts_with("skill ")
                    || value == "update" =>
            {
                lycheedev_delivery::commands().contains(&value)
            }
            _ => {
                lycheedev_core::workspace::commands().contains(&d.path.as_str())
                    || lycheedev_core::evidence::commands().contains(&d.path.as_str())
                    || lycheedev_core::cache::commands().contains(&d.path.as_str())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeSet;

    fn frozen_commands() -> Vec<Value> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/rust-parity/go-3.2.0.manifest.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["oracle"]["commit"],
            "3057a9666ab078270b63c7f06de748069b0d1f4b"
        );
        fixture["commands"].as_array().unwrap().clone()
    }

    #[test]
    fn current_inventory_matches_fixed_go_metadata_in_order() {
        let mut expected = frozen_commands();
        for definition in &mut expected {
            definition.as_object_mut().unwrap().remove("parserFlags");
            // The only allowed doctor metadata delta is the selected Source
            // engine; keep the fixed Go fixture and all other fields exact.
            if definition["path"] == "source batch" {
                definition["summary"] = serde_json::json!(definition["summary"].as_str().unwrap().replace("LuaLS", "native Emmy"));
            }
            if definition["path"] == "doctor" {
                assert_eq!(definition["summary"], "Aggregate read-only workspace, data, LuaLS and source checks; when a live target or CON is selected, include the same checks as live doctor; exit 3 means an unhealthy check");
                definition["summary"] = serde_json::json!("Aggregate read-only workspace, data, embedded Emmy worker smoke and source checks; when a live target or CON is selected, include the same checks as live doctor; exit 3 means an unhealthy check");
            }
        }
        assert_eq!(serde_json::to_value(all()).unwrap(), Value::Array(expected));
    }

    #[test]
    fn catalog_reuses_immutable_definitions_with_identical_serialization() {
        let first = all();
        let second = all();
        assert!(std::ptr::eq(first, second));
        let independently_parsed: Vec<Definition> =
            serde_json::from_str(include_str!("commands.json")).unwrap();
        assert_eq!(
            serde_json::to_vec(first).unwrap(),
            serde_json::to_vec(&independently_parsed).unwrap()
        );
        assert_eq!(
            serde_json::to_value(first).unwrap(),
            serde_json::from_str::<Value>(include_str!("commands.json")).unwrap()
        );
    }

    #[test]
    fn implemented_catalog_borrows_the_complete_current_inventory_in_order() {
        let current = all();
        let selected = implemented();
        // Every current route has a concrete handler. Keep this exhaustive
        // contract rather than accepting a filtered subset after caching.
        assert_eq!(current.len(), 84);
        assert_eq!(selected.len(), current.len());
        for (borrowed, original) in selected.iter().zip(current) {
            assert!(std::ptr::eq(*borrowed, original));
        }
        let independently_parsed: Vec<Definition> =
            serde_json::from_str(include_str!("commands.json")).unwrap();
        assert_eq!(
            serde_json::to_vec(&selected).unwrap(),
            serde_json::to_vec(&independently_parsed).unwrap()
        );
        let repeated = implemented();
        for (first, second) in selected.iter().zip(&repeated) {
            assert!(std::ptr::eq(*first, *second));
        }
        assert_eq!(selected.len(), repeated.len());
    }

    #[test]
    fn cached_describe_and_command_help_keep_all_fields_across_repeated_reads() {
        let expected = serde_json::from_str::<Value>(include_str!("commands.json")).unwrap();
        let describe = ["describe", "--format", "json"].map(str::to_owned);
        for _ in 0..2 {
            let result = crate::dispatch(&crate::parse(&describe).unwrap()).unwrap();
            assert_eq!(result.data, serde_json::json!({
                "commands":expected,
                "formats":["text", "json", "jsonl"],
                "development":crate::VERSION.contains('-')
            }));
            let input = ["--help".to_owned()];
            let result = crate::dispatch(&crate::parse(&input).unwrap()).unwrap();
            assert_eq!(result.data, serde_json::json!({
                "commands":expected,
                "flags":["--home <root>", "--format text|json|jsonl", "--help"]
            }));
            for definition in all() {
                let mut input: Vec<String> =
                    definition.path.split_whitespace().map(str::to_owned).collect();
                input.push("--help".into());
                let mut flags: Vec<&str> =
                    definition.flags.iter().map(|flag| flag.0.as_str()).collect();
                flags.push("--help");
                let result = crate::dispatch(&crate::parse(&input).unwrap()).unwrap();
                assert_eq!(result.data, serde_json::json!({
                    "commands":[definition], "flags":flags
                }), "{}", definition.path);
            }
        }
    }

    #[test]
    fn every_frozen_flag_has_exact_explicit_arity() {
        let mut seen = BTreeSet::new();
        for command in frozen_commands() {
            for expected in command["parserFlags"].as_array().unwrap() {
                let display = expected["display"].as_str().unwrap();
                let name = expected["name"].as_str().unwrap();
                let flag = Flag(display.to_owned());
                assert_eq!(flag.name(), name);
                assert_eq!(
                    flag.takes_value(),
                    expected["value"].as_bool().unwrap(),
                    "{display}"
                );
                seen.insert(name.to_owned());
            }
        }
        assert_eq!(seen.len(), 115);
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
                error.message.starts_with("unknown command"),
                "{path}: {}",
                error.message
            );
        }
    }
}
