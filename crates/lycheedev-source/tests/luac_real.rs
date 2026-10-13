use lycheedev_source::luac::{Manifest, Runtime};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
#[test]
#[ignore = "real bundled Lua 5.1 identifier grammar differential"]
fn actual_51_goto_identifier_structure() {
    let root = PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed stage"));
    let release: Value =
        serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let compiler = Runtime::verified(
        &root,
        serde_json::from_value(release["runtime"].clone()).unwrap(),
    )
    .unwrap();
    let cases=BTreeMap::from([
        ("Local.lua".into(),b"local goto = 1; return goto\n".to_vec()),
        ("Function.lua".into(),b"function goto() end\ngoto()\n".to_vec()),
        ("Field.lua".into(),b"local obj = {}; obj.goto()\n".to_vec()),
        ("Trivia.lua".into(),b"-- goto\nlocal s = 'goto'; local t = [=[goto]=]; --[==[ goto ]==]\nfunction goto() return s, t end\n".to_vec()),
    ]);
    let staging = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/rust-source-luac-real/goto-identifiers");
    let compiled = compiler.check(&staging, &cases).unwrap();
    assert_eq!(compiled["passed"], true);
    let facts: BTreeMap<_, _> = cases
        .iter()
        .map(|(name, raw)| (name.clone(), lycheedev_source::facts::lua(raw, false)))
        .collect();
    let invalid = compiler
        .check(
            &staging,
            &BTreeMap::from([
                ("GotoStatement.lua".into(), b"goto done\n".to_vec()),
                ("Label.lua".into(), b"::done::\n".to_vec()),
            ]),
        )
        .unwrap();
    assert_eq!(invalid["passed"], false);
    assert_eq!(invalid["diagnostics"].as_array().unwrap().len(), 2);
    assert!(
        !lycheedev_source::facts::lua(b"goto done\n", false)
            .diagnostics
            .is_empty()
    );
    let report = serde_json::json!({"compiler":compiled,"facts":facts,"invalidStatements":invalid,"scope":"Lua5.1 goto identifier false-positive fix; goto jumps and labels remain rejected"});
    let first = staging.parent().unwrap().join("goto-first-report.json");
    if !first.exists() {
        fs::write(first, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    fs::write(
        staging.parent().unwrap().join("goto-current-report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(facts.values().all(|f| f.diagnostics.is_empty()));
    assert!(
        facts["Function.lua"]
            .declarations
            .iter()
            .any(|d| d.name == "goto")
    );
    assert!(
        facts["Field.lua"]
            .relationships
            .iter()
            .any(|r| r.to == "obj.goto")
    );
}
#[test]
#[ignore = "requires authenticated bundled luac dev stage; explicit real process regression"]
fn actual_51_syntax_unicode_early_error_and_no_bytecode() {
    let runtime_root = PathBuf::from(
        std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("set fixed bundled luac stage"),
    );
    let identity: Value =
        serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let manifest: Manifest = serde_json::from_value(identity["runtime"].clone()).unwrap();
    let runtime = Runtime::verified(&runtime_root, manifest).unwrap();
    let staging =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/rust-source-luac-real/空 格目录");
    fs::create_dir_all(&staging).unwrap();
    let good=BTreeMap::from([("中文 子目录/合法.lua".to_string(),"-- 中文注释\nlocal function ok(...) local x={...}; return '字符串',unpack(x) end\nreturn ok(1,2)\n".as_bytes().to_vec())]);
    let result = runtime.check(&staging, &good).unwrap();
    assert_eq!(result["passed"], true, "{result}");
    let mut large = b"local =\n".to_vec();
    large.extend(std::iter::repeat_n(b' ', 4 << 20));
    let cases = BTreeMap::from([
        ("中文 子目录/早期错误.lua".to_string(), large),
        ("Goto.lua".into(), b"goto done\n::done::\n".to_vec()),
        ("Bitops.lua".into(), b"return 1 & 2\n".to_vec()),
        ("EOF.lua".into(), b"local function broken(\n".to_vec()),
    ]);
    let bad = runtime.check(&staging, &cases).unwrap();
    assert_eq!(bad["passed"], false);
    assert_eq!(bad["diagnostics"].as_array().unwrap().len(), 4);
    assert!(
        bad["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["file"] == "中文 子目录/早期错误.lua" && d["line"] == 1)
    );
    let bytecode = BTreeMap::from([("Bytecode.lua".to_string(), b"\x1bLua".to_vec())]);
    assert!(runtime.check(&staging, &bytecode).is_err());
    assert!(!staging.join("luac.out").exists());
    fs::write(
        staging.parent().unwrap().join("real-luac-report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"valid":result,"invalid":bad})).unwrap(),
    )
    .unwrap();
    assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
}
