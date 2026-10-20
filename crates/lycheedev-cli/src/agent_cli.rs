//! Reimplemented after artifact loss. Discovery reads immutable metadata only:
//! no project, home, cache, engine, worker, network or game is opened here.
use crate::contract::{self, Definition, Flag};
use clap::{Arg, ArgAction, ArgMatches, Command, builder::{NonEmptyStringValueParser, PossibleValuesParser}};
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError};
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
    if !matches!(flag.name(), "--format" | "--content-variant" | "--direction"
        | "--encoding" | "--mode" | "--policy" | "--product" | "--region" | "--source" | "--topic") {
        return None;
    }
    let token = flag.0.split_whitespace().nth(1)?;
    if !token.contains('|') && !matches!(flag.name(), "--format" | "--encoding") { return None; }
    let mut values: Vec<String> = token.trim_matches(|c| c == '<' || c == '>').split('|').map(str::to_owned).collect();
    if flag.name() == "--product" && flag.0.contains("legacy: forever") { values.push("forever".into()); }
    Some(values)
}
fn option(command: &str, flag: &Flag) -> Arg {
    let id = flag.name().trim_start_matches("--").to_owned();
    let arg = Arg::new(id.clone()).long(id).help(flag.0.clone());
    if !flag.takes_value() { return arg.action(ArgAction::SetTrue); }
    let mut arg = arg.num_args(1).value_parser(NonEmptyStringValueParser::new())
        .action(if repeatable(command, flag.name()) { ArgAction::Append } else { ArgAction::Set });
    if let Some(values) = enum_values(flag) { arg = arg.value_parser(PossibleValuesParser::new(values)); }
    arg
}
fn children(prefix: &str) -> Vec<String> {
    let stem = if prefix.is_empty() { String::new() } else { format!("{prefix} ") };
    contract::all().iter().filter_map(|d| d.path.strip_prefix(&stem))
        .filter_map(|tail| tail.split_whitespace().next()).map(str::to_owned)
        .collect::<BTreeSet<_>>().into_iter().collect()
}
fn node(name: &str, path: &str) -> Command {
    let mut command = Command::new(name.to_owned()).disable_help_flag(true)
        .disable_help_subcommand(true).disable_version_flag(true)
        .arg_required_else_help(false);
    if let Some(definition) = contract::all().iter().find(|d| d.path == path) {
        command = command.about(definition.summary.clone());
        for flag in &definition.flags {
            if !matches!(flag.name(), "--format" | "--home") { command = command.arg(option(path, flag)); }
        }
        if let Some(argument) = definition.arguments.first() {
            let mut arg = Arg::new("positional").value_name(argument.clone())
                .value_parser(NonEmptyStringValueParser::new()).required_unless_present("help");
            if path == "schema" { arg = arg.num_args(1..).action(ArgAction::Append); }
            command = command.arg(arg);
        }
    }
    for child in children(path) {
        let next = if path.is_empty() { child.clone() } else { format!("{path} {child}") };
        command = command.subcommand(node(&child, &next));
    }
    command
}
fn parser() -> Command {
    node("lycheedev", "")
        .arg(Arg::new("help").long("help").short('h').global(true)
            .action(ArgAction::SetTrue).overrides_with("help"))
        .arg(option("", &Flag("--format text|json|jsonl".into())).global(true))
        .arg(option("", &Flag("--home <root>".into())).global(true))
}
fn leaf(mut matches: &ArgMatches) -> (String, &ArgMatches) {
    let mut path = Vec::new();
    while let Some((name, child)) = matches.subcommand() { path.push(name); matches = child; }
    (path.join(" "), matches)
}
fn parse_error(error: clap::Error, args: &[String]) -> ToolkitError {
    let query = args.iter().filter(|s| !s.starts_with('-')).take(8)
        .map(String::as_str).collect::<Vec<_>>().join(" ");
    let mut failure = ToolkitError::arguments("invalid command arguments");
    // Do not call clap's print/exit APIs. Existing envelopes own both streams.
    failure.details = json!({"parser":"clap","kind":format!("{:?}",error.kind()),
        "diagnostic":error.to_string().chars().take(2048).collect::<String>(),
        "suggestions":nearest(&query),"discovery":DISCOVERY});
    failure
}
pub fn parse(args: &[String]) -> Result<Invocation> {
    let matches = parser().try_get_matches_from(std::iter::once("lycheedev".to_owned()).chain(args.iter().cloned()))
        .map_err(|error| parse_error(error, args))?;
    let (command, matched) = leaf(&matches);
    let definition = contract::all().iter().find(|d| d.path == command);
    let help = matched.get_flag("help") || definition.is_none();
    let mut flags = BTreeMap::new();
    let mut ordered = Vec::new();
    if help { flags.insert("--help".into(), "true".into()); }
    // Global placement does not authorize --home for a command without it.
    if matched.get_one::<String>("home").is_some()
        && definition.is_some_and(|d| !d.flags.iter().any(|f| f.name() == "--home")) {
        let mut error = ToolkitError::arguments(format!("--home is not supported by {command}"));
        error.details = json!({"parser":"clap","kind":"UnknownArgument","flag":"--home"});
        return Err(error);
    }
    // Help is a metadata operation, not the worker's JSONL transport. In
    // particular `data agent serve --help --format json` must remain legal.
    if !help && let Some(definition) = definition
        && let Some(value) = matched.get_one::<String>("format")
        && let Some(spec) = definition.flags.iter().find(|f| f.name() == "--format")
        && let Some(allowed) = enum_values(spec)
        && !allowed.contains(value) {
        let mut error = ToolkitError::arguments(format!("unsupported --format for {command}"));
        error.details = json!({"parser":"clap","kind":"InvalidValue","flag":"--format","allowed":allowed});
        return Err(error);
    }
    let specs = definition.map(|d| d.flags.clone()).unwrap_or_else(||
        vec![Flag("--home <root>".into()), Flag("--format text|json|jsonl".into())]);
    for flag in specs {
        let id = flag.name().trim_start_matches("--");
        if flag.takes_value() {
            if let Some(values) = matched.get_many::<String>(id) {
                let values: Vec<String> = values.cloned().collect();
                if values.len() > 256 { return Err(ToolkitError::arguments("repeated option exceeds 256 values")); }
                if let Some(last) = values.last() { flags.insert(flag.name().into(), last.clone()); }
                if values.len() > 1 { flags.insert(format!("__repeat:{}",flag.name()), serde_json::to_string(&values)?); }
                for (index, value) in matched.indices_of(id).into_iter().flatten().zip(values) {
                    ordered.push((index, flag.name().to_owned(), value));
                }
            }
        } else if matched.get_flag(id) {
            flags.insert(flag.name().into(), "true".into());
            ordered.push((matched.index_of(id).unwrap_or(0), flag.name().to_owned(), "true".into()));
        }
    }
    ordered.sort_by_key(|(index, _, _)| *index);
    let ordered: Vec<(String, String)> = ordered.into_iter().map(|(_, key, value)| (key, value)).collect();
    for (key, value) in &ordered {
        if command.starts_with("live ") || command == "doctor" { crate::formal_live::validate_option(key, value)?; }
        if command == "data hotfix" { lycheedev_data::hotfix::remote::validate_option(key, value)?; }
    }
    let positional = if definition.is_some_and(|d| !d.arguments.is_empty()) {
        matched.get_many::<String>("positional").map(|values| values.map(String::as_str).collect::<Vec<_>>().join(" "))
    } else { None };
    crate::finish_parse(command, positional, flags, ordered, help)
}
fn brief(definition: &Definition) -> Value {
    json!({"command":definition.path,"summary":definition.summary.chars().take(256).collect::<String>(),
        "mutates":definition.mutates,"schemaCommand":format!("lycheedev schema {}",definition.path)})
}
fn terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = query.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty())
        .map(str::to_lowercase).take(32).collect();
    for (needle, alias) in [("法术","spell"),("物品","item"),("生物","creature"),("技能","spell"),
        ("缓存","cache"),("查询","query"),("导航","navigate"),("源码","source"),("插件","addon"),
        ("安装","install"),("更新","update"),("会话","session"),("结构","schema"),("热修","hotfix")] {
        if query.contains(needle) { terms.push(alias.into()); }
    }
    terms.sort(); terms.dedup(); terms
}
fn rank(query: &str) -> Vec<&'static Definition> {
    let query = query.trim().to_lowercase();
    let terms = terms(&query);
    // Derive task vocabulary from the pure authoritative protocol constructor.
    // No independently invented Data task semantics or retired CLI aliases.
    let data_vocabulary = lycheedev_data::agent_session::schema().to_string().to_lowercase();
    let mut ranked: Vec<_> = contract::all().iter().filter_map(|definition| {
        let summary = if matches!(definition.path.as_str(), "data agent serve" | "data agent schema") {
            format!("{} {}",definition.summary,data_vocabulary).to_lowercase()
        } else { definition.summary.to_lowercase() };
        let score = terms.iter().map(|term| {
            if definition.path.split_whitespace().any(|word| word == term.as_str()) { 20 }
            else if definition.path.contains(term.as_str()) { 10 }
            else if summary.contains(term.as_str()) { 2 } else { 0 }
        }).sum::<usize>() + if definition.path == query { 1000 } else { 0 };
        (score > 0).then_some((score,definition))
    }).collect();
    ranked.sort_by(|(a,da),(b,db)| b.cmp(a).then_with(|| da.path.cmp(&db.path)));
    ranked.into_iter().take(SEARCH_LIMIT).map(|(_,d)|d).collect()
}
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().take(128).collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ac) in a.chars().take(128).enumerate() {
        let mut previous = row[0]; row[0] = i+1;
        for (j, bc) in b.iter().enumerate() {
            let old = row[j+1];
            row[j+1] = (row[j]+1).min(old+1).min(previous+usize::from(ac != *bc));
            previous = old;
        }
    }
    row[b.len()]
}
fn nearest(query: &str) -> Vec<Value> {
    let mut definitions: Vec<_> = contract::all().iter().collect();
    definitions.sort_by(|a,b| distance(query,&a.path).cmp(&distance(query,&b.path)).then_with(|| a.path.cmp(&b.path)));
    definitions.into_iter().take(SEARCH_LIMIT).map(brief).collect()
}
fn input_schema(definition: &Definition) -> Value {
    let flags: Vec<Value> = definition.flags.iter().map(|flag| json!({"name":flag.name(),
        "type":if flag.takes_value(){"string"}else{"boolean"},"arity":usize::from(flag.takes_value()),
        "repeatable":repeatable(&definition.path,flag.name()),"required":false,"contract":flag.0,"enum":enum_values(flag)})).collect();
    json!({"arguments":definition.arguments.iter().map(|name|json!({"name":name,"type":"string", "required":true,
        "variadic":definition.path == "schema"})).collect::<Vec<_>>(),"flags":flags,
        "metaFlags":[{"name":"--help","short":"-h","type":"boolean","required":false}],
        "validation":"CLI token shape and option contracts; handler-specific semantic requirements remain authoritative"})
}
fn exact_schema(path: &str) -> Result<Value> {
    let definition = contract::all().iter().find(|d|d.path == path).ok_or_else(|| {
        let mut error = ToolkitError::arguments("schema requires an exact command path");
        error.details = json!({"command":path.chars().take(256).collect::<String>(),"suggestions":nearest(path),"discovery":DISCOVERY}); error
    })?;
    let mut result = json!({"schema":"lycheedev.cli-command.v1","command":definition.path,
        "summary":definition.summary,"mutates":definition.mutates,"input":input_schema(definition)});
    if matches!(path,"data agent serve"|"data agent schema") { result["protocol"] = lycheedev_data::agent_session::schema(); }
    Ok(result)
}
fn help(inv: &Invocation) -> Result<Value> {
    if let Some(definition) = contract::all().iter().find(|d|d.path == inv.command) {
        return Ok(json!({"schema":"lycheedev.cli-help.v1","kind":"command","command":definition.path,
            "commands":[definition],"flags":definition.flags.iter().map(|f|f.0.clone()).chain(std::iter::once("--help".into())).collect::<Vec<_>>(),
            "input":input_schema(definition),"discovery":DISCOVERY,"schemaCommand":format!("lycheedev schema {}",definition.path)}));
    }
    let children = children(&inv.command);
    let total = children.len();
    let children: Vec<Value> = children.into_iter().take(CHILD_LIMIT).map(|child| {
        let path = if inv.command.is_empty(){child}else{format!("{} {child}",inv.command)};
        let definition = contract::all().iter().find(|d|d.path == path);
        json!({"command":path,"kind":if definition.is_some(){"command"}else{"group"},
            "summary":definition.map(|d|d.summary.chars().take(256).collect::<String>()),"helpCommand":format!("lycheedev {path} --help")})
    }).collect();
    Ok(json!({"schema":"lycheedev.cli-help.v1","kind":"group","command":inv.command,
        "children":children,"totalChildren":total,"truncated":total > CHILD_LIMIT,
        "flags":["--home <root>","--format text|json|jsonl","--help"],"discovery":DISCOVERY,
        "schemaCommand":"lycheedev schema <command...>","limits":{"children":CHILD_LIMIT,"searchResults":SEARCH_LIMIT}}))
}
pub fn discover(inv: &Invocation) -> Option<Result<Outcome>> {
    let result = if inv.flag_bool("--help") { help(inv) }
    else if inv.command == "schema" { exact_schema(inv.positional.as_deref().unwrap_or("")) }
    else if inv.command == "cli search" {
        let query = inv.positional.as_deref().unwrap_or("");
        if query.trim().is_empty() || query.len() > QUERY_LIMIT {
            Err(ToolkitError::arguments("task must contain 1..512 UTF-8 bytes and non-whitespace text"))
        } else { Ok(json!({"schema":"lycheedev.cli-search.v1","query":query,"limit":SEARCH_LIMIT,
            "matches":rank(query).into_iter().map(brief).collect::<Vec<_>>()})) }
    } else if inv.command == "data agent schema" { Ok(lycheedev_data::agent_session::schema()) }
    else { return None; };
    Some(result.map(Outcome::new))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(words: &[&str]) -> Vec<String> { words.iter().map(|word|(*word).to_owned()).collect() }
    #[test]
    fn clap_tree_and_every_exact_help_are_metadata_only() {
        parser().debug_assert();
        for definition in contract::all() {
            let mut words: Vec<String> = definition.path.split_whitespace().map(str::to_owned).collect();
            words.extend(["--help".into(),"--format".into(),"json".into()]);
            let mut inv = parse(&words);
            let guard = crate::install_invocation_control(&mut inv, |_| -> std::result::Result<(),String> {
                panic!("metadata installed business cancellation console")
            });
            assert!(guard.is_none());
            let result = discover(&inv.unwrap()).unwrap().unwrap().data;
            assert_eq!(result["commands"],json!([definition]));
        }
    }
    #[test]
    fn root_and_group_help_are_immediate_bounded_explicit() {
        for (words,path) in [(vec![],""),(vec!["data"],"data"),(vec!["data","agent","--help"],"data agent")] {
            let result = discover(&parse(&args(&words)).unwrap()).unwrap().unwrap().data;
            assert_eq!(result["schema"],"lycheedev.cli-help.v1");
            assert_eq!(result["kind"],"group");
            assert_eq!(result["command"],path);
            assert!(result["commands"].is_null());
            let children = result["children"].as_array().unwrap();
            assert!(!children.is_empty() && children.len() <= CHILD_LIMIT);
            assert_eq!(result["totalChildren"],children.len());
            assert_eq!(result["truncated"],false);
            for child in children {
                let command = child["command"].as_str().unwrap();
                assert_eq!(command.split_whitespace().count(),path.split_whitespace().count()+1);
                assert_eq!(child["helpCommand"],format!("lycheedev {command} --help"));
            }
        }
    }
    #[test]
    fn exact_schema_uses_authoritative_protocol_without_session() {
        let result = discover(&parse(&args(&["schema","data","agent","serve"])).unwrap()).unwrap().unwrap().data;
        assert_eq!(result["command"],"data agent serve");
        assert_eq!(result["protocol"],lycheedev_data::agent_session::schema());
        assert!(exact_schema("data agent").is_err());
        assert!(exact_schema("data sql").is_err());
        for words in [vec!["cli","search","query"],vec!["schema","version"],vec!["data","agent","schema"]] {
            let mut inv = parse(&args(&words));
            assert!(crate::install_invocation_control(&mut inv, |_| -> std::result::Result<(),String> {
                panic!("discovery installed console")
            }).is_none());
            assert!(discover(&inv.unwrap()).unwrap().is_ok());
        }
    }
    #[test]
    fn search_is_deterministic_bounded_and_never_guesses_on_zero_matches() {
        let first: Vec<_> = rank("spell effects").into_iter().map(brief).collect();
        assert!(first.len() <= SEARCH_LIMIT);
        assert!(first.iter().any(|value|value["command"] == "data agent serve"));
        assert_eq!(first,rank("spell effects").into_iter().map(brief).collect::<Vec<_>>());
        assert!(rank("zzzzzznotacapability").is_empty());
        assert!(rank("查询法术").iter().any(|d|d.path == "data agent serve"));
        assert!(discover(&parse(&args(&["cli","search"," "])).unwrap()).unwrap().is_err());
        assert!(parse(&args(&["cli","search","spell","effects"])).is_err());
    }
    #[test]
    fn serve_help_accepts_json_but_worker_does_not() {
        let inv = parse(&args(&["data","agent","serve","--help","--format","json"])).unwrap();
        assert!(inv.flag_bool("--help"));
        assert_eq!(inv.flag("--format"),Some("json"));
        assert_eq!(discover(&inv).unwrap().unwrap().data["schema"],"lycheedev.cli-help.v1");
        let error = parse(&args(&["data","agent","serve","--format","json"])).unwrap_err();
        assert_eq!(error.code,"command.invalid_arguments");
        assert_eq!(error.details["kind"],"InvalidValue");
    }
    #[test]
    fn clap_rejects_bad_arity_enum_repeats_and_unknowns_with_machine_errors() {
        for words in [vec!["versoin"],vec!["version","--bogus"],vec!["version","--home="],
            vec!["version","--format","yaml"],vec!["asset","export","--encoding","invalid"],
            vec!["version","--format","json","--format","jsonl"]] {
            let error = parse(&args(&words)).unwrap_err();
            assert_eq!(error.code,"command.invalid_arguments");
            assert_eq!(error.details["parser"],"clap");
            assert!(error.details["suggestions"].as_array().unwrap().len() <= SEARCH_LIMIT);
        }
        assert!(parse(&args(&["target","show","--help"])).is_ok());
        assert!(parse(&args(&["target","show"])).is_err());
        let inv = parse(&args(&["update","--path","a","--path","b","--help"])).unwrap();
        assert_eq!(inv.flags["__repeat:--path"],"[\"a\",\"b\"]");
    }
    #[test]
    fn all_eighteen_retired_routes_remain_oracle_only() {
        let old: Vec<Definition> = serde_json::from_str(include_str!("../../../tests/rust-parity/agent-pre-cutover.commands.json")).unwrap();
        let current: BTreeSet<_> = contract::all().iter().map(|d|d.path.as_str()).collect();
        let retired: Vec<_> = old.iter().filter(|d|!current.contains(d.path.as_str())).collect();
        assert_eq!(retired.len(),18);
        for definition in retired {
            let mut words: Vec<String> = definition.path.split_whitespace().map(str::to_owned).collect();
            words.push("--help".into());
            let error = parse(&words).unwrap_err();
            assert_eq!(error.code,"command.invalid_arguments");
            assert!(exact_schema(&definition.path).is_err());
        }
    }
}
