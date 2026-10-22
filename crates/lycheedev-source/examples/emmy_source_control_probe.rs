// Parse fixed Lua inputs; never execute investigated source.
include!("support/owned_query.rs");
use emmylua_code_analysis::{Emmyrc, EmmyrcLuaVersion, LuaType};
use lycheedev_core::control::OperationControl;
use lycheedev_source::native::*;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, String>;
const FILES: &[(&str, &str)] = &[
    (
        "control-numeric.lua",
        include_str!("../../../tests/fixtures/source-native/wow/native/control-numeric.lua"),
    ),
    (
        "control-repeat.lua",
        include_str!("../../../tests/fixtures/source-native/wow/native/control-repeat.lua"),
    ),
    (
        "control-break.lua",
        include_str!("../../../tests/fixtures/source-native/wow/native/control-break.lua"),
    ),
    (
        "control-return.lua",
        include_str!("../../../tests/fixtures/source-native/wow/native/control-return.lua"),
    ),
    (
        "control-while.lua",
        include_str!("../../../tests/fixtures/source-native/wow/native/control-while.lua"),
    ),
    (
        "control-generic.lua",
        include_str!("../../../tests/fixtures/source-native/wow/native/control-generic.lua"),
    ),
    (
        "control-budget.lua",
        include_str!("../../../tests/fixtures/source-native/wow/native/control-budget.lua"),
    ),
    (
        "control-zero-step.lua",
        include_str!("../../../tests/fixtures/source-native/wow/native/control-zero-step.lua"),
    ),
    (
        "control-function-budget.lua",
        include_str!("../../../tests/fixtures/source-native/wow/native/control-function-budget.lua"),
    ),
    (
        "control-function-generic.lua",
        include_str!("../../../tests/fixtures/source-native/wow/native/control-function-generic.lua"),
    ),
];
fn prepare_case(file: &str, source: &str, c: &OperationControl) -> Result<PreparedSnapshot> {
    let mut config = Emmyrc::default();
    config.runtime.version = EmmyrcLuaVersion::Lua51;
    let spec = SourceInputSpec {
        files: vec![SourceFileInput {
            path: file.into(),
            original_utf8: source.into(),
            content_sha256: hash(source.as_bytes()),
            analysis_utf8: source.into(),
            map: SourceMap::Identity,
        }],
        workspace_root: PathBuf::from("C:/lycheedev-source-control-probe"),
        source_identity: json!({"kind":"fixed-common-control-source"}),
        phase: "after_load".into(),
        ordered_occurrences: vec![file.into()],
        config,
        host: TrustedHostEnvironment::from_host_material(
            "closed-common-control-probe".into(),
            true,
            vec![],
            vec![],
            c,
        )
        .map_err(|e| e.to_string())?,
        luac: LuacIdentity {
            version: "5.1.5".into(),
            platform: "windows-amd64".into(),
            executable_sha256: "64cd0d82734a743ccbffe3a2d28727ce8e48c8cf1452dd69066f2c56643425f7"
                .into(),
            executable_bytes: 487424,
        },
        limits: SourceLimits::default(),
    };
    prepare(SourceInputs::admit(spec, c).map_err(|e| e.to_string())?, c).map_err(|e| e.to_string())
}
struct Output {
    bytes: usize,
}
impl Output {
    fn emit(&mut self, mut frame: Value) -> Result<()> {
        frame["schema"] = json!("lycheedev.source-control-probe.v1");
        let line = serde_json::to_string(&frame).map_err(|e| e.to_string())?;
        self.bytes += line.len() + 1;
        if self.bytes > 2 * 1024 * 1024 {
            return Err("control output byte budget".into());
        }
        println!("{line}");
        Ok(())
    }
}
fn run(out: &mut Output) -> Result<()> {
    out.emit(json!({"event":"build_identity","engineCommit":ENGINE_COMMIT,"engineArchiveSHA256":ENGINE_ARCHIVE_SHA256}))?;
    for (file, source) in FILES {
        out.emit(json!({"event":"input_identity","file":file,"bytes":source.len(),"sha256":hash(source.as_bytes())}))?;
        let c = OperationControl::uncancelable(Instant::now() + Duration::from_secs(30));
        let prepared = prepare_case(file, source, &c)?;
        let graph = prepared
            .build(
                file,
                Some("common-control-fixture-v1"),
                &c,
            )
            .map_err(|e| e.to_string())?;
        if *file == "control-numeric.lua" {
            let syntax = prepared.source_queries(&c).map_err(|e| e.to_string())?;
            let facts = graph
                .native_invocations
                .as_ref()
                .ok_or("native control facts")?;
            for call in &graph.calls {
                let Some(argument) = call.arguments.first().map(|id| &graph.uses[*id]) else {
                    continue;
                };
                if argument
                    .site
                    .text(prepared.inputs())
                    .map_err(|e| e.to_string())?
                    != "i"
                {
                    continue;
                }
                let occurrence = *facts
                    .control
                    .context_occurrences
                    .get(&call.context)
                    .ok_or("call occurrence")?;
                let query = syntax
                    .with_execution(&graph, QuerySelector::Occurrence { occurrence }, &c)
                    .map_err(|e| e.to_string())?;
                let definition = query
                    .definitions(&argument.site)
                    .map_err(|e| e.to_string())?;
                let selected_freeze=prepared.freeze(&graph,NativeViewSelection::Occurrence(occurrence),&c).map(|_|json!({"accepted":true})).unwrap_or_else(|e|json!({"accepted":false,"code":e.code,"exit":e.exit_code,"error":e.to_string()}));
                let binding = prepared.binding_site(&argument.site)?;
                let native = prepared
                    .query(
                        &graph,
                        NativeViewSelection::Occurrence(occurrence),
                        &c,
                        |analysis, session, control| {
                            control.check()?;
                            let mut check = || control.check();
                            let mut model = ProbeOwnedQuery::new_with_contribution_order(
                                analysis,
                                &session,
                                binding.file_id,
                                &mut check,
                                prepared.contribution_order(),
                            )
                            .with_workspace_facts(prepared.workspace_facts());
                            let ty = model.type_of_expr(binding.syntax).map_err(probe_failure)?;
                            Ok(match ty.structural_type() {
                                LuaType::FloatConst(number) => {
                                    json!({"state":"number_constant","value":number})
                                }
                                other => json!({"state":"other","type":format!("{other:?}")}),
                            })
                        },
                    )
                    .unwrap_or_else(|e| json!({"state":"error","error":e.to_string()}));
                out.emit(json!({"event":"occurrence_query","file":file,"call":call.id,"occurrence":occurrence,"argumentFact":argument.id,"definition":definition,"freezeAccepted":selected_freeze["accepted"],"freeze":selected_freeze,"native":native}))?;
            }
        }
        let freeze=prepared.freeze(&graph,NativeViewSelection::Load(0),&c).map(|_|json!({"accepted":true})).unwrap_or_else(|e|json!({"accepted":false,"code":e.code,"exit":e.exit_code,"error":e.to_string()}));
        out.emit(json!({"event":"scenario","file":file,"graph":graph,"inputIdentity":prepared.inputs().identity(),"freeze":freeze,"fullSourceAcceptance":false}))?;
        c.tracked_cancel_handle().unwrap().cancel();
        let cancelled = prepared
            .freeze(&graph, NativeViewSelection::Load(0), &c)
            .err()
            .map(|e| json!({"code":e.code,"exit":e.exit_code}));
        out.emit(json!({"event":"cancel","file":file,"result":cancelled}))?;
    }
    Ok(())
}
fn main() {
    let mut out = Output { bytes: 0 };
    let result = run(&mut out);
    let complete = result.is_ok();
    let error = result.err();
    if let Err(e) = out
        .emit(json!({"event":"end","complete":complete,"error":error,"fullSourceAcceptance":false}))
    {
        eprintln!("{e}");
        std::process::exit(1)
    }
    if !complete {
        std::process::exit(1)
    }
}
