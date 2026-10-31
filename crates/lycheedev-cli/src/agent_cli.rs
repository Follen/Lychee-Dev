//! Reimplemented after artifact loss. Discovery reads immutable metadata only:
//! no project, home, cache, engine, worker, network or game is opened here.
use crate::Invocation;
use crate::contract::{self, Definition, Flag};
use clap::{
    Arg, ArgAction, ArgMatches, Command,
    builder::{NonEmptyStringValueParser, PossibleValuesParser},
};
use lycheedev_core::{Outcome, Result, ToolkitError};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const SEARCH_LIMIT: usize = 5;
const CHILD_LIMIT: usize = 32;
const QUERY_LIMIT: usize = 512;
const DISCOVERY: &str = "lycheedev cli search \"<task>\"";

fn repeatable(command: &str, flag: &str) -> bool {
    command == "update" && matches!(flag, "--path" | "--installation")
}
fn enum_values(flag: &Flag) -> Option<Vec<String>> {
    // Only explicit enum identities: filename examples such as --key-file and
    // --listfile contain pipes too, but are not closed sets of accepted values.
    if !matches!(
        flag.name(),
        "--format"
            | "--content-variant"
            | "--direction"
            | "--encoding"
            | "--mode"
            | "--policy"
            | "--product"
            | "--region"
            | "--source"
            | "--topic"
    ) {
        return None;
    }
    let token = flag.0.split_whitespace().nth(1)?;
    if !token.contains('|') && !matches!(flag.name(), "--format" | "--encoding") {
        return None;
    }
    let values: Vec<String> = token
        .trim_matches(|c| c == '<' || c == '>')
        .split('|')
        .map(str::to_owned)
        .collect();
    Some(values)
}
fn option(command: &str, flag: &Flag) -> Arg {
    let id = flag.name().trim_start_matches("--").to_owned();
    let arg = Arg::new(id.clone()).long(id).help(flag.0.clone());
    if !flag.takes_value() {
        return arg.action(ArgAction::SetTrue);
    }
    let mut arg = arg
        .num_args(1)
        .value_parser(NonEmptyStringValueParser::new())
        .action(if repeatable(command, flag.name()) {
            ArgAction::Append
        } else {
            ArgAction::Set
        });
    if let Some(values) = enum_values(flag) {
        arg = arg.value_parser(PossibleValuesParser::new(values));
    }
    arg
}
fn children(prefix: &str) -> Vec<String> {
    let stem = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix} ")
    };
    contract::all()
        .iter()
        .filter_map(|d| d.path.strip_prefix(&stem))
        .filter_map(|tail| tail.split_whitespace().next())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
fn node(name: &str, path: &str) -> Command {
    let mut command = Command::new(name.to_owned())
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .disable_version_flag(true)
        .arg_required_else_help(false);
    if let Some(definition) = contract::all().iter().find(|d| d.path == path) {
        command = command.about(definition.summary.clone());
        for flag in &definition.flags {
            if !matches!(flag.name(), "--format" | "--home") {
                command = command.arg(option(path, flag));
            }
        }
        if let Some(argument) = definition.arguments.first() {
            let mut arg = Arg::new("positional")
                .value_name(argument.clone())
                .value_parser(NonEmptyStringValueParser::new())
                .required_unless_present("help");
            if path == "schema" {
                arg = arg.num_args(1..).action(ArgAction::Append);
            }
            command = command.arg(arg);
        }
    }
    for child in children(path) {
        let next = if path.is_empty() {
            child.clone()
        } else {
            format!("{path} {child}")
        };
        command = command.subcommand(node(&child, &next));
    }
    command
}
fn parser() -> Command {
    node("lycheedev", "")
        .arg(
            Arg::new("help")
                .long("help")
                .short('h')
                .global(true)
                .action(ArgAction::SetTrue)
                .overrides_with("help"),
        )
        .arg(option("", &Flag("--format text|json|jsonl".into())).global(true))
        .arg(option("", &Flag("--home <root>".into())).global(true))
}
fn leaf(mut matches: &ArgMatches) -> (String, &ArgMatches) {
    let mut path = Vec::new();
    while let Some((name, child)) = matches.subcommand() {
        path.push(name);
        matches = child;
    }
    (path.join(" "), matches)
}
fn command_path_before_error(args: &[String], tree: &Command) -> (String, usize) {
    let mut current = tree;
    let mut path = Vec::new();
    let mut index = 0;
    while let Some(token) = args.get(index) {
        if token == "--" {
            break;
        }
        if token.starts_with('-') {
            let (name, attached) = token.split_once('=').map_or((token.as_str(), false), |(name, _)| (name, true));
            let option = current.get_arguments().find(|arg| {
                name.strip_prefix("--").is_some_and(|name| arg.get_long() == Some(name))
                    || name.strip_prefix('-').is_some_and(|name| {
                        name.chars().count() == 1 && arg.get_short() == name.chars().next()
                    })
            });
            let Some(option) = option else { break };
            index += if option.get_action().takes_values() && !attached { 2 } else { 1 };
            continue;
        }
        let Some(child) = current.find_subcommand(token) else { break };
        path.push(child.get_name());
        current = child;
        index += 1;
    }
    (path.join(" "), index.min(args.len()))
}
fn annotate_argument_error(error: &mut ToolkitError, definition: &Definition) {
    let mut accepted = definition.flags.iter().map(|flag| flag.name()).collect::<Vec<_>>();
    accepted.extend(["--help", "-h"]);
    let usage = std::iter::once(format!("lycheedev {}", definition.path))
        .chain(definition.arguments.iter().cloned())
        .chain(std::iter::once("[OPTIONS]".into()))
        .collect::<Vec<_>>()
        .join(" ");
    let details = error.details.as_object_mut().expect("parser failure details");
    details.insert("command".into(), json!(definition.path));
    details.insert("usage".into(), json!(usage));
    details.insert("schemaCommand".into(), json!(format!("lycheedev schema {}", definition.path)));
    details.insert("helpCommand".into(), json!(format!("lycheedev {} --help", definition.path)));
    details.insert("acceptedFlags".into(), json!(accepted));
    details.insert("arguments".into(), json!(definition.arguments));
    details.insert("requiredFlags".into(), json!(definition.required));
    // Correct this command's input; another operation is not an argument fix.
    details.insert("suggestions".into(), json!([]));
}
fn parse_error(error: clap::Error, args: &[String], tree: &Command) -> ToolkitError {
    let (path, unmatched) = command_path_before_error(args, tree);
    let invalid_command = error.get(clap::error::ContextKind::InvalidSubcommand);
    let query = if invalid_command.is_some() {
        // Retain compound misspellings, but never rank an option value as a
        // command. The prefix and first unmatched token come from the same tree.
        std::iter::once(path.as_str()).chain(args[unmatched..].iter()
            .take_while(|word| !word.starts_with('-')).take(8).map(String::as_str))
            .filter(|word| !word.is_empty()).collect::<Vec<_>>().join(" ")
    } else {
        args.iter().filter(|s| !s.starts_with('-')).take(8)
            .map(String::as_str).collect::<Vec<_>>().join(" ")
    };
    let mut failure = ToolkitError::arguments("invalid command arguments");
    // Do not call clap's print/exit APIs. Existing envelopes own both streams.
    failure.details = json!({"parser":"clap","kind":format!("{:?}",error.kind()),
        "diagnostic":error.to_string().chars().take(2048).collect::<String>(),
        "suggestions":[],"discovery":DISCOVERY});
    if invalid_command.is_none()
        && let Some(definition) = contract::all().iter().find(|definition| definition.path == path)
    {
        annotate_argument_error(&mut failure, definition);
    } else {
        failure.details["suggestions"] = json!(nearest(&query));
    }
    failure
}
pub fn parse(args: &[String]) -> Result<Invocation> {
    let mut tree = parser();
    let matches = tree
        .try_get_matches_from_mut(std::iter::once("lycheedev".to_owned()).chain(args.iter().cloned()))
        .map_err(|error| parse_error(error, args, &tree))?;
    let (command, matched) = leaf(&matches);
    let definition = contract::all().iter().find(|d| d.path == command);
    let help = matched.get_flag("help") || definition.is_none();
    let mut flags = BTreeMap::new();
    let mut ordered = Vec::new();
    if help {
        flags.insert("--help".into(), "true".into());
    }
    // Global placement does not authorize --home for a command without it.
    if matched.get_one::<String>("home").is_some()
        && definition.is_some_and(|d| !d.flags.iter().any(|f| f.name() == "--home"))
    {
        let mut error = ToolkitError::arguments(format!("--home is not supported by {command}"));
        error.details = json!({"parser":"clap","kind":"UnknownArgument","flag":"--home"});
        annotate_argument_error(&mut error, definition.expect("known command"));
        return Err(error);
    }
    // Help is a metadata operation, not the worker's JSONL transport. In
    // particular `data agent serve --help --format json` must remain legal.
    if !help
        && let Some(definition) = definition
        && let Some(value) = matched.get_one::<String>("format")
        && let Some(spec) = definition.flags.iter().find(|f| f.name() == "--format")
        && let Some(allowed) = enum_values(spec)
        && !allowed.contains(value)
    {
        let mut error = ToolkitError::arguments(format!("unsupported --format for {command}"));
        error.details =
            json!({"parser":"clap","kind":"InvalidValue","flag":"--format","allowed":allowed});
        annotate_argument_error(&mut error, definition);
        return Err(error);
    }
    let specs = definition.map(|d| d.flags.clone()).unwrap_or_else(|| {
        vec![
            Flag("--home <root>".into()),
            Flag("--format text|json|jsonl".into()),
        ]
    });
    for flag in specs {
        let id = flag.name().trim_start_matches("--");
        if flag.takes_value() {
            if let Some(values) = matched.get_many::<String>(id) {
                let values: Vec<String> = values.cloned().collect();
                if values.len() > 256 {
                    return Err(ToolkitError::arguments(
                        "repeated option exceeds 256 values",
                    ));
                }
                if let Some(last) = values.last() {
                    flags.insert(flag.name().into(), last.clone());
                }
                if values.len() > 1 {
                    flags.insert(
                        format!("__repeat:{}", flag.name()),
                        serde_json::to_string(&values)?,
                    );
                }
                for (index, value) in matched.indices_of(id).into_iter().flatten().zip(values) {
                    ordered.push((index, flag.name().to_owned(), value));
                }
            }
        } else if matched.get_flag(id) {
            flags.insert(flag.name().into(), "true".into());
            ordered.push((
                matched.index_of(id).unwrap_or(0),
                flag.name().to_owned(),
                "true".into(),
            ));
        }
    }
    ordered.sort_by_key(|(index, _, _)| *index);
    let ordered: Vec<(String, String)> = ordered
        .into_iter()
        .map(|(_, key, value)| (key, value))
        .collect();
    for (key, value) in &ordered {
        if command.starts_with("live ") || command == "doctor" {
            crate::formal_live::validate_option(key, value)?;
        }
        if command == "data hotfix" {
            crate::hotfix_cli::validate_option(key, value)?;
        }
    }
    let positional = if definition.is_some_and(|d| !d.arguments.is_empty()) {
        matched
            .get_many::<String>("positional")
            .map(|values| values.map(String::as_str).collect::<Vec<_>>().join(" "))
    } else {
        None
    };
    crate::finish_parse(command, positional, flags, help)
}
fn brief(definition: &Definition) -> Value {
    json!({"command":definition.path,"summary":definition.summary.chars().take(256).collect::<String>(),
        "mutates":definition.mutates,"readOnly":!definition.mutates,
        "schemaCommand":format!("lycheedev schema {}",definition.path)})
}
fn terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .take(32)
        .collect();
    for (needle, alias) in [
        ("法术", "spell"),
        ("物品", "item"),
        ("生物", "creature"),
        ("技能", "spell"),
        ("缓存", "cache"),
        ("查询", "query"),
        ("导航", "navigate"),
        ("源码", "source"),
        ("插件", "addon"),
        ("安装", "install"),
        ("更新", "update"),
        ("会话", "session"),
        ("结构", "schema"),
        ("热修", "hotfix"),
    ] {
        if query.contains(needle) {
            terms.push(alias.into());
        }
    }
    terms.sort();
    terms.dedup();
    terms
}
fn rank(query: &str) -> Vec<&'static Definition> {
    let query = query.trim().to_lowercase();
    let terms = terms(&query);
    // Derive task vocabulary from the pure authoritative protocol constructor.
    // No independently invented Data task semantics or retired CLI aliases.
    let data_vocabulary = lycheedev_data::agent_session::schema()
        .to_string()
        .to_lowercase();
    let mut ranked: Vec<_> = contract::all()
        .iter()
        .filter_map(|definition| {
            let summary = if matches!(
                definition.path.as_str(),
                "data agent serve" | "data agent schema"
            ) {
                format!("{} {}", definition.summary, data_vocabulary).to_lowercase()
            } else {
                definition.summary.to_lowercase()
            };
            let score = terms
                .iter()
                .map(|term| {
                    if definition
                        .path
                        .split_whitespace()
                        .any(|word| word == term.as_str())
                    {
                        20
                    } else if definition.path.contains(term.as_str()) {
                        10
                    } else if summary.contains(term.as_str()) {
                        2
                    } else {
                        0
                    }
                })
                .sum::<usize>()
                + if definition.path == query { 1000 } else { 0 };
            (score > 0).then_some((score, definition))
        })
        .collect();
    ranked.sort_by(|(a, da), (b, db)| b.cmp(a).then_with(|| da.path.cmp(&db.path)));
    ranked
        .into_iter()
        .take(SEARCH_LIMIT)
        .map(|(_, d)| d)
        .collect()
}
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().take(128).collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ac) in a.chars().take(128).enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, bc) in b.iter().enumerate() {
            let old = row[j + 1];
            row[j + 1] = (row[j] + 1)
                .min(old + 1)
                .min(previous + usize::from(ac != *bc));
            previous = old;
        }
    }
    row[b.len()]
}
fn nearest(query: &str) -> Vec<Value> {
    let mut definitions: Vec<_> = contract::all().iter().collect();
    definitions.sort_by(|a, b| {
        distance(query, &a.path)
            .cmp(&distance(query, &b.path))
            .then_with(|| a.path.cmp(&b.path))
    });
    definitions
        .into_iter()
        .take(SEARCH_LIMIT)
        .map(brief)
        .collect()
}
fn input_schema(definition: &Definition) -> Value {
    let flags: Vec<Value> = definition.flags.iter().map(|flag| {
        let name = flag.name();
        json!({"name":name,
        "type":if flag.takes_value(){"string"}else{"boolean"},"arity":usize::from(flag.takes_value()),
        "repeatable":repeatable(&definition.path,name),
        "required":definition.required.iter().any(|required|required==name),
        "default":definition.defaults.get(name),
        "contract":flag.0,"enum":enum_values(flag)})
    }).collect();
    json!({"arguments":definition.arguments.iter().map(|name|json!({"name":name,"type":"string", "required":true,
        "variadic":definition.path == "schema"})).collect::<Vec<_>>(),"flags":flags,
        "requiredFlags":definition.required,
        "defaults":definition.defaults,
        "mutuallyExclusive":definition.mutually_exclusive,
        "oneOfRequired":definition.one_of_required,
        "constraints":definition.constraints,
        "examples":definition.examples,
        "metaFlags":[{"name":"--help","short":"-h","type":"boolean","required":false}],
        "validation":"CLI token shape and option contracts; constraints and handler-specific semantic requirements remain authoritative"})
}
fn file_input(path: &str) -> Option<Value> {
    match path {
        "source batch" => Some(json!({"flag":"--file",
            "description":"UTF-8 JSON file containing the typed batch requests below. Every item shares the selected fixed source and environment, while retaining its own cursor and coverage.",
            "schema":lycheedev_source::batch::schema()})),
        "source validate" => Some(json!({"flag":"--matrix",
            "description":"JSON matrix configuration only. Matrix targets each select their own source/product/ref; --matrix forbids --snapshot, --path, --toc and --environment. For ordinary single-addon validation, --path is the addon directory and --toc is its relative TOC manifest, not a JSON configuration file.",
            "schema":lycheedev_source::matrix::schema()})),
        _ => None,
    }
}
fn attach_agent_protocol(result: &mut Value, path: &str) {
    if !matches!(path, "data agent serve" | "data agent schema") {
        return;
    }
    let protocol = lycheedev_data::agent_session::schema();
    if path == "data agent serve" {
        result["result"] = json!({
            "protocol":protocol["protocol"],"transport":protocol["transport"],
            "jsonl":{"beginFrame":"absent","endFrame":"absent","responseId":"matches request id"},
            "responseSchemaPointer":"/protocol/responseSchema","resultSchemasPointer":"/protocol/resultSchemas",
            "notes":protocol["responseSchema"]["description"],"lifecycle":protocol["lifecycle"]});
    }
    result["protocol"] = protocol;
}
fn exact_schema(path: &str) -> Result<Value> {
    let definition = contract::all().iter().find(|d|d.path == path).ok_or_else(|| {
        let mut error = ToolkitError::arguments("schema requires an exact command path");
        error.details = json!({"command":path.chars().take(256).collect::<String>(),"suggestions":nearest(path),
            "next":{"discovery":DISCOVERY,"schema":"lycheedev schema <command...>"},"discovery":DISCOVERY}); error
    })?;
    let mut result = json!({"schema":"lycheedev.cli-command.v1","command":definition.path,
        "summary":definition.summary,"mutates":definition.mutates,"input":input_schema(definition),
        "effects":crate::discovery::effects(definition),
        "prerequisites":crate::discovery::prerequisites(definition),
        "result":crate::discovery::result_contract(),
        "examples":definition.examples,
        "errors":{"exitCodes":crate::discovery::exit_codes(),"discovery":DISCOVERY}});
    attach_agent_protocol(&mut result, path);
    if path == "update" {
        result["updatePolicy"] = lycheedev_delivery::npm_update::update_policy();
    }
    if let Some(input) = file_input(path) {
        result["fileInput"] = input;
    }
    Ok(result)
}
fn help(inv: &Invocation) -> Result<Value> {
    if let Some(definition) = contract::all().iter().find(|d| d.path == inv.command) {
        let mut result = json!({"schema":"lycheedev.cli-help.v1","kind":"command","command":definition.path,
            "commands":[definition],"flags":definition.flags.iter().map(|f|f.0.clone()).chain(std::iter::once("--help".into())).collect::<Vec<_>>(),
            "input":input_schema(definition),
            "effects":crate::discovery::effects(definition),
            "prerequisites":crate::discovery::prerequisites(definition),
            "result":crate::discovery::result_contract(),
            "examples":definition.examples,
            "errors":{"exitCodes":crate::discovery::exit_codes(),"discovery":DISCOVERY},
            "discovery":DISCOVERY,"schemaCommand":format!("lycheedev schema {}",definition.path)});
        attach_agent_protocol(&mut result, &definition.path);
        if let Some(input) = file_input(&definition.path) {
            result["fileInput"] = input;
        }
        if definition.path == "update" {
            result["updatePolicy"] = lycheedev_delivery::npm_update::update_policy();
        }
        return Ok(result);
    }
    let children = children(&inv.command);
    let total = children.len();
    let children: Vec<Value> = children.into_iter().take(CHILD_LIMIT).map(|child| {
        let path = if inv.command.is_empty(){child}else{format!("{} {child}",inv.command)};
        let definition = contract::all().iter().find(|d|d.path == path);
        json!({"command":path,"kind":if definition.is_some(){"command"}else{"group"},
            "summary":definition.map(|d|d.summary.chars().take(256).collect::<String>()),"helpCommand":format!("lycheedev {path} --help")})
    }).collect();
    Ok(
        json!({"schema":"lycheedev.cli-help.v1","kind":"group","command":inv.command,
        "children":children,"totalChildren":total,"truncated":total > CHILD_LIMIT,
        "flags":["--home <root>","--format text|json|jsonl","--help"],"discovery":DISCOVERY,
        "schemaCommand":"lycheedev schema <command...>","limits":{"children":CHILD_LIMIT,"searchResults":SEARCH_LIMIT}}),
    )
}
pub fn discover(inv: &Invocation) -> Option<Result<Outcome>> {
    let result = if inv.flag_bool("--help") {
        help(inv)
    } else if inv.command == "schema" {
        exact_schema(inv.positional.as_deref().unwrap_or(""))
    } else if inv.command == "cli search" {
        let query = inv.positional.as_deref().unwrap_or("");
        if query.trim().is_empty() || query.len() > QUERY_LIMIT {
            Err(ToolkitError::arguments(
                "task must contain 1..512 UTF-8 bytes and non-whitespace text",
            ))
        } else {
            let products = crate::discovery::product_matches(query);
            let mut next = json!({"schema":"lycheedev schema <command...>","help":"lycheedev <command> --help"});
            if !products.is_empty() {
                let forever = products
                    .iter()
                    .any(|product| product["family"] == "forever");
                next["product"] = if forever {
                    json!(
                        "Choose forever-asia-beta or forever-cn-beta by region; equal builds do not merge them."
                    )
                } else {
                    json!(
                        "Use the matched product identity as-is; keep its exact series/build and TOC interface."
                    )
                };
            }
            Ok(
                json!({"schema":"lycheedev.cli-search.v1","query":query,"limit":SEARCH_LIMIT,
            "matches":rank(query).into_iter().map(brief).collect::<Vec<_>>(),
            "products":products,"next":next}),
            )
        }
    } else if inv.command == "data agent schema" {
        Ok(lycheedev_data::agent_session::schema())
    } else {
        return None;
    };
    Some(result.map(Outcome::new))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn source_batch_public_file_schema_examples_enter_strict_typed_admission() {
        let public = discover(&parse(&args(&["schema", "source", "batch"])).unwrap())
            .unwrap().unwrap().data;
        assert_eq!(public["fileInput"]["flag"], "--file");
        let help = discover(&parse(&args(&["source", "batch", "--help"])).unwrap())
            .unwrap().unwrap().data;
        assert_eq!(help["fileInput"], public["fileInput"]);
        let schema = &public["fileInput"]["schema"];
        for (kind, field, default, minimum, maximum) in [
            ("query", "limit", 10, 1, 200),
            ("refs", "limit", 25, 1, 200),
            ("context", "limit", 25, 1, 200),
            ("context", "maxBytes", 16384, 1, 1_048_576),
            ("context", "maxLines", 80, 1, 2000),
            ("context", "depth", 0, 0, 4),
        ] {
            let property = &schema["$defs"][kind]["properties"][field];
            assert_eq!(property["default"], default, "{kind}.{field} default");
            assert_eq!(property["minimum"], minimum, "{kind}.{field} minimum");
            assert_eq!(property["maximum"], maximum, "{kind}.{field} maximum");
        }
        for pointer in [
            "/properties/requests/items/$ref",
            "/$defs/item/properties/query/$ref",
            "/$defs/item/properties/refs/$ref",
            "/$defs/item/properties/context/$ref",
        ] {
            let reference = schema.pointer(pointer).and_then(Value::as_str).unwrap();
            assert!(reference.starts_with("#/"), "{reference}");
            assert!(schema.pointer(&reference[1..]).is_some(), "unresolved {reference}");
        }
        let examples = schema["examples"].as_array()
            .expect("source batch must publish complete JSON file examples");
        assert!(!examples.is_empty());
        for example in examples {
            lycheedev_source::batch::parse(&serde_json::to_vec(example).unwrap()).unwrap();
        }
        let mut input = examples[0].clone();
        let items = input["requests"].as_array_mut().unwrap();
        assert_eq!(items.len(), 3, "the minimal example must cover query, refs and context");
        for (kind, fields) in [
            ("query", json!({"text":"SchemaQuery"})),
            ("refs", json!({"symbol":"SchemaRefs"})),
            ("context", json!({"symbolId":"SYM-schema-context"})),
        ] {
            let item = items.iter_mut().find(|item| item.get(kind).is_some()).unwrap();
            item[kind] = fields;
        }
        let parsed = lycheedev_source::batch::parse(&serde_json::to_vec(&input).unwrap()).unwrap();
        let query = parsed.requests.iter().find_map(|item| item.query.as_ref()).unwrap();
        assert_eq!(query.text, "SchemaQuery");
        assert_eq!(query.limit, 10);
        let refs = parsed.requests.iter().find_map(|item| item.refs.as_ref()).unwrap();
        assert_eq!(refs.symbol, "SchemaRefs");
        assert_eq!(refs.limit, 25);
        let context = parsed.requests.iter().find_map(|item| item.context.as_ref()).unwrap();
        assert_eq!(context.symbol_id, "SYM-schema-context");
        assert_eq!((context.limit, context.max_bytes, context.max_lines, context.depth), (25, 16384, 80, 0));
        for (kind, field, values) in [
            ("query", "limit", vec![0, -1, 201]),
            ("refs", "limit", vec![0, -1, 201]),
            ("context", "limit", vec![0, -1, 201]),
            ("context", "maxBytes", vec![0, -1, 1_048_577]),
            ("context", "maxLines", vec![0, -1, 2001]),
            ("context", "depth", vec![-1, 5]),
        ] {
            for value in values {
                let mut invalid = input.clone();
                let item = invalid["requests"].as_array_mut().unwrap().iter_mut()
                    .find(|item| item.get(kind).is_some()).unwrap();
                item[kind][field] = json!(value);
                assert!(lycheedev_source::batch::parse(&serde_json::to_vec(&invalid).unwrap()).is_err(), "{kind}.{field}={value}");
            }
        }
        for kind in ["query", "refs", "context"] {
            let mut invalid = input.clone();
            let item = invalid["requests"].as_array_mut().unwrap().iter_mut()
                .find(|item| item.get(kind).is_some()).unwrap();
            item[kind]["unknown"] = json!(true);
            assert!(lycheedev_source::batch::parse(&serde_json::to_vec(&invalid).unwrap()).is_err(), "{kind} unknown field");
        }
    }

    #[test]
    fn source_matrix_public_file_schema_examples_enter_real_config_admission() {
        let public = discover(&parse(&args(&["schema", "source", "validate"])).unwrap())
            .unwrap().unwrap().data;
        assert_eq!(public["fileInput"]["flag"], "--matrix");
        let help = discover(&parse(&args(&["source", "validate", "--help"])).unwrap())
            .unwrap().unwrap().data;
        assert_eq!(help["fileInput"], public["fileInput"]);
        let schema = &public["fileInput"]["schema"];
        assert_eq!(schema["properties"]["targets"]["minItems"], 1);
        assert_eq!(schema["properties"]["targets"]["maxItems"], 32);
        let examples = schema["examples"].as_array()
            .expect("source validate must publish its matrix JSON shape, distinct from --path/--toc addon inputs");
        assert!(!examples.is_empty());
        struct Directory(std::path::PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
        }
        let directory = Directory(std::env::temp_dir().join(format!(
            "lycheedev-source-file-schema-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        )));
        std::fs::create_dir(&directory.0).unwrap();
        let file = directory.0.join("matrix.json");
        let control = lycheedev_core::control::OperationControl::default();
        let read = |input: &Value| {
            std::fs::write(&file, serde_json::to_vec(input).unwrap()).unwrap();
            lycheedev_source::matrix::read_config(&file, &control)
        };
        for example in examples { read(example).unwrap(); }
        let mut input = examples[0].clone();
        input["path"] = json!("relative-addon");
        input["targets"] = json!([{"id":"schema-client","toc":"Main.toc","product":"classic","ref":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]);
        let (config, root) = read(&input).unwrap();
        assert_eq!(root, directory.0.join("relative-addon"));
        assert_eq!(config.targets[0].id, "schema-client");
        assert_eq!(config.targets[0].toc, "Main.toc");
        assert_eq!(config.targets[0].product, "classic");
        assert_eq!(config.targets[0].reference, "a".repeat(40));
        assert!(config.targets[0].source.is_empty(), "omitted source retains the owning resolver's wow-ui-source default");
        let mut default_path = input.clone();
        default_path.as_object_mut().unwrap().remove("path");
        assert_eq!(read(&default_path).unwrap().1, directory.0);
        for field in ["id", "toc", "product", "ref"] {
            let mut missing = input.clone();
            missing["targets"][0].as_object_mut().unwrap().remove(field);
            assert_eq!(read(&missing).unwrap_err().code, "codebase.invalid_matrix_config", "{field}");
        }
        for count in [0, 32, 33] {
            let mut bounded = input.clone();
            bounded["targets"] = json!((0..count).map(|index| {
                let mut target = input["targets"][0].clone();
                target["id"] = json!(format!("client-{index}")); target
            }).collect::<Vec<_>>());
            assert_eq!(read(&bounded).is_ok(), count == 32, "target count {count}");
        }
        for nested in [false, true] {
            let mut unknown = input.clone();
            if nested { unknown["targets"][0]["unknown"] = json!(true); }
            else { unknown["unknown"] = json!(true); }
            assert_eq!(read(&unknown).unwrap_err().code, "codebase.invalid_matrix_config");
        }
    }

    #[test]
    fn clap_tree_and_every_exact_help_are_metadata_only() {
        parser().debug_assert();
        for definition in contract::all() {
            let mut words: Vec<String> = definition
                .path
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            words.extend(["--help".into(), "--format".into(), "json".into()]);
            let mut inv = parse(&words);
            let guard = crate::install_invocation_control(
                &mut inv,
                |_| -> std::result::Result<(), String> {
                    panic!("metadata installed business cancellation console")
                },
            );
            assert!(guard.is_none());
            let result = discover(&inv.unwrap()).unwrap().unwrap().data;
            assert_eq!(result["commands"], json!([definition]));
        }
    }
    #[test]
    fn root_and_group_help_are_immediate_bounded_explicit() {
        for (words, path) in [
            (vec![], ""),
            (vec!["data"], "data"),
            (vec!["data", "agent", "--help"], "data agent"),
        ] {
            let result = discover(&parse(&args(&words)).unwrap())
                .unwrap()
                .unwrap()
                .data;
            assert_eq!(result["schema"], "lycheedev.cli-help.v1");
            assert_eq!(result["kind"], "group");
            assert_eq!(result["command"], path);
            assert!(result["commands"].is_null());
            let children = result["children"].as_array().unwrap();
            assert!(!children.is_empty() && children.len() <= CHILD_LIMIT);
            assert_eq!(result["totalChildren"], children.len());
            assert_eq!(result["truncated"], false);
            for child in children {
                let command = child["command"].as_str().unwrap();
                assert_eq!(
                    command.split_whitespace().count(),
                    path.split_whitespace().count() + 1
                );
                assert_eq!(child["helpCommand"], format!("lycheedev {command} --help"));
            }
        }
    }
    #[test]
    fn exact_schema_uses_authoritative_protocol_without_session() {
        let result = discover(&parse(&args(&["schema", "data", "agent", "serve"])).unwrap())
            .unwrap()
            .unwrap()
            .data;
        assert_eq!(result["command"], "data agent serve");
        assert_eq!(result["protocol"], lycheedev_data::agent_session::schema());
        assert!(exact_schema("data agent").is_err());
        assert!(exact_schema("data sql").is_err());
        for words in [
            vec!["cli", "search", "query"],
            vec!["schema", "version"],
            vec!["data", "agent", "schema"],
        ] {
            let mut inv = parse(&args(&words));
            assert!(
                crate::install_invocation_control(
                    &mut inv,
                    |_| -> std::result::Result<(), String> {
                        panic!("discovery installed console")
                    }
                )
                .is_none()
            );
            assert!(discover(&inv.unwrap()).unwrap().is_ok());
        }
    }
    #[test]
    fn serve_schema_and_help_describe_agent_frames_while_metadata_keeps_cli_framing() {
        let home = std::env::temp_dir().join(format!("agent-framing-unused-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        for words in [args(&["schema", "data", "agent", "serve"]),
            args(&["data", "agent", "serve", "--help"])] {
            let mut inv = parse(&words).unwrap();
            inv.home = home.clone();
            let outcome = discover(&inv).unwrap().unwrap();
            let result = &outcome.data["result"];
            assert!(result.get("envelope").is_none());
            assert!(result.get("responseSchema").is_none(), "embed the protocol schema only once");
            assert!(result.get("resultSchemas").is_none());
            assert_eq!(result["protocol"], "lycheedev.data-agent.v1");
            assert_eq!(result["jsonl"], json!({"beginFrame":"absent","endFrame":"absent","responseId":"matches request id"}));
            let frame_pointer = result["responseSchemaPointer"].as_str().unwrap();
            assert_eq!(frame_pointer, "/protocol/responseSchema");
            let frames = &outcome.data.pointer(frame_pointer).unwrap()["$defs"];
            assert_eq!(frames["SuccessFrame"]["properties"]["protocol"]["const"], "lycheedev.data-agent.v1");
            assert_eq!(frames["TerminalDiagnostic"]["x-stream"], "stderr");
            assert_eq!(frames["TerminalDiagnostic"]["required"], json!(["protocol", "ok", "error"]));
            let payloads = outcome.data.pointer(result["resultSchemasPointer"].as_str().unwrap()).unwrap();
            assert_eq!(payloads["close"]["properties"]["closed"]["const"], true);
            assert!(result["notes"].as_str().unwrap().contains("process status independently"));
            // The metadata operation itself still uses the normal CLI renderer.
            let mut stdout = Vec::new();
            crate::render(&mut stdout, "jsonl", &Ok(outcome)).unwrap();
            let frames: Vec<Value> = String::from_utf8(stdout).unwrap().lines()
                .map(|line| serde_json::from_str(line).unwrap()).collect();
            assert_eq!(frames.first().unwrap()["type"], "begin");
            assert_eq!(frames.last().unwrap()["type"], "end");
        }
        let schema_command = discover(&parse(&args(&["schema", "data", "agent", "schema"])).unwrap())
            .unwrap().unwrap().data;
        assert_eq!(schema_command["result"]["schema"], "lycheedev.result.v1");
        assert_eq!(schema_command["result"]["jsonl"]["endFrame"], "required");
        assert!(!home.exists());
    }
    #[test]
    fn always_required_metadata_is_exact_for_the_remaining_typed_admissions() {
        for (path, required) in [
            ("data hotfix", vec!["--source"]),
            ("live probe put", vec!["--name", "--file"]),
            ("live execute", vec!["--session", "--request", "--budget-seconds"]),
            ("live bugs", vec!["--session", "--request"]),
            ("live reload", vec!["--session", "--request"]),
            ("live reload fallback", vec!["--installation", "--pid", "--request"]),
            ("asset inspect", vec!["--file-id"]),
            ("asset search", vec!["--listfile"]),
            ("asset export", vec!["--file-id", "--output"]),
            ("asset demux", vec!["--output"]),
            ("target add", vec!["--product", "--region", "--locale"]),
            ("evidence bundle", vec!["--ids", "--output"]),
            ("source batch", vec!["--file"]),
            ("source diff", vec!["--from", "--to"]),
            ("source prune", vec!["--target-bytes"]),
        ] {
            let mut words = vec!["schema".to_owned()];
            words.extend(path.split_whitespace().map(str::to_owned));
            let schema = discover(&parse(&words).unwrap()).unwrap().unwrap().data;
            assert_eq!(schema["input"]["requiredFlags"], json!(required), "{path}");
            for flag in schema["input"]["flags"].as_array().unwrap() {
                assert_eq!(flag["required"], required.contains(&flag["name"].as_str().unwrap()), "{path}: {flag}");
            }
        }
        for path in ["source validate", "source inspect", "source refs", "source context", "target resolve", "doctor"] {
            assert_eq!(exact_schema(path).unwrap()["input"]["requiredFlags"], json!([]), "{path}");
        }
    }
    #[test]
    fn search_is_deterministic_bounded_and_never_guesses_on_zero_matches() {
        let first: Vec<_> = rank("spell effects").into_iter().map(brief).collect();
        assert!(first.len() <= SEARCH_LIMIT);
        assert!(
            first
                .iter()
                .any(|value| value["command"] == "data agent serve")
        );
        assert_eq!(
            first,
            rank("spell effects")
                .into_iter()
                .map(brief)
                .collect::<Vec<_>>()
        );
        assert!(rank("zzzzzznotacapability").is_empty());
        assert!(
            rank("查询法术")
                .iter()
                .any(|d| d.path == "data agent serve")
        );
        assert!(
            discover(&parse(&args(&["cli", "search", " "])).unwrap())
                .unwrap()
                .is_err()
        );
        assert!(parse(&args(&["cli", "search", "spell", "effects"])).is_err());
    }
    #[test]
    fn product_search_returns_both_betas_and_a_conditional_next_hint() {
        let outcome = discover(&parse(&args(&["cli", "search", "无限服"])).unwrap())
            .unwrap()
            .unwrap()
            .data;
        let ids: Vec<_> = outcome["products"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"forever-asia-beta"), "{ids:?}");
        assert!(ids.contains(&"forever-cn-beta"), "{ids:?}");
        assert!(outcome["next"]["product"].is_string());
        let outcome = discover(&parse(&args(&["cli", "search", "spell"])).unwrap())
            .unwrap()
            .unwrap()
            .data;
        assert!(outcome["next"]["product"].is_null());
        let outcome = discover(&parse(&args(&["cli", "search", "时光服"])).unwrap())
            .unwrap()
            .unwrap()
            .data;
        assert!(
            outcome["products"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == "titan")
        );
        let hint = outcome["next"]["product"].as_str().unwrap();
        assert!(hint.contains("product identity"));
        assert!(!hint.contains("forever-asia-beta"));
    }
    #[test]
    fn structured_contract_exposes_required_defaults_exclusions_and_examples() {
        let schema = |path: &str| {
            discover(&parse(&args(&["schema", path, "--format", "json"])).unwrap())
                .unwrap()
                .unwrap()
                .data
        };
        let install = schema("addon install");
        assert_eq!(install["input"]["requiredFlags"], json!(["--installation"]));
        assert_eq!(
            install["input"]["oneOfRequired"],
            json!([["--release", "--resume"]])
        );
        let flags = install["input"]["flags"].as_array().unwrap();
        let installation = flags
            .iter()
            .find(|flag| flag["name"] == "--installation")
            .unwrap();
        assert_eq!(installation["required"], true);
        let serve = schema("data agent serve");
        assert_eq!(serve["input"]["defaults"]["--max-objects"], "32");
        assert_eq!(serve["input"]["defaults"]["--timeout-seconds"], "300");
        assert!(
            serve["input"]["constraints"]
                .as_array()
                .unwrap()
                .contains(&json!(
                    "--format must be jsonl for the supervised Data Agent worker."
                ))
        );
        let init = schema("init");
        assert_eq!(
            init["input"]["mutuallyExclusive"],
            json!([["--plan", "--fresh", "--resume"]])
        );
        let add = schema("target add");
        assert_eq!(
            add["input"]["oneOfRequired"],
            json!([["--installation", "--remote"]])
        );
        let described = crate::describe_commands();
        for definition in contract::all() {
            let path = definition.path.as_str();
            let data = schema(path);
            let expected = json!(definition.examples);
            assert!(!definition.examples.is_empty(), "{path}");
            assert_eq!(data["examples"], expected, "{path}/schema");
            assert_eq!(data["input"]["examples"], expected, "{path}/input");
            assert_eq!(
                data["input"]["constraints"],
                json!(definition.constraints),
                "{path}/constraints"
            );
            let described = described.iter().find(|row| row["path"] == path).unwrap();
            assert_eq!(described["examples"], expected, "{path}/describe");
            assert_eq!(
                described["effects"], data["effects"],
                "{path}/describe-effects"
            );
            let mut help_words: Vec<String> = path.split_whitespace().map(str::to_owned).collect();
            help_words.push("--help".into());
            let help = help(&parse(&help_words).unwrap()).unwrap();
            assert_eq!(help["examples"], expected, "{path}/help");
            assert_eq!(help["effects"], data["effects"], "{path}/help-effects");
            assert_eq!(help["input"]["examples"], expected, "{path}/help-input");
            for command in &definition.examples {
                let mut tokens = command.split_whitespace();
                assert_eq!(tokens.next(), Some("lycheedev"), "{path}");
                let words: Vec<String> = tokens.map(str::to_owned).collect();
                let inv = parse(&words).unwrap_or_else(|error| {
                    panic!("{path} example does not parse: {command}: {error}")
                });
                assert_eq!(inv.command, path, "{command}");
                assert!(!inv.flag_bool("--help"), "{path} needs a business example");
                for required in &definition.required {
                    assert!(inv.flag(required).is_some_and(|value| !value.is_empty()),
                        "{path} example omits {required}: {command}");
                }
            }
        }
    }
    #[test]
    fn serve_help_accepts_json_but_worker_does_not() {
        let inv = parse(&args(&[
            "data", "agent", "serve", "--help", "--format", "json",
        ]))
        .unwrap();
        assert!(inv.flag_bool("--help"));
        assert_eq!(inv.flag("--format"), Some("json"));
        assert_eq!(
            discover(&inv).unwrap().unwrap().data["schema"],
            "lycheedev.cli-help.v1"
        );
        let error = parse(&args(&["data", "agent", "serve", "--format", "json"])).unwrap_err();
        assert_eq!(error.code, "command.invalid_arguments");
        assert_eq!(error.details["kind"], "InvalidValue");
    }
    #[test]
    fn clap_rejects_bad_arity_enum_repeats_and_unknowns_with_machine_errors() {
        for words in [
            vec!["versoin"],
            vec!["version", "--bogus"],
            vec!["version", "--home="],
            vec!["version", "--format", "yaml"],
            vec!["asset", "export", "--encoding", "invalid"],
            vec!["version", "--format", "json", "--format", "jsonl"],
        ] {
            let error = parse(&args(&words)).unwrap_err();
            assert_eq!(error.code, "command.invalid_arguments");
            assert_eq!(error.details["parser"], "clap");
            assert!(error.details["suggestions"].as_array().unwrap().len() <= SEARCH_LIMIT);
        }
        assert!(parse(&args(&["target", "show", "--help"])).is_ok());
        assert!(parse(&args(&["target", "show"])).is_err());
        let inv = parse(&args(&["update", "--path", "a", "--path", "b", "--help"])).unwrap();
        assert_eq!(inv.flags["__repeat:--path"], "[\"a\",\"b\"]");
    }
    #[test]
    fn known_command_argument_errors_offer_its_own_metadata_correction() {
        let home = std::env::temp_dir().join(format!(
            "lycheedev-argument-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        assert!(!home.exists());
        let words = vec![
            "--home".into(), home.to_string_lossy().into_owned(),
            "version".into(), "--timeout-seconds".into(), "1".into(),
        ];
        let error = parse(&words).unwrap_err();
        assert_eq!(error.code, "command.invalid_arguments");
        assert_eq!(error.exit_code, 2);
        let details = &error.details;
        assert_eq!(details["command"], "version");
        assert_eq!(details["usage"], "lycheedev version [OPTIONS]");
        assert_eq!(details["acceptedFlags"], json!(["--home", "--format", "--help", "-h"]));
        assert_eq!(details["arguments"], json!([]));
        assert_eq!(details["schemaCommand"], "lycheedev schema version");
        assert_eq!(details["helpCommand"], "lycheedev version --help");
        assert_eq!(details["suggestions"], json!([]));
        let mut rendered = Vec::new();
        crate::render(&mut rendered, "json", &Err(error.clone())).unwrap();
        let envelope: Value = serde_json::from_slice(&rendered).unwrap();
        assert_eq!(envelope["error"]["details"], *details);
        assert_eq!(envelope["ok"], false);
        // Follow both returned metadata corrections in the same absent home.
        for key in ["schemaCommand", "helpCommand"] {
            let correction = details[key].as_str().unwrap().split_whitespace()
                .skip(1).map(str::to_owned).collect::<Vec<_>>();
            let mut parsed = parse(&correction);
            parsed.as_mut().unwrap().home = home.clone();
            let guard = crate::install_invocation_control(&mut parsed, |_| -> std::result::Result<(), String> {
                panic!("argument correction installed a business console/worker owner")
            });
            assert!(guard.is_none());
            let result = discover(&parsed.unwrap()).unwrap().unwrap();
            assert_eq!(result.data["command"], "version");
        }
        assert!(!home.exists());
    }
    #[test]
    fn missing_values_and_positionals_keep_the_recognized_leaf_contract() {
        for (words, command, argument) in [
            (vec!["source", "query", "Needle", "--limit"], "source query", "<term>"),
            (vec!["target", "show"], "target show", "<name-or-pin-id>"),
        ] {
            let error = parse(&args(&words)).unwrap_err();
            assert_eq!(error.exit_code, 2);
            assert_eq!(error.details["command"], command);
            assert_eq!(error.details["arguments"], json!([argument]));
            assert_eq!(error.details["schemaCommand"], format!("lycheedev schema {command}"));
            assert_eq!(error.details["helpCommand"], format!("lycheedev {command} --help"));
            assert_eq!(error.details["suggestions"], json!([]));
        }
        let error = parse(&args(&["data", "agent", "serve", "--format", "json"])).unwrap_err();
        assert_eq!(error.details["command"], "data agent serve");
        assert_eq!(error.details["requiredFlags"], json!(["--format"]));
        assert_eq!(error.details["allowed"], json!(["jsonl"]));
    }
    #[test]
    fn public_required_source_and_project_flags_enter_typed_admission() {
        for (command, required, values) in [
            ("source sync", vec!["--source", "--product"], vec!["wow-ui-source", "retail"]),
            ("project init", vec!["--product"], vec!["retail"]),
            ("project lock", vec!["--snapshot"], vec!["PIN-fixed"]),
        ] {
            let mut schema_words = vec!["schema".to_owned()];
            schema_words.extend(command.split_whitespace().map(str::to_owned));
            let schema = discover(&parse(&schema_words).unwrap()).unwrap().unwrap().data;
            assert_eq!(schema["input"]["requiredFlags"], json!(required));
            for flag in &required {
                assert_eq!(schema["input"]["flags"].as_array().unwrap().iter()
                    .find(|row| row["name"] == *flag).unwrap()["required"], true);
            }
            let mut words = command.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
            for (flag, value) in required.iter().zip(&values) { words.extend([flag.to_string(), value.to_string()]); }
            let inv = parse(&words).unwrap();
            match command {
                "source sync" => assert!(matches!(crate::source_cli::admit(&inv).unwrap(), crate::source_cli::SourceCommand::Ordinary(_))),
                "project init" => assert!(matches!(crate::core_workspace_cli::admit_project(&inv).unwrap(),
                    lycheedev_core::selection::ProjectRequest::Init { product, path } if product=="retail" && path==std::path::PathBuf::from("."))),
                "project lock" => assert!(matches!(crate::core_workspace_cli::admit_project(&inv).unwrap(),
                    lycheedev_core::selection::ProjectRequest::Lock { snapshot, path:None } if snapshot=="PIN-fixed")),
                _ => unreachable!(),
            }
            for missing in &required {
                let mut omitted = words.clone();
                let at = omitted.iter().position(|word| word.as_str()==*missing).unwrap();
                omitted.drain(at..at+2);
                let inv = parse(&omitted).unwrap();
                let error = if command=="source sync" { crate::source_cli::admit(&inv).err().unwrap() }
                    else { crate::core_workspace_cli::admit_project(&inv).unwrap_err() };
                assert_eq!((error.code.as_str(),error.exit_code),("command.invalid_arguments",2));
                assert!(error.message.contains(missing), "{command}: {}", error.message);
            }
        }
    }
    #[test]
    fn public_update_discovery_uses_compiled_channel_authority_without_io() {
        let authority: Value = serde_json::from_str(include_str!("../../../release/npm-channel.json")).unwrap();
        let home = std::env::temp_dir().join(format!("lycheedev-update-discovery-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        assert!(!home.exists());
        let mut results = Vec::new();
        for words in [vec!["schema", "update"], vec!["update", "--help"], vec!["describe"]] {
            let mut inv = parse(&args(&words));
            inv.as_mut().unwrap().home = home.clone();
            let guard = crate::install_invocation_control(&mut inv, |_| -> std::result::Result<(), String> {
                panic!("update discovery installed business cancellation/worker owner")
            });
            assert!(guard.is_none());
            results.push(crate::dispatch(&inv.unwrap()).unwrap().data);
        }
        let policy = &results[0]["updatePolicy"];
        assert_eq!(policy["schema"], "lycheedev.update-policy.v1");
        assert_eq!(policy["version"], crate::VERSION);
        assert_eq!(policy["npmChannel"], authority);
        assert_eq!(policy["registrySelector"], format!("lycheedev@{}", authority["distTag"].as_str().unwrap()));
        assert_eq!(policy["selectors"], json!({"version":false,"channel":false}));
        for mode in ["registry", "plan", "release"] {
            assert!(!policy["modes"][mode].as_str().unwrap().is_empty());
        }
        assert_eq!(results[1]["updatePolicy"], *policy);
        assert_eq!(results[2]["updatePolicy"], *policy);
        assert!(!home.exists());
    }
    #[test]
    fn misspelled_commands_keep_search_without_inventing_a_parsed_leaf() {
        for words in [vec!["versoin"], vec!["--home", "not-a-command-but-an-option-value", "versoin"]] {
            let error = parse(&args(&words)).unwrap_err();
            assert_eq!(error.code, "command.invalid_arguments");
            assert_eq!(error.exit_code, 2);
            assert_eq!(error.details["suggestions"][0]["command"], "version");
            assert!(error.details["command"].is_null());
        }
        let error = parse(&args(&["sorce", "sync"])).unwrap_err();
        assert_eq!(error.details["suggestions"][0]["command"], "source sync");
        assert!(error.details["command"].is_null());
        let error = parse(&args(&["--timeout-seconds", "1", "version"])).unwrap_err();
        assert!(error.details["command"].is_null(), "a later token was never parsed as a command");
        assert!(error.details["schemaCommand"].is_null());
    }
    #[test]
    fn retired_routes_have_no_parser_or_schema_entry() {
        for path in [
            "data sql",
            "data db2",
            "data spell info",
            "data item get",
            "live cancel",
            "live abandon",
        ] {
            let mut words: Vec<String> = path.split_whitespace().map(str::to_owned).collect();
            words.push("--help".into());
            let error = parse(&words).unwrap_err();
            assert_eq!(error.code, "command.invalid_arguments");
            assert!(exact_schema(path).is_err());
        }
        assert!(parse(&args(&["--source-broker-server"])).is_err());
    }
}
