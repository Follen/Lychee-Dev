use std::{io::Write, path::PathBuf, sync::Arc};
use emmylua_code_analysis::{Emmyrc, EmmyrcLuaVersion};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, String>;
use lycheedev_source::native::*;


const SOURCES: &[(&str,&str)] = &[("profile.lua",include_str!("../../../../tests/fixtures/source-native/wow/profile.lua"))];
const NATIVE_SOURCES: &[(&str,&str)] = &[
    ("generic-defs.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/generic-defs.lua")),
    ("generic-calls.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/generic-calls.lua")),
    ("generic-replace.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/generic-replace.lua")),
    ("coverage-unknown-argument.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/coverage-unknown-argument.lua")),
    ("object-versions.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/object-versions.lua")),
    ("object-facts.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/object-facts.lua")),
    ("audit-rebind.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/audit-rebind.lua")),
    ("audit-colon.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/audit-colon.lua")),
    ("audit-field-closure.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/audit-field-closure.lua")),
    ("audit-expression.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/audit-expression.lua")),
    ("audit-record-union.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/audit-record-union.lua")),
    ("invocation-captures.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/invocation-captures.lua")),
    ("invocation-reassign.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/invocation-reassign.lua")),
    ("invocation-short-circuit.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/invocation-short-circuit.lua")),
    ("invocation-selector.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/invocation-selector.lua")),
    ("invocation-escape.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/invocation-escape.lua")),
    ("invocation-escape-closure.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/invocation-escape-closure.lua")),
    ("invocation-escape-rebind.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/invocation-escape-rebind.lua")),
    ("invocation-call-capture.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/invocation-call-capture.lua")),
    ("invocation-return-slots.lua", include_str!("../../../../tests/fixtures/source-native/wow/native/invocation-return-slots.lua")),
];
fn source(name: &str) -> &'static str { SOURCES.iter().chain(NATIVE_SOURCES.iter()).find(|(n,_)|*n==name).expect("fixture name").1 }

// The fixture harness supplies immutable host policy; investigated source never chooses intrinsics.
fn fixture_inputs(names:&[&str],control:&lycheedev_core::control::OperationControl)->Result<Arc<SourceInputs>> {
    let mut config=Emmyrc::default();config.runtime.version=EmmyrcLuaVersion::Lua51;
    {
        config.diagnostics.disable=emmylua_code_analysis::DiagnosticCode::all().into_iter().filter(|code|!matches!(code,emmylua_code_analysis::DiagnosticCode::ParamTypeMismatch|emmylua_code_analysis::DiagnosticCode::ReturnTypeMismatch)).collect();
        config.diagnostics.enables=vec![emmylua_code_analysis::DiagnosticCode::ParamTypeMismatch,emmylua_code_analysis::DiagnosticCode::ReturnTypeMismatch];
    }
    let policy=b"Lychee Dev fixed fixture host: explicit Mixin, CreateFromMixins and setmetatable shallow-copy profile v1";
    let mut intrinsics=Vec::new();
    if names.contains(&"profile.lua") {
        let text=source("profile.lua");
        for name in ["Mixin","CreateFromMixins","setmetatable"] {
            let marker=format!("function {name}");let start=text.find(&marker).ok_or("fixture host declaration")?+"function ".len();
            intrinsics.push(IntrinsicDeclaration{file:"profile.lua".into(),source_sha256:hash(text.as_bytes()),start:start as u32,end:(start+name.len())as u32,name:name.into(),rule:PROFILE.into(),host_material:"compiled-fixture-host-v1".into()});
        }
    }
    let host=TrustedHostEnvironment::from_host_material("closed-fixture-initial-globals-v1".into(),true,vec![HostMaterialInput{name:"compiled-fixture-host-v1".into(),bytes:policy.to_vec(),expected_sha256:hash(policy)}],intrinsics,control).map_err(|e|e.to_string())?;
    let luac:Value=serde_json::from_str(include_str!("../../../../release/tools/luac.json")).map_err(|e|e.to_string())?;
    let runtime=&luac["runtime"];
    let inputs=SourceInputSpec{
        files:names.iter().map(|name|{let text=source(name);SourceFileInput{path:(*name).into(),original_utf8:text.into(),content_sha256:hash(text.as_bytes()),analysis_utf8:text.into(),map:SourceMap::Identity}}).collect(),
        workspace_root:PathBuf::from("C:/lycheedev-wow-probe"),source_identity:json!({"kind":"compiled-fixture-inputs","schema":"lycheedev.native-fixture-admission.v1"}),phase:"unknown".into(),ordered_occurrences:names.iter().map(|name|(*name).into()).collect(),config,host,
        luac:LuacIdentity{version:runtime["version"].as_str().ok_or("luac version")?.into(),platform:runtime["platform"].as_str().ok_or("luac platform")?.into(),executable_sha256:runtime["sha256"].as_str().ok_or("luac hash")?.into(),executable_bytes:runtime["bytes"].as_u64().ok_or("luac bytes")?},limits:SourceLimits::default()};
    SourceInputs::admit(inputs,control).map_err(|e|e.to_string())
}
fn fixture_control()->lycheedev_core::control::OperationControl {
    lycheedev_core::control::OperationControl::uncancelable(std::time::Instant::now()+std::time::Duration::from_millis(SourceLimits::default().total_milliseconds))
}
