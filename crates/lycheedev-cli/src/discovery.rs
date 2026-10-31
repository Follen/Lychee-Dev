//! Agent-facing self-description shared by `describe`, `schema`, `--help` and
//! `cli search`. It reads only the static command contract and the current
//! product catalog: no project, home, cache, engine, worker, network or game is
//! opened, and discovery never mutates persistent state.
use crate::contract::Definition;
use serde_json::{Value, json};

/// Stable process exit codes and the applicable next step.
const EXIT_CODES: [(i64, &str, &str); 9] = [
    (0, "complete", "Result is complete for the selected scope."),
    (
        2,
        "arguments/selection",
        "Use `lycheedev schema <command>` or `--help`; correct the named argument or selection.",
    ),
    (
        3,
        "capability/environment",
        "Inspect the selected target or installation with the matching read-only status or doctor command.",
    ),
    (
        4,
        "invalid input/protocol",
        "Correct the input to the documented shape; do not replay unknown or malformed input.",
    ),
    (
        5,
        "external/business failure",
        "Read the error details; retry only work known not to be committed or replayed.",
    ),
    (
        6,
        "pending",
        "Retain the original request and deadline identity, then continue the matching recovery command.",
    ),
    (
        7,
        "cancelled",
        "The operation was cancelled; handles from it are invalid and it is not replayed automatically.",
    ),
    (
        8,
        "internal",
        "Keep the error details for diagnosis; do not loop-retry.",
    ),
    (
        // Reserved documentation slot: unknown code must not be invented.
        -1,
        "unknown",
        "Read the reported code and details instead of assuming a category.",
    ),
];

pub fn exit_codes() -> Value {
    Value::Array(
        EXIT_CODES
            .iter()
            .filter(|(code, _, _)| *code >= 0)
            .map(|(code, meaning, next)| json!({"code":code,"meaning":meaning,"next":next}))
            .collect(),
    )
}

/// What invoking the command changes, separate from whether an error is
/// business, environment or protocol related. Game input is called out because
/// it is a user-visible side effect, not an ordinary workspace write.
pub fn effects(definition: &Definition) -> Value {
    let game = definition.effects.as_ref();
    let mut effects = json!({
        "readOnly": !definition.mutates,
        "mutatesWorkspace": definition.mutates,
        "gameProcess": game.is_some_and(|effects| effects.game_process),
        "gameInput": game.is_some_and(|effects| effects.game_input),
    });
    if let Some(game) = game.filter(|effects| !effects.game_input_when.is_empty()) {
        effects["gameInputWhen"] = json!(game.game_input_when);
    }
    effects
}

/// Identity inputs already expressed by the command's own flags. Business
/// modules own the actual validation; this is discovery, not a second gate.
const IDENTITY_FLAGS: [(&str, &str); 16] = [
    (
        "--project",
        "Project directory and lock; the nearest parent project is used when omitted.",
    ),
    (
        "--snapshot",
        "Explicit fixed source/data pin; wins over project context.",
    ),
    (
        "--installation",
        "Selected client or game installation directory.",
    ),
    ("--pid", "Bind the running process with this PID."),
    ("--session", "Named retained session to continue."),
    ("--character", "Actor display-name constraint."),
    ("--realm", "Actor realm constraint."),
    ("--product", "Exact client product identity."),
    ("--build", "Exact client build identity."),
    ("--region", "Exact data region."),
    ("--locale", "Exact data locale."),
    ("--key-file", "CASC keys file for encrypted input."),
    ("--file", "Operation input read from this file."),
    ("--stdin", "Operation input read from standard input."),
    (
        "--release",
        "Verified distribution root or release archive for install/update.",
    ),
    ("--definitions", "Pinned definitions commit or full ref."),
];

pub fn prerequisites(definition: &Definition) -> Value {
    let mut inputs = Vec::new();
    for flag in &definition.flags {
        let requirement = if matches!(flag.name(), "--path" | "--output") {
            Some(format!("{}: {}", definition.path, flag.0))
        } else {
            IDENTITY_FLAGS.iter().find(|(name, _)| *name == flag.name())
                .map(|(_, requirement)| (*requirement).to_owned())
        };
        if let Some(requirement) = requirement {
            inputs.push(json!({"flag":flag.name(),"requirement":requirement}));
        }
    }
    json!({
        "identityInputs": inputs,
        "exampleInputs": "Replace uppercase tokens with the corresponding paths or returned IDs, and replace 12345 with the selected process PID. Keep paths containing spaces in a single argument. Command-specific input requirements are in input.constraints.",
        "workspace": "Persistent state lives under --home (default LYCHEEDEV_HOME or ~/.lycheedev); discovery commands never create it.",
    })
}

/// Ordinary command output contract. Data Agent JSONL keeps its own typed
/// session protocol, attached separately by `schema data agent serve`.
pub fn result_contract() -> Value {
    json!({
        "schema": "lycheedev.result.v1",
        "envelope": {"schema":"string","ok":"boolean","operationId":"string","context":"object",
            "result":"value","captures":"array","warnings":"array","error":"object|null"},
        "jsonl": {"endFrame":"required","exit":"0 on success"},
        "statuses": ["complete","partial","pending","cancelled"],
        "coverage": ["complete","partial","truncated","unknown"],
        "notes": "Preserve precise numbers, partial/truncated coverage and capture IDs; result handles belong to their session and are not replayed.",
    })
}

/// Current product identities (verified series/interface only). Two Forever
/// deliveries share a source family but remain separate Data/Live identities.
pub fn products() -> Value {
    Value::Array(lycheedev_core::client::discovery_products())
}

/// Common-name matching for `cli search`; returns canonical pending products so
/// the agent can choose by region instead of merging the two beta deliveries.
pub fn product_matches(query: &str) -> Vec<Value> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for product in lycheedev_core::client::discovery_products() {
        let matched = product["commonNames"]
            .as_array()
            .map(|names| {
                names.iter().filter_map(Value::as_str).any(|name| {
                    let name = name.to_lowercase();
                    query == name
                        || (name.chars().count() >= 2 && query.contains(&name))
                        || (query.chars().count() >= 2 && name.contains(&query))
                })
            })
            .unwrap_or(false);
        if matched {
            rows.push(product);
        }
    }
    rows.truncate(8);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract;

    #[test]
    fn public_result_contract_names_the_actual_success_and_error_envelope() {
        use std::collections::BTreeSet;
        let args = |words: &[&str]| words.iter().map(|word| (*word).into()).collect::<Vec<String>>();
        let invocation = crate::parse(&args(&["schema", "version"])).unwrap();
        let public = crate::dispatch(&invocation).unwrap().data;
        let expected: BTreeSet<_> = [
            "schema", "ok", "operationId", "context", "result", "captures", "warnings", "error",
        ].into_iter().collect();
        let version = crate::parse(&args(&["version"])).unwrap();
        for result in [
            crate::dispatch(&version),
            Err(lycheedev_core::ToolkitError::arguments("invalid selected argument")),
        ] {
            let mut bytes = Vec::new();
            crate::render(&mut bytes, "json", &result).unwrap();
            let actual: Value = serde_json::from_slice(&bytes).unwrap();
            let keys: BTreeSet<_> = actual.as_object().unwrap().keys().map(String::as_str).collect();
            assert_eq!(keys, expected);
            assert_eq!(actual["schema"], "lycheedev.result.v1");
            assert_eq!(actual["ok"], result.is_ok());
            assert!(actual["operationId"].is_string());
            assert!(actual["context"].is_object());
            assert!(actual["captures"].is_array());
            assert!(actual["warnings"].is_array());
            assert_eq!(actual["error"].is_null(), result.is_ok());
        }
        let documented: BTreeSet<_> = public["result"]["envelope"]
            .as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(documented, expected, "Agent discovery must describe the emitted envelope");
    }

    #[test]
    fn public_prerequisites_keep_command_specific_path_and_output_meanings() {
        for (command, flag, meaning) in [
            ("asset export", "--output", "export"),
            ("asset demux", "--output", "directory"),
            ("evidence bundle", "--output", "bundle"),
            ("source inspect", "--path", "source"),
            ("source validate", "--path", "addon"),
            ("project init", "--path", "project"),
            ("update", "--path", "skill"),
        ] {
            let schema_args = ["schema".to_owned(), command.to_owned()];
            let schema = crate::dispatch(&crate::parse(&schema_args).unwrap()).unwrap().data;
            let mut help_args: Vec<_> = command.split_whitespace().map(str::to_owned).collect();
            help_args.push("--help".into());
            let help = crate::dispatch(&crate::parse(&help_args).unwrap()).unwrap().data;
            assert_eq!(schema["prerequisites"], help["prerequisites"]);
            let row = schema["prerequisites"]["identityInputs"].as_array().unwrap()
                .iter().find(|row| row["flag"] == flag).unwrap();
            let requirement = row["requirement"].as_str().unwrap().to_lowercase();
            assert!(requirement.contains(meaning), "{command} {flag}: {requirement}");
            if command.starts_with("asset ") {
                assert!(!requirement.contains("upgrade"), "asset outputs are not upgrade archives");
            }
        }
    }

    #[test]
    fn exit_codes_are_stable_and_actionable() {
        let rows = exit_codes();
        let rows = rows.as_array().unwrap();
        // DESIGN keeps eight meaningful codes: 0,2,3,4,5,6,7,8 (1 is unused).
        assert_eq!(rows.len(), 8);
        for (index, expected) in [0, 2, 3, 4, 5, 6, 7, 8].into_iter().enumerate() {
            assert_eq!(rows[index]["code"], expected);
            assert!(!rows[index]["next"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn product_catalog_keeps_forever_betas_distinct() {
        let rows = products();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 8, "seven deliveries plus legacy family alias");
        let forever: Vec<_> = rows
            .iter()
            .filter(|row| row["family"] == "forever")
            .collect();
        assert_eq!(forever.len(), 3);
        let deliveries: Vec<_> = forever
            .iter()
            .filter(|row| row["delivery"] == true)
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(deliveries, ["forever-asia-beta", "forever-cn-beta"]);
        for row in &forever {
            assert_eq!(row["buildSeries"], json!(["1.60.1"]));
            assert_eq!(row["tocInterfaces"], json!([16001]));
        }
    }

    #[test]
    fn common_names_match_both_beta_deliveries_without_merging() {
        let rows = product_matches("无限服");
        let ids: Vec<_> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
        assert!(ids.contains(&"forever-asia-beta"), "{ids:?}");
        assert!(ids.contains(&"forever-cn-beta"), "{ids:?}");
        assert!(!ids.contains(&"retail"));
        for (name, expected) in [
            ("时光服", "titan"),
            ("经典版", "classic"),
            ("经典旧世", "classic-era"),
            ("怀旧服", "classic"),
            ("正式服", "retail"),
            ("周年服", "anniversary"),
        ] {
            let rows = product_matches(name);
            let ids: Vec<_> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
            assert!(ids.contains(&expected), "{name} matched {ids:?}");
        }
    }

    #[test]
    fn effects_distinguish_probe_store_from_game_interaction() {
        let effect = |path: &str| {
            effects(
                contract::all()
                    .iter()
                    .find(|definition| definition.path == path)
                    .unwrap(),
            )
        };
        for path in [
            "live connect",
            "live execute",
            "live bugs",
            "live resume",
            "live disconnect",
            "live reload",
            "live reload fallback",
        ] {
            let row = effect(path);
            assert_eq!(row["gameProcess"], true, "{path}");
            assert_eq!(row["gameInput"], true, "{path}");
            assert!(row["gameInputWhen"].is_null(), "{path}");
        }
        let repair = effect("doctor");
        assert_eq!(repair["gameProcess"], true);
        assert_eq!(repair["gameInput"], true);
        assert_eq!(repair["gameInputWhen"], json!({"--fix": true}));
        for path in ["live instances"] {
            assert_eq!(effect(path)["gameProcess"], true, "{path}");
            assert_eq!(effect(path)["gameInput"], false, "{path}");
        }
        for path in [
            "live probe put",
            "live probe list",
            "live probe show",
            "live probe remove",
            "live status",
            "live session",
            "version",
        ] {
            assert_eq!(effect(path)["gameProcess"], false, "{path}");
            assert_eq!(effect(path)["gameInput"], false, "{path}");
        }
        assert_eq!(effect("version")["readOnly"], true);
    }
}
