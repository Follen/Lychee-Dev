//! Isolated direct-library feasibility probe; never used by the product CLI.
//! The caller owns the process/job deadline and memory cap. See tests/rust-source-engine/README.md.
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use emmylua_code_analysis::{
    EmmyLuaAnalysis, Emmyrc, EmmyrcLuaVersion, FileId, LuaType, SemanticId, SemanticModel,
    WorkspaceFolder, file_path_to_uri, humanize_semantic_type, load_resource_from_include_dir,
};
use emmylua_parser::{
    LuaAssignStat, LuaAstNode, LuaCallExpr, LuaExpr, LuaFuncStat, LuaIndexExpr, LuaLanguageLevel,
    LuaParseErrorKind, LuaParser, LuaSyntaxKind, ParserConfig,
};
use rowan::{TextRange, TextSize};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

const ENGINE: &str = "08b0873962e83bb8307e9a81052e3f41578fbc78";
const SOURCE_COMMIT: &str = "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b";
const SOURCE_DIGEST: &str = "88d68f2c4bdb352245cbcb53cd41ec906b294e2ea599ee6374e00be169be7928";
const ENV_DIGEST: &str = "d5c3071e9c696cc19b02f609cae3fe8be19089ee5e434537bdf81f3f81b7c103";
const VIRTUAL_ROOT: &str = "C:/lycheedev-emmy-probe";
const CALLBACK: &str = "Interface/AddOns/Blizzard_SharedXMLBase/CallbackRegistry.lua";
const OUTPUT_LIMIT: usize = 1 << 20;
const FILE_LIMIT: u64 = 8 << 20;
const DOCUMENT_LIMIT: u64 = 128 << 20;

const SCOPE: &str = include_str!("../../../tests/rust-source-engine/golden/scope.lua");
const GLOBAL_A: &str = include_str!("../../../tests/rust-source-engine/golden/global-a.lua");
const GLOBAL_B: &str = include_str!("../../../tests/rust-source-engine/golden/global-b.lua");
const ALIASES: &str = include_str!("../../../tests/rust-source-engine/golden/aliases.lua");
const ALIAS_FUNCTION_WRITE: &str = include_str!("../../../tests/rust-source-engine/golden/alias-function-write.lua");
const ALIAS_DEFERRED_CALL: &str = include_str!("../../../tests/rust-source-engine/golden/alias-deferred-call.lua");
const ALIAS_COPIED_VALUE: &str = include_str!("../../../tests/rust-source-engine/golden/alias-copied-value.lua");
const ALIAS_COPY_CHAIN: &str = include_str!("../../../tests/rust-source-engine/golden/alias-copy-chain.lua");
const ALIAS_LOOP_CALL: &str = include_str!("../../../tests/rust-source-engine/golden/alias-loop-call.lua");
const ALIAS_NAME_NIL_SLOT: &str = include_str!("../../../tests/rust-source-engine/golden/alias-name-nil-slot.lua");
const ALIAS_CLOSURE_NIL_SLOT: &str = include_str!("../../../tests/rust-source-engine/golden/alias-closure-nil-slot.lua");
const ALIAS_NAME_FIRST_SLOT: &str = include_str!("../../../tests/rust-source-engine/golden/alias-name-first-slot.lua");
const ALIAS_NIL_SLOT_COPY: &str = include_str!("../../../tests/rust-source-engine/golden/alias-nil-slot-copy.lua");
const MEMBER: &str = include_str!("../../../tests/rust-source-engine/golden/member.lua");
const TYPES: &str = include_str!("../../../tests/rust-source-engine/golden/types.lua");
const DIAGNOSTIC: &str = include_str!("../../../tests/rust-source-engine/golden/diagnostic.lua");
const UNICODE: &str = include_str!("../../../tests/rust-source-engine/golden/unicode.lua");
const WRITES: &str = include_str!("../../../tests/rust-source-engine/golden/writes-redeclaration.lua");
const BOM_UNICODE: &[u8] = include_bytes!("../../../tests/rust-source-engine/golden/bom-unicode.lua");

type ProbeResult<T> = Result<T, String>;

struct Output {
    started: Instant,
    bytes: usize,
    failed: usize,
    checked: usize,
    raw_engine_failed: usize,
    raw_engine_checked: usize,
    hold_ms: u64,
    cycle: Option<u32>,
}

impl Output {
    fn emit(&mut self, mut value: Value) -> ProbeResult<()> {
        let object = value.as_object_mut().ok_or("event must be an object")?;
        object.insert("schema".into(), json!("lycheedev.emmy-library-probe.v1"));
        object.insert("pid".into(), json!(std::process::id()));
        object.insert("elapsedMs".into(), json!(self.started.elapsed().as_millis()));
        object.insert("cycle".into(), json!(self.cycle));
        let mut bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        if self.bytes + bytes.len() > OUTPUT_LIMIT {
            return Err("stdout byte budget exceeded; no complete terminal record".into());
        }
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&bytes).map_err(|e| e.to_string())?;
        stdout.flush().map_err(|e| e.to_string())?;
        self.bytes += bytes.len();
        Ok(())
    }

    fn milestone(&mut self, name: &str) -> ProbeResult<()> {
        self.emit(json!({"event":"resource_milestone","phase":name,
            "measurementOwner":"external process observer", "holdMs":self.hold_ms}))?;
        // Optional bounded observation window, excluded from reported operation timings.
        if self.hold_ms != 0 {
            std::thread::sleep(Duration::from_millis(self.hold_ms));
        }
        Ok(())
    }

    fn check(&mut self, name: &str, expected: Value, actual: Value) -> ProbeResult<()> {
        self.check_in_domain("business_baseline", name, expected, actual)
    }

    fn check_engine(&mut self, name: &str, expected: Value, actual: Value) -> ProbeResult<()> {
        self.check_in_domain("raw_engine_syntax_capability", name, expected, actual)
    }

    fn check_in_domain(&mut self, domain: &str, name: &str, expected: Value, actual: Value) -> ProbeResult<()> {
        let passed = expected == actual;
        self.checked += 1;
        if !passed {
            self.failed += 1;
        }
        if domain == "raw_engine_syntax_capability" {
            self.raw_engine_checked += 1;
            self.raw_engine_failed += usize::from(!passed);
        }
        self.emit(json!({"event":"assertion","case":name,"passed":passed,
            "domain":domain,"expected":expected,"actual":actual}))
    }
}

struct FileMapping {
    path: PathBuf,
    original_bytes: usize,
    original_sha256: String,
    analyzed_sha256: String,
    prefix_bytes: u32,
    // Only changed files retain a second text allocation. Unchanged files use the VFS text.
    original_text: Option<String>,
}

struct ProbeSnapshot {
    analysis: EmmyLuaAnalysis,
    mappings: BTreeMap<FileId, FileMapping>,
}

impl std::ops::Deref for ProbeSnapshot {
    type Target = EmmyLuaAnalysis;
    fn deref(&self) -> &Self::Target { &self.analysis }
}

impl ProbeSnapshot {
    fn mapping(&self, file: FileId) -> ProbeResult<&FileMapping> {
        self.mappings.get(&file).ok_or_else(||format!("missing input map for file {}",file.id))
    }

    fn original_text(&self, file: FileId) -> ProbeResult<&str> {
        match &self.mapping(file)?.original_text {
            Some(text) => Ok(text),
            None => self.db.get_file_text(file).ok_or_else(||"mapped file missing from VFS".into()),
        }
    }

    fn original_range(&self, file: FileId, analyzed: TextRange) -> ProbeResult<TextRange> {
        let mapping=self.mapping(file)?;
        let text=self.db.get_file_text(file).ok_or("mapped file missing analyzed text")?;
        let start=u32::from(analyzed.start());
        let end=u32::from(analyzed.end());
        let expected=text.get(start as usize..end as usize).ok_or("invalid analyzed UTF8 byte range")?;
        let original_start=start.checked_add(mapping.prefix_bytes).ok_or("mapped start overflow")?;
        let original_end=end.checked_add(mapping.prefix_bytes).ok_or("mapped end overflow")?;
        let original=self.original_text(file)?;
        let actual=original.get(original_start as usize..original_end as usize).ok_or("invalid original UTF8 byte range")?;
        if actual!=expected {return Err("prefix map failed exact original-byte round trip".into());}
        Ok(TextRange::new(original_start.into(),original_end.into()))
    }

    fn analysis_offset(&self, file: FileId, original: usize) -> ProbeResult<TextSize> {
        let mapping=self.mapping(file)?;
        let text=self.original_text(file)?;
        if original>text.len() || !text.is_char_boundary(original) {return Err("invalid original UTF8 position".into());}
        let offset=original.checked_sub(mapping.prefix_bytes as usize).ok_or("position is inside removed BOM prefix")?;
        Ok(u32::try_from(offset).map_err(|_|"analysis position overflow")?.into())
    }

    fn original_locations(&self, analyzed: &[(u32,u32,u32)]) -> ProbeResult<Vec<(u32,u32,u32)>> {
        analyzed.iter().map(|(file,start,end)| {
            let mapped=self.original_range(FileId::new(*file),TextRange::new((*start).into(),(*end).into()))?;
            Ok((*file,u32::from(mapped.start()),u32::from(mapped.end())))
        }).collect()
    }

    fn range_value(&self, file: FileId, analyzed: TextRange) -> ProbeResult<Value> {
        Ok(range_value(self.original_range(file,analyzed)?))
    }
}

fn prepare_mapped_text(path: PathBuf, original: String) -> (String, FileMapping) {
    let original_bytes=original.len();
    let original_sha256=hash(original.as_bytes());
    if let Some(analyzed)=original.strip_prefix('\u{feff}') {
        let analyzed_sha256=hash(analyzed.as_bytes());
        let analyzed=analyzed.to_string();
        (analyzed,FileMapping{path,original_bytes,original_sha256,analyzed_sha256,prefix_bytes:3,
            original_text:Some(original)})
    } else {
        let analyzed_sha256=original_sha256.clone();
        (original,FileMapping{path,original_bytes,original_sha256,analyzed_sha256,prefix_bytes:0,original_text:None})
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn short(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn source_path(relative: &str) -> PathBuf {
    PathBuf::from(VIRTUAL_ROOT).join("source").join(relative)
}

fn range(start: usize, len: usize) -> TextRange {
    TextRange::new((start as u32).into(), ((start + len) as u32).into())
}

fn range_value(range: TextRange) -> Value {
    json!([u32::from(range.start()), u32::from(range.end())])
}

// Expected positions come from fixed input text, never from the queried AST.
fn marker(text: &str, context: &str, word: &str) -> usize {
    text.find(context).expect("fixed fixture marker") + context.find(word).expect("marker word")
}

fn file_id(analysis: &EmmyLuaAnalysis, relative: &str) -> ProbeResult<FileId> {
    let uri = file_path_to_uri(&source_path(relative)).ok_or("invalid virtual URI")?;
    analysis.get_file_id(&uri).ok_or_else(|| format!("missing VFS file: {relative}"))
}

fn id_value(snapshot: &ProbeSnapshot, id: Option<&SemanticId>) -> ProbeResult<Value> {
    Ok(match id {
        Some(SemanticId::Decl(key)) => json!({"kind":"decl","file":key.file_id.id,
            "bytes":snapshot.range_value(key.file_id,key.name_range)?}),
        Some(SemanticId::Member(key)) => json!({"kind":"member","file":key.file_id.id,
            "bytes":snapshot.range_value(key.file_id,key.key_range)?}),
        Some(SemanticId::Name(name)) => json!({"kind":"unresolved_global","name":name.as_str()}),
        Some(_) => json!({"kind":"other_semantic_identity"}),
        None => Value::Null,
    })
}

fn sorted_ranges(ranges: Vec<(FileId, TextRange)>) -> Vec<(u32, u32, u32)> {
    let mut result: Vec<_> = ranges.into_iter()
        .map(|(file, r)| (file.id, u32::from(r.start()), u32::from(r.end())))
        .collect();
    result.sort_unstable();
    result.dedup();
    result
}

fn expected_ranges(file: FileId, positions: &[usize], len: usize) -> Value {
    json!(sorted_ranges(positions.iter().map(|p| (file, range(*p, len))).collect()))
}

struct ReferencePartition {
    // Sorted raw multiset preserves duplicate index entries for evidence.
    raw_index: Vec<(u32,u32,u32)>,
    declared: (u32,u32,u32),
    raw_declarations: Vec<(u32,u32,u32)>,
    references: Vec<(u32,u32,u32)>,
}

fn reference_partition(snapshot: &ProbeSnapshot, target: &SemanticId) -> ProbeResult<ReferencePartition> {
    let (raw,declared)=match target {
        SemanticId::Decl(key) => {
            let model=snapshot.semantic_model(key.file_id);
            let decl=model.file_facts().and_then(|facts|facts.decl_by_id(target)).ok_or("reference target has no exact declaration fact")?;
            if decl.file_id!=key.file_id || decl.name_range!=key.name_range {return Err("declaration identity/range mismatch".into());}
            (snapshot.db.decl_reference_ranges(target),(key.file_id,key.name_range))
        }
        SemanticId::Member(key) => {
            let model=snapshot.semantic_model(key.file_id);
            let member=model.file_facts().and_then(|facts|facts.member_by_id(target)).ok_or("reference target has no exact member fact")?;
            if member.id.member_key_range()!=Some(key.key_range) {return Err("member identity/range mismatch".into());}
            (snapshot.db.member_reference_ranges(target),(key.file_id,key.key_range))
        }
        _ => return Err("reference projection supports concrete declaration/member identities only".into()),
    };
    let declared=(declared.0.id,u32::from(declared.1.start()),u32::from(declared.1.end()));
    let mut raw_index:Vec<_>=raw.into_iter().map(|(file,r)|(file.id,u32::from(r.start()),u32::from(r.end()))).collect();
    raw_index.sort_unstable();
    let mut unique=raw_index.clone();
    unique.dedup();
    let (raw_declarations,references)=unique.into_iter().partition(|site|*site==declared);
    Ok(ReferencePartition{raw_index,declared,raw_declarations,references})
}

impl ReferencePartition {
    fn partition_is_exact(&self) -> bool {
        let mut unique=self.raw_index.clone(); unique.dedup();
        let mut union=self.raw_declarations.clone(); union.extend_from_slice(&self.references); union.sort_unstable();
        union==unique && self.references.iter().all(|site|!self.raw_declarations.contains(site))
    }

    fn evidence(&self, snapshot: &ProbeSnapshot, sample_limit: usize) -> ProbeResult<Value> {
        let original_raw=snapshot.original_locations(&self.raw_index)?;
        let original_refs=snapshot.original_locations(&self.references)?;
        let original_declarations=snapshot.original_locations(&self.raw_declarations)?;
        Ok(json!({"rawIndexCoordinate":"analysis_utf8_bytes","rawIndexCount":self.raw_index.len(),
            "rawIndexSHA256":hash(&serde_json::to_vec(&self.raw_index).map_err(|e|e.to_string())?),
            "rawIndexSample":self.raw_index.iter().take(sample_limit).collect::<Vec<_>>(),
            "rawIndexSampleTruncated":self.raw_index.len()>sample_limit,
            "rawIndexOriginalSample":original_raw.iter().take(sample_limit).collect::<Vec<_>>(),
            "projectedCoordinate":"original_utf8_bytes",
            "verifiedTargetDeclaration":snapshot.original_locations(&[self.declared])?,
            "rawDeclarationCount":self.raw_declarations.len(),"rawDeclarations":original_declarations,
            "referenceCount":original_refs.len(),"referencesSHA256":hash(&serde_json::to_vec(&original_refs).map_err(|e|e.to_string())?),
            "referenceSample":original_refs.iter().take(sample_limit).collect::<Vec<_>>(),
            "referenceSampleTruncated":original_refs.len()>sample_limit,"partitionExact":self.partition_is_exact(),
            "roleRule":"only the exact target identity's verified declaration range is excluded; other writes and redefinitions remain occurrences"}))
    }
}

fn check_references(out: &mut Output, snapshot: &ProbeSnapshot, name: &str, target: &SemanticId, expected: Value) -> ProbeResult<()> {
    let partition=reference_partition(snapshot,target)?;
    out.check(name,expected,json!(snapshot.original_locations(&partition.references)?))?;
    out.check(&format!("{name}_partition"),json!(true),json!(partition.partition_is_exact()))?;
    out.emit(json!({"event":"reference_partition","case":name,"evidence":partition.evidence(snapshot,32)?}))
}

fn scalar_position_to_byte(text: &str, position: &Value) -> ProbeResult<usize> {
    let line=position["line"].as_u64().ok_or("diagnostic line missing")? as usize;
    let column=position["character"].as_u64().ok_or("diagnostic scalar column missing")? as usize;
    let mut start=0usize;
    let mut found=None;
    for (index,part) in text.split('\n').enumerate() {
        if index==line {found=Some(part);break;}
        start+=part.len()+1;
    }
    let part=found.ok_or("diagnostic line outside analyzed text")?;
    let offset=part.char_indices().map(|(offset,_)|offset).chain(std::iter::once(part.len()))
        .nth(column).ok_or("diagnostic scalar column outside line")?;
    Ok(start+offset)
}

fn byte_to_scalar_position(text: &str, offset: usize) -> ProbeResult<Value> {
    let prefix=text.get(..offset).ok_or("diagnostic mapped byte position invalid")?;
    let line=prefix.bytes().filter(|byte|*byte==b'\n').count();
    let start=prefix.rfind('\n').map_or(0,|at|at+1);
    Ok(json!({"line":line,"character":prefix[start..].chars().count()}))
}

// The upstream diagnostic API reports Unicode-scalar columns, despite using LSP types.
// Normalize every diagnostic range through the same checked original-byte mapping.
fn mapped_diagnostics(snapshot: &ProbeSnapshot, file: FileId, raw: Value) -> ProbeResult<Vec<Value>> {
    let analyzed=snapshot.db.get_file_text(file).ok_or("diagnostic file not loaded")?;
    let original=snapshot.original_text(file)?;
    raw.as_array().ok_or("diagnostic output not array")?.iter().map(|diagnostic| {
        let start=scalar_position_to_byte(analyzed,&diagnostic["range"]["start"])?;
        let end=scalar_position_to_byte(analyzed,&diagnostic["range"]["end"])?;
        if start>end {return Err("reversed diagnostic range".into());}
        let mapped=snapshot.original_range(file,range(start,end-start))?;
        let mut result=diagnostic.clone();
        let object=result.as_object_mut().ok_or("diagnostic not object")?;
        object.insert("analyzedRange".into(),diagnostic["range"].clone());
        object.insert("analyzedRangeCoordinate".into(),json!("unicode_scalar"));
        object.insert("range".into(),json!({
            "start":byte_to_scalar_position(original,u32::from(mapped.start()) as usize)?,
            "end":byte_to_scalar_position(original,u32::from(mapped.end()) as usize)?}));
        object.insert("rangeCoordinate".into(),json!("original_unicode_scalar"));
        object.insert("originalByteRange".into(),range_value(mapped));
        Ok(result)
    }).collect()
}

fn build_analysis(
    out: &mut Output,
    sources: Vec<(String, String)>,
    environment: Option<String>,
) -> ProbeResult<ProbeSnapshot> {
    let mut config = Emmyrc::default();
    config.runtime.version = EmmyrcLuaVersion::Lua51;
    // No discovery, file-config loader, executable configuration, or std disk cache.
    let config_value = serde_json::to_value(&config).map_err(|e| e.to_string())?;
    let config_bytes = serde_json::to_vec(&config_value).map_err(|e| e.to_string())?;
    out.emit(json!({"event":"configuration","declaredEngineRevision":ENGINE,
        "config":config_value,"configSHA256":hash(&config_bytes),"executableConfig":false,
        "libraryThreads":1,"syntaxAuthority":"pinned luac 5.1.5 remains separate; not run here"}))?;

    let mut std_files = load_resource_from_include_dir();
    for file in &mut std_files {
        file.path = file.path.replace('\\', "/");
    }
    const JIT: &[&str] = &["jit.lua", "jit/profile.lua", "jit/util.lua", "string/buffer.lua",
        "table/clear.lua", "table/new.lua", "ffi.lua"];
    std_files.retain(|file| file.path.starts_with("std/") && file.path.ends_with(".lua")
        && !JIT.iter().any(|path| file.path.strip_prefix("std/") == Some(*path)));
    std_files.sort_by(|a, b| a.path.cmp(&b.path));
    if std_files.len() != 13 {
        return Err("fixed upstream embedded standard-library file set differs".into());
    }
    let std_identity: Vec<_> = std_files.iter().map(|file| json!({"path":file.path,
        "bytes":file.content.len(),"sha256":hash(file.content.as_bytes())})).collect();
    let std_digest = hash(&serde_json::to_vec(&std_identity).map_err(|e| e.to_string())?);
    out.emit(json!({"event":"embedded_std","files":std_identity,"identitySHA256":std_digest,
        "virtualRoot":format!("{VIRTUAL_ROOT}/stdlib"),"diskResourcesRead":false}))?;

    let source_count = sources.len();
    let environment_count = usize::from(environment.is_some());
    let std_count = std_files.len();
    let mut files: Vec<_> = sources.into_iter()
        .map(|(path, text)| (source_path(&path), Some(text))).collect();
    if let Some(text) = environment {
        files.push((PathBuf::from(VIRTUAL_ROOT).join("environment/wow-api.lua"), Some(text)));
    }
    for file in std_files {
        files.push((PathBuf::from(VIRTUAL_ROOT).join("stdlib").join(file.path), Some(file.content)));
    }
    let mut pending_maps=Vec::with_capacity(files.len());
    let files:Vec<_>=files.into_iter().map(|(path,text)| {
        let (text,mapping)=prepare_mapped_text(path.clone(),text.expect("explicit set input"));
        pending_maps.push(mapping);
        (path,Some(text))
    }).collect();
    out.milestone("before_load")?;
    let start = Instant::now();
    let mut analysis = EmmyLuaAnalysis::new();
    analysis.update_config(Arc::new(config));
    analysis.add_main_workspace(source_path(""));
    analysis.add_library_workspace(&WorkspaceFolder::new(
        PathBuf::from(VIRTUAL_ROOT).join("environment"), true));
    analysis.add_library_workspace(&WorkspaceFolder::new(
        PathBuf::from(VIRTUAL_ROOT).join("stdlib"), true));
    let inserted = analysis.update_files_by_path(files);
    let elapsed = start.elapsed().as_micros();
    let mut mappings=BTreeMap::new();
    let mut mapping_identity=Vec::with_capacity(pending_maps.len());
    let mut changed=Vec::new();
    for mapping in pending_maps {
        let uri=file_path_to_uri(&mapping.path).ok_or("invalid mapped virtual URI")?;
        let id=analysis.get_file_id(&uri).ok_or("mapped file absent after load")?;
        let record=json!({"file":id.id,"path":mapping.path,"originalBytes":mapping.original_bytes,
            "analyzedBytes":mapping.original_bytes-mapping.prefix_bytes as usize,
            "originalSHA256":mapping.original_sha256,"analyzedSHA256":mapping.analyzed_sha256,
            "prefixMap":{"removedOriginalPrefixBytes":mapping.prefix_bytes,"analyzedToOriginalByteDelta":mapping.prefix_bytes}});
        mapping_identity.push(record.clone());
        if mapping.prefix_bytes!=0 {changed.push(record);}
        mappings.insert(id,mapping);
    }
    out.emit(json!({"event":"source_maps","mapping":"utf8-bom-prefix-v1","mappedFiles":mappings.len(),
        "identitySHA256":hash(&serde_json::to_vec(&mapping_identity).map_err(|e|e.to_string())?),
        "changedFiles":changed,"unchangedFiles":mappings.len()-changed.len(),
        "unchangedRule":"prefix delta 0; analyzed SHA256 equals original manifest, environment or embedded resource SHA256",
        "resultCoordinate":"original_utf8_bytes"}))?;
    out.check("all_explicit_files_inserted", json!(source_count + environment_count + std_count),
        json!(inserted.len()))?;
    out.emit(json!({"event":"loaded","loadMicros":elapsed,"sourceLuaFiles":source_count,
        "environmentFiles":environment_count,"stdlibFiles":std_count,
        "vfsFiles":analysis.db.file_ids().len(),"parseErrorFilesExcluded":0}))?;
    out.milestone("after_load")?;
    Ok(ProbeSnapshot{analysis,mappings})
}

fn callee_identity(model: &SemanticModel<'_>, expr: &LuaExpr) -> Option<SemanticId> {
    match expr {
        LuaExpr::NameExpr(name) => model.resolve_name(name.get_position()),
        LuaExpr::IndexExpr(index) => model.resolve_member(index).and_then(|m| m.member_id),
        LuaExpr::ParenExpr(paren) => paren.get_expr().and_then(|inner| callee_identity(model, &inner)),
        _ => None,
    }
}

// Deliberately narrow evidence: same-file, straight-line top-level initializer copies.
// Function/control-flow execution order and general points-to analysis remain unknown.
fn initializer_origin_before(
    model: &SemanticModel<'_>, initial: &SemanticId, at: TextSize,
) -> (String, Option<SemanticId>) {
    let Some(chunk)=model.chunk() else {return ("unknown_missing_tree".into(),None)};
    let Some(call)=chunk.descendants::<LuaCallExpr>().find(|call|call.get_position()==at)
        else {return ("unknown_call_site".into(),None)};
    if call.syntax().ancestors().any(|node| {
        let kind:LuaSyntaxKind=node.kind().into();
        matches!(kind,LuaSyntaxKind::ClosureExpr|LuaSyntaxKind::IfStat|LuaSyntaxKind::WhileStat
            |LuaSyntaxKind::RepeatStat|LuaSyntaxKind::ForStat|LuaSyntaxKind::ForRangeStat)
    }) {
        return ("unknown_call_execution_order".into(),None);
    }
    let mut current = initial.clone();
    // Each alias captures a value at its initializer, not at the final call.
    // Reassignment after that copy cannot retroactively change the copied value.
    let mut observed_at=at;
    let mut seen = HashSet::new();
    for _ in 0..64 {
        if !seen.insert(current.clone()) {
            return ("unknown_alias_cycle".into(), None);
        }
        let SemanticId::Decl(key) = &current else {
            return ("unknown_nonlocal_initializer".into(), None);
        };
        if key.file_id != model.file_id() {
            return ("unknown_cross_file_execution_order".into(), None);
        }
        let Some(facts) = model.file_facts() else { return ("unknown_missing_facts".into(), None) };
        let Some(decl) = facts.decl_by_id(&current) else { return ("unknown_missing_decl".into(), None) };
        if !decl.kind.is_local() {
            return ("unknown_mutable_global".into(), None);
        }
        if key.name_range.start()>=observed_at {
            return ("unknown_initializer_execution_order".into(),None);
        }
        for assignment in chunk.descendants::<LuaAssignStat>() {
            if assignment.get_position() < observed_at && assignment.get_position() > key.name_range.start() {
                let (vars, _) = assignment.get_var_and_expr_list();
                if vars.iter().any(|var| model.find_decl(var.syntax().clone().into()).as_ref() == Some(&current)) {
                    return ("unknown_reassigned_before_call".into(), None);
                }
            }
        }
        // `function alias() ... end` assigns to a visible local as well as globals.
        for assignment in chunk.descendants::<LuaFuncStat>() {
            if assignment.get_position() < observed_at && assignment.get_position() > key.name_range.start() {
                let Some(var)=assignment.get_func_name() else {continue};
                if model.find_decl(var.syntax().clone().into()).as_ref()==Some(&current) {
                    return ("unknown_reassigned_before_call".into(),None);
                }
            }
        }
        let Some(syntax) = decl.value_expr_syntax else { return ("unknown_no_initializer".into(), None) };
        let Some(tree) = model.syntax_tree() else { return ("unknown_missing_tree".into(), None) };
        let Some(expr) = syntax.to_node_from_root(&tree.get_red_root()).and_then(LuaExpr::cast)
            else { return ("unknown_initializer_syntax".into(), None) };
        // Names and function literals yield exactly one value. A later slot is
        // padded with nil, even when this declaration is reached through a copy.
        if decl.multi_return_index.is_some_and(|slot|slot>0) {
            return match expr {
                LuaExpr::NameExpr(_)|LuaExpr::ClosureExpr(_) => ("known_non_function_nil".into(),None),
                _ => ("unknown_multi_return_value".into(),None),
            };
        }
        match expr {
            LuaExpr::ClosureExpr(_) => return ("initializer_function_origin".into(), Some(current)),
            LuaExpr::NameExpr(name) => {
                let Some(next) = model.resolve_name(name.get_position())
                    else { return ("unknown_initializer_binding".into(), None) };
                observed_at=name.get_position();
                current = next;
            }
            _ => return ("unknown_initializer_not_supported".into(), None),
        }
    }
    ("unknown_alias_depth_limit".into(), None)
}

fn golden(out: &mut Output) -> ProbeResult<()> {
    for (name, source, accepted) in [
        ("lua51_goto_identifier", "local goto = 1\nreturn goto\n", true),
        ("lua51_reject_goto_statement", "goto target\n", false),
        ("lua51_reject_label", "::target::\n", false),
    ] {
        let tree = LuaParser::parse(source, ParserConfig::with_level(LuaLanguageLevel::Lua51));
        let errors: Vec<_> = tree.get_errors().iter()
            .filter(|error| error.kind == LuaParseErrorKind::SyntaxError)
            .map(|error| json!({"bytes":range_value(error.range),"message":short(&error.message,256)})).collect();
        out.check_engine(name, json!(accepted), json!(errors.is_empty()))?;
        out.emit(json!({"event":"parser_evidence","case":name,"sourceSHA256":hash(source.as_bytes()),"errors":errors}))?;
    }
    let bom_text=std::str::from_utf8(BOM_UNICODE).map_err(|_|"BOM fixture is not UTF8")?;
    let inputs = [("scope.lua",SCOPE),("global-a.lua",GLOBAL_A),("global-b.lua",GLOBAL_B),
        ("aliases.lua",ALIASES),("member.lua",MEMBER),("types.lua",TYPES),
        ("diagnostic.lua",DIAGNOSTIC),("unicode.lua",UNICODE),
        ("writes-redeclaration.lua",WRITES),("bom-unicode.lua",bom_text),
        ("alias-function-write.lua",ALIAS_FUNCTION_WRITE),("alias-deferred-call.lua",ALIAS_DEFERRED_CALL),
        ("alias-copied-value.lua",ALIAS_COPIED_VALUE),("alias-copy-chain.lua",ALIAS_COPY_CHAIN),
        ("alias-loop-call.lua",ALIAS_LOOP_CALL),("alias-name-nil-slot.lua",ALIAS_NAME_NIL_SLOT),
        ("alias-closure-nil-slot.lua",ALIAS_CLOSURE_NIL_SLOT),("alias-name-first-slot.lua",ALIAS_NAME_FIRST_SLOT),
        ("alias-nil-slot-copy.lua",ALIAS_NIL_SLOT_COPY)];
    out.emit(json!({"event":"golden_inputs","files":inputs.iter().map(|(path,text)|
        json!({"path":path,"bytes":text.len(),"sha256":hash(text.as_bytes())})).collect::<Vec<_>>()}))?;
    let analysis = build_analysis(out, inputs.iter().map(|(p,t)|(p.to_string(),t.to_string())).collect(), None)?;
    for (path, _) in inputs {
        let model = analysis.semantic_model(file_id(&analysis,path)?);
        out.check(&format!("{path}_parses"), json!(0), json!(model.parse_errors().unwrap_or_default().len()))?;
    }

    let scope_id = file_id(&analysis,"scope.lua")?;
    let model = analysis.semantic_model(scope_id);
    let outer_pos = marker(SCOPE,"local value = 1","value");
    let inner_pos = marker(SCOPE,"local value = 2","value");
    let outer = SemanticId::decl(scope_id,range(outer_pos,5));
    let inner = SemanticId::decl(scope_id,range(inner_pos,5));
    for (case, context, expected) in [
        ("upvalue_resolves_outer", "captured = value", &outer),
        ("shadow_resolves_inner", "inner_read = value", &inner),
        ("block_exit_resolves_outer", "return value, captured", &outer),
        ("top_level_resolves_outer", "return value, outer", &outer),
    ] {
        let actual = model.resolve_name((marker(SCOPE,context,"value") as u32).into());
        out.check(case,id_value(&analysis,Some(expected))?,id_value(&analysis,actual.as_ref())?)?;
    }
    check_references(out,&analysis,"outer_exact_references",&outer,expected_ranges(scope_id,&[
        marker(SCOPE,"captured = value","value"),marker(SCOPE,"return value, captured","value"),
        marker(SCOPE,"return value, outer","value")],5))?;
    check_references(out,&analysis,"inner_exact_references",&inner,
        expected_ranges(scope_id,&[marker(SCOPE,"inner_read = value","value")],5))?;
    drop(model);

    let a = file_id(&analysis,"global-a.lua")?;
    let b = file_id(&analysis,"global-b.lua")?;
    let shared = SemanticId::decl(a,range(marker(GLOBAL_A,"function SharedProbe","SharedProbe"),11));
    let model = analysis.semantic_model(b);
    let use_offset = marker(GLOBAL_B,"alias = SharedProbe","SharedProbe");
    out.check("cross_file_global_definition",id_value(&analysis,Some(&shared))?,
        id_value(&analysis,model.resolve_name((use_offset as u32).into()).as_ref())?)?;
    let call_offset = GLOBAL_B.find("\nSharedProbe()").expect("fixture call") + 1;
    check_references(out,&analysis,"cross_file_exact_refs_exclude_string_comment",&shared,
        expected_ranges(b,&[use_offset,call_offset],11))?;
    let calls: Vec<_> = model.chunk().ok_or("missing global chunk")?.descendants::<LuaCallExpr>()
        .filter_map(|call| call.get_prefix_expr().map(|callee| (callee.get_range(),callee_identity(&model,&callee))))
        .collect();
    let actual_calls=calls.iter().map(|(r,id)|Ok(json!({"bytes":analysis.range_value(b,*r)?,
        "target":id_value(&analysis,id.as_ref())?}))).collect::<ProbeResult<Vec<_>>>()?;
    out.check("call_not_plain_reference",json!([{ "bytes":[call_offset,call_offset+11],"target":id_value(&analysis,Some(&shared))? }]),
        json!(actual_calls))?;
    drop(model);

    let alias_file = file_id(&analysis,"aliases.lua")?;
    let model = analysis.semantic_model(alias_file);
    let original = SemanticId::decl(alias_file,range(marker(ALIASES,"function original","original"),8));
    let alias = SemanticId::decl(alias_file,range(marker(ALIASES,"local alias","alias"),5));
    let alias_calls: Vec<_> = model.chunk().ok_or("missing alias chunk")?.descendants::<LuaCallExpr>().collect();
    out.check("alias_fixture_two_calls",json!(2),json!(alias_calls.len()))?;
    for (index, call) in alias_calls.iter().enumerate() {
        let callee = call.get_prefix_expr().ok_or("missing alias callee")?;
        out.check(&format!("alias_call_{index}_lexical_binding"),id_value(&analysis,Some(&alias))?,
            id_value(&analysis,callee_identity(&model,&callee).as_ref())?)?;
        let (state, origin) = initializer_origin_before(&model,&alias,call.get_position());
        let expected = if index == 0 {json!({"state":"initializer_function_origin","origin":id_value(&analysis,Some(&original))?})}
            else {json!({"state":"unknown_reassigned_before_call","origin":Value::Null})};
        out.check(&format!("alias_call_{index}_origin_evidence"),expected,
            json!({"state":state,"origin":id_value(&analysis,origin.as_ref())?}))?;
    }
    drop(model);

    let member_file = file_id(&analysis,"member.lua")?;
    let model = analysis.semantic_model(member_file);
    let method = SemanticId::member(member_file,range(marker(MEMBER,"function TableProbe:run","run"),3));
    let member_use = marker(MEMBER,"value = TableProbe.run","run");
    let member_call = MEMBER.find("\nTableProbe:run()").expect("fixture member call") + 1 + "TableProbe:".len();
    let resolved: Vec<_> = model.chunk().ok_or("missing member chunk")?.descendants::<LuaIndexExpr>()
        .filter_map(|index| {
            let key = index.get_index_key()?;
            let r = key.get_range()?;
            (r.start() == TextSize::from(member_use as u32) || r.start() == TextSize::from(member_call as u32))
                .then(|| (r,model.resolve_member(&index).and_then(|r| r.member_id)))
        }).collect();
    let resolved=resolved.iter().map(|(r,id)|Ok(json!({"bytes":analysis.range_value(member_file,*r)?,
        "target":id_value(&analysis,id.as_ref())?}))).collect::<ProbeResult<Vec<_>>>()?;
    out.check("table_member_definition_bindings",json!([
        {"bytes":[member_use,member_use+3],"target":id_value(&analysis,Some(&method))?},
        {"bytes":[member_call,member_call+3],"target":id_value(&analysis,Some(&method))?}]),json!(resolved))?;
    check_references(out,&analysis,"table_member_exact_refs",&method,expected_ranges(member_file,&[member_use,member_call],3))?;
    let actual_calls: Vec<_> = model.chunk().ok_or("missing member chunk")?.descendants::<LuaCallExpr>()
        .filter_map(|call|call.get_prefix_expr()).map(|expr|id_value(&analysis,callee_identity(&model,&expr).as_ref()))
        .collect::<ProbeResult<Vec<_>>>()?;
    out.check("member_call_excludes_value_read",json!([id_value(&analysis,Some(&method))?]),json!(actual_calls))?;
    drop(model);

    let typed_file = file_id(&analysis,"types.lua")?;
    let model = analysis.semantic_model(typed_file);
    let numeric = SemanticId::decl(typed_file,range(marker(TYPES,"local numeric","numeric"),7));
    out.check("annotated_number_type",json!(true),json!(model.type_of_decl(&numeric) == Some(LuaType::Number)))?;
    drop(model);
    let diagnostics = analysis.diagnose_file(file_id(&analysis,"diagnostic.lua")?,CancellationToken::new())
        .ok_or("diagnostic unexpectedly unavailable")?;
    let diagnostics_value = mapped_diagnostics(&analysis,file_id(&analysis,"diagnostic.lua")?,
        serde_json::to_value(&diagnostics).map_err(|e|e.to_string())?)?;
    let mismatch: Vec<_> = diagnostics_value.iter()
        .filter(|d|d.get("code").and_then(Value::as_str)==Some("assign-type-mismatch"))
        .map(|d|json!({"code":d["code"],"range":d["range"]})).collect();
    out.check("precise_assignment_diagnostic",json!([{"code":"assign-type-mismatch",
        "range":{"start":{"line":1,"character":12},"end":{"line":1,"character":19}}}]),json!(mismatch))?;
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    out.check("already_cancelled_diagnostic_returns_unavailable",json!(true),
        json!(analysis.diagnose_file(typed_file,cancelled).is_none()))?;

    let unicode_file = file_id(&analysis,"unicode.lua")?;
    let model = analysis.semantic_model(unicode_file);
    let target = SemanticId::decl(unicode_file,range(marker(UNICODE,"local target","target"),6));
    let unicode_use = marker(UNICODE,"alias = target","target");
    let unicode_return = marker(UNICODE,"return target","target");
    out.check("unicode_byte_definition",id_value(&analysis,Some(&target))?,
        id_value(&analysis,model.resolve_name((unicode_use as u32).into()).as_ref())?)?;
    check_references(out,&analysis,"unicode_exact_byte_references",&target,expected_ranges(unicode_file,&[unicode_use,unicode_return],6))?;
    let text_slices: Vec<_> = analysis.original_locations(&reference_partition(&analysis,&target)?.references)?.into_iter()
        .map(|(_,start,end)|UNICODE.get(start as usize..end as usize).unwrap_or("INVALID_BOUNDARY").to_string()).collect();
    out.check("unicode_ranges_round_trip",json!(["target","target"]),json!(text_slices))?;
    out.emit(json!({"event":"unicode_coordinate_evidence","byteOffset":unicode_use,
        "unicodeScalarColumn":UNICODE[..unicode_use].chars().count(),
        "utf16Column":UNICODE[..unicode_use].encode_utf16().count(),
        "libraryPosition":model.document_current().and_then(|d|d.get_line_col((unicode_use as u32).into()))}))?;
    drop(model);
    write_redeclaration_golden(out,&analysis)?;
    bom_golden(out,&analysis,bom_text)?;
    alias_counterexample_golden(out,&analysis)?;
    alias_slot_golden(out,&analysis)?;
    out.milestone("before_drop")?;
    drop(analysis);
    out.milestone("after_drop")?;
    Ok(())
}

// Regression expectations concern the value used at the call, not just the
// historical initializer spelling. Original red evidence is retained in run 03.
fn alias_counterexample_golden(out: &mut Output, snapshot: &ProbeSnapshot) -> ProbeResult<()> {
    for (path,text,context,expected_state,expect_original,meaning) in [
        ("alias-function-write.lua",ALIAS_FUNCTION_WRITE,"\nalias()","unknown_reassigned_before_call",false,
            "function alias() writes the visible local; the old original initializer cannot prove this call target"),
        ("alias-deferred-call.lua",ALIAS_DEFERRED_CALL,"invoke() alias()","unknown_call_execution_order",false,
            "a nested callee's textual position is not its execution time; the call occurs after the captured alias changes"),
        ("alias-copied-value.lua",ALIAS_COPIED_VALUE,"\nalias()","initializer_function_origin",true,
            "the top-level alias copies the original function value before original changes; the copied value still reaches this call"),
        ("alias-copy-chain.lua",ALIAS_COPY_CHAIN,"\nalias()","initializer_function_origin",true,
            "each copy captures its source at that initializer; later writes to either source leave the twice-copied function intact"),
        ("alias-loop-call.lua",ALIAS_LOOP_CALL,"    alias()","unknown_call_execution_order",false,
            "the second loop iteration reaches the same textual call after alias changes; a single source offset does not prove its value"),
    ] {
        let file=file_id(snapshot,path)?;
        let model=snapshot.semantic_model(file);
        let alias=SemanticId::decl(file,range(marker(text,"local alias","alias"),5));
        let original=SemanticId::decl(file,range(marker(text,"function original","original"),8));
        let call_at=marker(text,context,"alias");
        let chunk=model.chunk().ok_or("alias counterexample syntax missing")?;
        let matching_calls:Vec<_>=chunk.descendants::<LuaCallExpr>()
            .filter(|call|call.get_position()==TextSize::from(call_at as u32)).collect();
        out.check(&format!("{path}_exact_callee_node"),json!(1),json!(matching_calls.len()))?;
        let call=matching_calls.first().ok_or("alias counterexample selected callee missing")?;
        let callee=call.get_prefix_expr().ok_or("alias counterexample prefix missing")?;
        out.check(&format!("{path}_lexical_binding"),id_value(snapshot,Some(&alias))?,
            id_value(snapshot,callee_identity(&model,&callee).as_ref())?)?;
        let (state,origin)=initializer_origin_before(&model,&alias,call.get_position());
        let expected=json!({"state":expected_state,
            "origin":if expect_original {id_value(snapshot,Some(&original))?} else {Value::Null}});
        let actual=json!({"state":state,"origin":id_value(snapshot,origin.as_ref())?});
        out.check(&format!("{path}_call_value_origin"),expected,actual.clone())?;

        let facts=model.file_facts().ok_or("alias counterexample facts missing")?;
        let declarations=facts.decls.iter().map(|decl|Ok(json!({"identity":id_value(snapshot,Some(&decl.id))?,
            "name":decl.name.as_str(),"initializerBytes":decl.value_expr_syntax
                .map(|syntax|snapshot.range_value(file,syntax.get_range())).transpose()?})))
            .collect::<ProbeResult<Vec<_>>>()?;
        let mut writes=Vec::new();
        for assignment in chunk.descendants::<LuaAssignStat>() {
            let (vars,_)=assignment.get_var_and_expr_list();
            for var in vars {
                writes.push(json!({"kind":"LuaAssignStat","statementBytes":snapshot.range_value(file,assignment.get_range())?,
                    "targetBytes":snapshot.range_value(file,var.get_range())?,
                    "targetIdentity":id_value(snapshot,model.find_decl(var.syntax().clone().into()).as_ref())?}));
            }
        }
        for assignment in chunk.descendants::<LuaFuncStat>() {
            let var=assignment.get_func_name().ok_or("function assignment name missing")?;
            writes.push(json!({"kind":"LuaFuncStat","statementBytes":snapshot.range_value(file,assignment.get_range())?,
                "targetBytes":snapshot.range_value(file,var.get_range())?,
                "targetIdentity":id_value(snapshot,model.find_decl(var.syntax().clone().into()).as_ref())?}));
        }
        let calls=chunk.descendants::<LuaCallExpr>().filter_map(|call|call.get_prefix_expr())
            .map(|callee|Ok(json!({"bytes":snapshot.range_value(file,callee.get_range())?,
                "lexicalBinding":id_value(snapshot,callee_identity(&model,&callee).as_ref())?})))
            .collect::<ProbeResult<Vec<_>>>()?;
        let partition=reference_partition(snapshot,&alias)?;
        out.emit(json!({"event":"alias_counterexample_raw_facts","case":path,"sourceSHA256":hash(text.as_bytes()),
            "coordinate":"original_utf8_bytes","assertionMeaning":meaning,"inferenceRevision":"bounded-copy-time-and-slots-v3",
            "callOriginResult":actual,"declarations":declarations,"writes":writes,"calls":calls,
            "aliasReferencePartition":partition.evidence(snapshot,32)?,
            "runtimeExecution":"not_run; fixed source semantics provide the expected result"}))?;
    }
    Ok(())
}

// Exact Lua single-value adjustment expectations; original red evidence is in run 05.
// Slot 1 is known nil for either a name or closure expression; slot 0 keeps its value.
fn alias_slot_golden(out: &mut Output, snapshot: &ProbeSnapshot) -> ProbeResult<()> {
    for (path,text,alias_context,slot,known_nil) in [
        ("alias-name-nil-slot.lua",ALIAS_NAME_NIL_SLOT,"unused, alias",1usize,true),
        ("alias-closure-nil-slot.lua",ALIAS_CLOSURE_NIL_SLOT,"unused, alias",1usize,true),
        ("alias-name-first-slot.lua",ALIAS_NAME_FIRST_SLOT,"local alias",0usize,false),
        ("alias-nil-slot-copy.lua",ALIAS_NIL_SLOT_COPY,"local alias",0usize,true),
    ] {
        let file=file_id(snapshot,path)?;
        let model=snapshot.semantic_model(file);
        let alias=SemanticId::decl(file,range(marker(text,alias_context,"alias"),5));
        let facts=model.file_facts().ok_or("alias slot facts missing")?;
        let decl=facts.decl_by_id(&alias).ok_or("alias slot exact declaration missing")?;
        out.check(&format!("{path}_exact_assignment_slot"),json!(Some(slot)),json!(decl.multi_return_index))?;
        let call_at=marker(text,"\nalias()","alias");
        let calls:Vec<_>=model.chunk().ok_or("alias slot chunk missing")?.descendants::<LuaCallExpr>()
            .filter(|call|call.get_position()==TextSize::from(call_at as u32)).collect();
        out.check(&format!("{path}_exact_callee_node"),json!(1),json!(calls.len()))?;
        let call=calls.first().ok_or("alias slot callee missing")?;
        let callee=call.get_prefix_expr().ok_or("alias slot prefix missing")?;
        out.check(&format!("{path}_lexical_binding"),id_value(snapshot,Some(&alias))?,
            id_value(snapshot,callee_identity(&model,&callee).as_ref())?)?;
        let expected=if known_nil {
            json!({"state":"known_non_function_nil","origin":Value::Null})
        } else {
            let original=SemanticId::decl(file,range(marker(text,"function original","original"),8));
            json!({"state":"initializer_function_origin","origin":id_value(snapshot,Some(&original))?})
        };
        let (state,origin)=initializer_origin_before(&model,&alias,call.get_position());
        let actual=json!({"state":state,"origin":id_value(snapshot,origin.as_ref())?});
        out.check(&format!("{path}_call_value_origin"),expected,actual.clone())?;
        let declarations=facts.decls.iter().map(|decl|Ok(json!({"identity":id_value(snapshot,Some(&decl.id))?,
            "name":decl.name.as_str(),"multiReturnIndex":decl.multi_return_index,
            "initializerBytes":decl.value_expr_syntax.map(|syntax|snapshot.range_value(file,syntax.get_range())).transpose()?})))
            .collect::<ProbeResult<Vec<_>>>()?;
        out.emit(json!({"event":"alias_slot_raw_facts","case":path,"sourceSHA256":hash(text.as_bytes()),
            "coordinate":"original_utf8_bytes","selectedDeclaration":id_value(snapshot,Some(&alias))?,
            "declarations":declarations,"calleeBytes":snapshot.range_value(file,callee.get_range())?,
            "calleeBinding":id_value(snapshot,callee_identity(&model,&callee).as_ref())?,"callOriginResult":actual,
            "expectedLuaValue":if known_nil {"nil; known non-function"} else {"original function value"},
            "expectationBasis":"a name or closure expression yields one value; assignment pads later slots with nil; copying a padded value preserves nil",
            "inferenceRevision":"bounded-copy-time-and-slots-v3","slotSemanticsRevision":"single-value-padding-v1",
            "runtimeExecution":"not_run; fixed Lua assignment semantics provide the expected result"}))?;
    }
    Ok(())
}

fn write_redeclaration_golden(out: &mut Output, snapshot: &ProbeSnapshot) -> ProbeResult<()> {
    let file=file_id(snapshot,"writes-redeclaration.lua")?;
    let first=SemanticId::decl(file,range(marker(WRITES,"local reused = 0","reused"),6));
    let second=SemanticId::decl(file,range(marker(WRITES,"local reused = 2","reused"),6));
    let first_write=marker(WRITES,"reused = 1","reused");
    let first_read=marker(WRITES,"old = reused","reused");
    let second_write=marker(WRITES,"reused = 3","reused");
    let second_read=marker(WRITES,"return old, reused","reused");
    check_references(out,snapshot,"first_declaration_preserves_ordinary_write",&first,
        expected_ranges(file,&[first_write,first_read],6))?;
    check_references(out,snapshot,"same_name_redeclaration_preserves_own_write",&second,
        expected_ranges(file,&[second_write,second_read],6))?;
    let model=snapshot.semantic_model(file);
    for (name,at,expected) in [("first_write",first_write,&first),("second_write",second_write,&second),
        ("first_read",first_read,&first),("second_read",second_read,&second)] {
        out.check(&format!("redeclaration_{name}_exact_binding"),id_value(snapshot,Some(expected))?,
            id_value(snapshot,model.resolve_name(snapshot.analysis_offset(file,at)?).as_ref())?)?;
    }
    let first_refs=reference_partition(snapshot,&first)?;
    let second_refs=reference_partition(snapshot,&second)?;
    out.check("same_name_declarations_keep_disjoint_references",json!(true),
        json!(first_refs.references.iter().all(|r|!second_refs.references.contains(r))))?;
    Ok(())
}

fn bom_golden(out: &mut Output, snapshot: &ProbeSnapshot, original: &str) -> ProbeResult<()> {
    let file=file_id(snapshot,"bom-unicode.lua")?;
    let mapping=snapshot.mapping(file)?;
    let analyzed=snapshot.db.get_file_text(file).ok_or("missing BOM analyzed fixture")?;
    out.check("bom_only_prefix_is_removed",json!(true),json!(original.as_bytes().starts_with(&[0xef,0xbb,0xbf])
        && mapping.prefix_bytes==3 && original.get(3..)==Some(analyzed)
        && mapping.original_bytes==original.len() && mapping.original_sha256==hash(original.as_bytes())
        && mapping.analyzed_sha256==hash(analyzed.as_bytes()) && mapping.original_sha256!=mapping.analyzed_sha256))?;
    let target_at=marker(original,"local target","target");
    let target=SemanticId::decl(file,TextRange::at(snapshot.analysis_offset(file,target_at)?,6.into()));
    let use_at=marker(original,"alias = target","target");
    let return_at=marker(original,"return target","target");
    let expected_definition=json!({"kind":"decl","file":file.id,"bytes":[target_at,target_at+6]});
    let model=snapshot.semantic_model(file);
    out.check("bom_unicode_definition_original_bytes",expected_definition,
        id_value(snapshot,model.resolve_name(snapshot.analysis_offset(file,use_at)?).as_ref())?)?;
    check_references(out,snapshot,"bom_unicode_references_original_bytes",&target,
        expected_ranges(file,&[use_at,return_at],6))?;
    let caller_at=marker(original,"function caller","caller");
    let call_at=original.find("\ncaller()").ok_or("BOM fixture call marker missing")?+1;
    let calls=model.chunk().ok_or("BOM fixture chunk missing")?.descendants::<LuaCallExpr>()
        .filter_map(|call|call.get_prefix_expr()).map(|callee|Ok(json!({
            "bytes":snapshot.range_value(file,callee.get_range())?,
            "binding":id_value(snapshot,callee_identity(&model,&callee).as_ref())?})))
        .collect::<ProbeResult<Vec<_>>>()?;
    out.check("bom_unicode_callee_original_bytes",json!([{"bytes":[call_at,call_at+6],
        "binding":{"kind":"decl","file":file.id,"bytes":[caller_at,caller_at+6]}}]),json!(calls))?;
    let diagnostics=snapshot.diagnose_file(file,CancellationToken::new()).ok_or("BOM diagnostics unavailable")?;
    let diagnostics=mapped_diagnostics(snapshot,file,serde_json::to_value(diagnostics).map_err(|e|e.to_string())?)?;
    let bad_at=marker(original,"local bad = \"wrong\"","\"wrong\"");
    let mismatch:Vec<_>=diagnostics.iter().filter(|d|d["code"].as_str()==Some("assign-type-mismatch"))
        .map(|d|json!({"code":d["code"],"bytes":d["originalByteRange"],"range":d["range"]})).collect();
    out.check("bom_unicode_diagnostic_original_bytes",json!([{"code":"assign-type-mismatch",
        "bytes":[bad_at,bad_at+7],"range":{"start":{"line":2,"character":12},"end":{"line":2,"character":19}}}]),json!(mismatch))?;
    out.emit(json!({"event":"bom_golden_evidence","originalSHA256":mapping.original_sha256,
        "analyzedSHA256":mapping.analyzed_sha256,"prefixByteDelta":mapping.prefix_bytes,
        "definitionByteOffset":target_at,"definitionOriginalScalarColumn":original[..target_at].chars().count(),
        "definitionOriginalUTF16Column":original[..target_at].encode_utf16().count(),"diagnostics":diagnostics}))?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct InputManifest {
    schema: String,
    commit: String,
    documents: usize,
    lua_files: usize,
    lua_bytes: u64,
    #[serde(rename="documentPathSHA256SizeDigest")]
    document_digest: String,
    files: Vec<InputFile>,
}

#[derive(Deserialize)]
struct InputFile { path: String, bytes: u64, sha256: String, mode: String }

fn regular_file(path: &Path) -> ProbeResult<fs::Metadata> {
    let meta = fs::symlink_metadata(path).map_err(|e|format!("{}: {e}",path.display()))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(format!("not a regular non-link file: {}",path.display()));
    }
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 { return Err("reparse point input rejected".into()); }
    }
    Ok(meta)
}

fn read_bounded(path: &Path, maximum: u64) -> ProbeResult<Vec<u8>> {
    if !path.is_absolute() { return Err("input paths must be absolute".into()); }
    let meta = regular_file(path)?;
    if meta.len() > maximum { return Err(format!("input exceeds byte budget: {}",path.display())); }
    let file = File::open(path).map_err(|e|e.to_string())?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(maximum+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    if bytes.len() as u64 > maximum || bytes.len() as u64 != meta.len() {
        return Err("input changed length during read or exceeded byte budget".into());
    }
    Ok(bytes)
}

fn checked_relative(path: &str) -> ProbeResult<PathBuf> {
    if path.is_empty() || path.chars().any(|c|matches!(c,'\\'|':'|'\0')) || path.split('/').any(|p|p.is_empty()||p=="."||p=="..") {
        return Err(format!("invalid manifest path: {}",short(path,160)));
    }
    let p = PathBuf::from(path);
    if p.components().any(|part|!matches!(part,Component::Normal(_))) {
        return Err("manifest path is not relative".into());
    }
    Ok(p)
}

fn contained_file(root: &Path, relative: &Path) -> ProbeResult<PathBuf> {
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        let meta = fs::symlink_metadata(&path).map_err(|e|format!("{}: {e}",path.display()))?;
        if meta.file_type().is_symlink() { return Err("source symlink rejected".into()); }
        #[cfg(windows)] {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 { return Err("source reparse point rejected".into()); }
        }
    }
    Ok(path)
}

fn load_retail(out: &mut Output, manifest_path: &Path, root: &Path, environment_path: &Path)
    -> ProbeResult<Vec<(String,String)>> {
    let start = Instant::now();
    let bytes = read_bounded(manifest_path,4<<20)?;
    let manifest: InputManifest = serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
    if manifest.schema != "lycheedev.source-memory-input.v1" || manifest.commit != SOURCE_COMMIT
        || manifest.documents != 4036 || manifest.files.len() != 4036 || manifest.lua_files != 2551
        || manifest.lua_bytes != 23354176 || manifest.document_digest != SOURCE_DIGEST {
        return Err("manifest does not identify the complete fixed Retail input".into());
    }
    if !root.is_absolute() { return Err("source root must be absolute".into()); }
    let root = root.canonicalize().map_err(|e|e.to_string())?;
    if !root.is_dir() { return Err("source root must be a directory".into()); }
    let mut digest = Sha256::new();
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    let mut lua_bytes = 0u64;
    let mut sources = Vec::with_capacity(2551);
    for input in &manifest.files {
        let relative = checked_relative(&input.path)?;
        if input.mode != "100644" || !seen.insert(input.path.to_lowercase()) || input.bytes > FILE_LIMIT
            || input.sha256.len()!=64 || !input.sha256.bytes().all(|c|c.is_ascii_hexdigit()&&!c.is_ascii_uppercase()) {
            return Err("invalid, duplicate, oversized or unsupported input record".into());
        }
        total = total.checked_add(input.bytes).ok_or("document byte sum overflow")?;
        if total>DOCUMENT_LIMIT { return Err("total document byte budget exceeded".into()); }
        let path = contained_file(&root,&relative)?;
        let content = read_bounded(&path,FILE_LIMIT)?;
        if content.len() as u64 != input.bytes || hash(&content)!=input.sha256 {
            return Err(format!("fixed file hash/length mismatch: {}",input.path));
        }
        digest.update(input.path.as_bytes()); digest.update([0]);
        digest.update(input.sha256.as_bytes()); digest.update([0]); digest.update(input.bytes.to_le_bytes());
        if input.path.to_ascii_lowercase().ends_with(".lua") {
            lua_bytes += input.bytes;
            let text = String::from_utf8(content).map_err(|_|format!("non-UTF8 Lua input: {}",input.path))?;
            sources.push((input.path.clone(),text));
        }
    }
    if format!("{:x}",digest.finalize())!=SOURCE_DIGEST || sources.len()!=2551 || lua_bytes!=23354176 {
        return Err("verified document set differs from fixed Retail input".into());
    }
    out.emit(json!({"event":"verified_input","manifestSHA256":hash(&bytes),"sourceCommit":SOURCE_COMMIT,
        "documentsVerified":manifest.documents,"documentBytesVerified":total,
        "documentPathSHA256SizeDigest":SOURCE_DIGEST,"sourceLuaFiles":sources.len(),"sourceLuaBytes":lua_bytes,
        "sourceRoot":root,"environmentPath":environment_path,"verificationMicros":start.elapsed().as_micros(),
        "luaExcluded":0,"nonLuaDisposition":"hash-verified; TOC/XML semantic adaptation not implemented"}))?;
    Ok(sources)
}

fn coverage(analysis: &ProbeSnapshot) -> ProbeResult<Value> {
    let mut source_files=0usize;
    let mut source_bytes=0usize;
    let mut analyzed_bytes=0usize;
    let mut parse_error_files=0usize;
    let mut parse_errors=0usize;
    let mut errors=Vec::new();
    let mut counts=BTreeMap::from([("decls",0usize),("members",0),("nameUses",0),("signatures",0)]);
    for file in analysis.db.main_workspace_file_ids() {
        source_files+=1;
        let model=analysis.semantic_model(file);
        source_bytes+=analysis.mapping(file)?.original_bytes;
        analyzed_bytes+=analysis.db.get_file_text(file).ok_or("missing source text")?.len();
        let facts=model.file_facts().ok_or("missing facts for loaded source file")?;
        *counts.get_mut("decls").expect("count")+=facts.decls.len();
        *counts.get_mut("members").expect("count")+=facts.members.len();
        *counts.get_mut("nameUses").expect("count")+=facts.name_uses.len();
        *counts.get_mut("signatures").expect("count")+=facts.signatures.len();
        let file_errors=model.parse_errors().unwrap_or_default();
        if !file_errors.is_empty() {parse_error_files+=1;}
        parse_errors+=file_errors.len();
        for error in file_errors {
            if errors.len()<24 {errors.push(json!({"file":analysis.db.file_path(file),
                "kind":format!("{:?}",error.kind),"bytes":analysis.range_value(file,error.range)?,
                "coordinate":"original_utf8_bytes","analyzedBytes":range_value(error.range),"message":short(&error.message,256)}));}
        }
    }
    Ok(json!({"sourceLuaFiles":source_files,"sourceLuaBytes":source_bytes,"analyzedLuaBytes":analyzed_bytes,"facts":counts,
        "parseErrorFiles":parse_error_files,"parseErrors":parse_errors,"errorSample":errors,
        "errorSampleTruncated":parse_errors>24,"excludedFiles":0,
        "analysisCompletion":"indexed inputs; semantic query completeness assessed separately"}))
}

fn verify_retail_bom(out: &mut Output, snapshot: &ProbeSnapshot) -> ProbeResult<()> {
    let expected_paths=[
        "Interface/AddOns/Blizzard_CharacterCustomize/Blizzard_CharacterCustomize.lua",
        "Interface/AddOns/Blizzard_Communities/ClubFinderApplicantList.lua",
        "Interface/AddOns/Blizzard_FrameXMLBase/Constants.lua",
    ];
    let expected_ids=expected_paths.iter().map(|path|file_id(snapshot,path)).collect::<ProbeResult<BTreeSet<_>>>()?;
    let changed_ids:BTreeSet<_>=snapshot.mappings.iter().filter(|(_,map)|map.prefix_bytes!=0).map(|(id,_)|*id).collect();
    out.check("retail_exact_three_bom_files",json!(expected_ids.iter().map(|id|id.id).collect::<Vec<_>>()),
        json!(changed_ids.iter().map(|id|id.id).collect::<Vec<_>>()))?;
    for path in expected_paths {
        let file=file_id(snapshot,path)?;
        let map=snapshot.mapping(file)?;
        let original=snapshot.original_text(file)?;
        let analyzed=snapshot.db.get_file_text(file).ok_or("Retail BOM analyzed file missing")?;
        out.check(&format!("{path}_bom_byte_identity"),json!(true),json!(map.prefix_bytes==3
            && original.as_bytes().starts_with(&[0xef,0xbb,0xbf]) && original.get(3..)==Some(analyzed)
            && hash(original.as_bytes())==map.original_sha256 && hash(analyzed.as_bytes())==map.analyzed_sha256))?;
        let model=snapshot.semantic_model(file);
        out.check(&format!("{path}_bom_parse_errors"),json!(0),json!(model.parse_errors().unwrap_or_default().len()))?;
        let facts=model.file_facts().ok_or("Retail BOM facts missing")?;
        let mut sites=Vec::new();
        for decl in &facts.decls {sites.push(("declaration",decl.name_range));}
        for member in &facts.members {
            if let Some(range)=member.id.member_key_range() {sites.push(("member_declaration",range));}
        }
        for use_site in &facts.name_uses {sites.push(("name_reference",use_site.syntax.get_range()));}
        for use_site in &facts.member_uses {sites.push(("member_occurrence",use_site.get_range()));}
        for call in model.chunk().ok_or("Retail BOM chunk missing")?.descendants::<LuaCallExpr>() {
            if let Some(callee)=call.get_prefix_expr() {sites.push(("callee",callee.get_range()));}
        }
        let mut counts=BTreeMap::<&str,usize>::new();
        let mut identities=Vec::new();
        let mut samples=Vec::new();
        for (kind,analyzed_range) in sites {
            let mapped=snapshot.original_range(file,analyzed_range)?;
            *counts.entry(kind).or_default()+=1;
            let bytes=original.get(u32::from(mapped.start()) as usize..u32::from(mapped.end()) as usize)
                .ok_or("Retail BOM original slice invalid")?;
            let record=json!({"kind":kind,"originalBytes":range_value(mapped),
                "analyzedBytes":range_value(analyzed_range),"sliceSHA256":hash(bytes.as_bytes())});
            if samples.len()<12 {samples.push(record.clone());}
            identities.push(record);
        }
        // Check spelling against raw source independently of the analyzed-text equality check.
        let names_match=facts.decls.iter().filter(|decl|!decl.is_implicit_self).all(|decl| {
            let start=u32::from(decl.name_range.start()) as usize+3;
            let end=u32::from(decl.name_range.end()) as usize+3;
            original.get(start..end)==Some(decl.name.as_str())
        }) && facts.name_uses.iter().all(|use_site| {
            let r=use_site.syntax.get_range();
            original.get(u32::from(r.start()) as usize+3..u32::from(r.end()) as usize+3)==Some(use_site.name.as_str())
        });
        out.check(&format!("{path}_bom_original_definition_reference_spelling"),json!(true),json!(names_match))?;
        let diagnostics=snapshot.diagnose_file(file,CancellationToken::new()).ok_or("Retail BOM diagnostics unavailable")?;
        let diagnostics=mapped_diagnostics(snapshot,file,serde_json::to_value(diagnostics).map_err(|e|e.to_string())?)?;
        out.check(&format!("{path}_bom_has_position_checks"),json!(true),json!(!identities.is_empty()))?;
        out.emit(json!({"event":"retail_bom_round_trip","path":path,"originalSHA256":map.original_sha256,
            "analyzedSHA256":map.analyzed_sha256,"prefixMap":{"analyzedToOriginalByteDelta":3},
            "checkedRanges":identities.len(),"rangeCounts":counts,"originalReadbackMismatches":0,
            "rangeIdentitySHA256":hash(&serde_json::to_vec(&identities).map_err(|e|e.to_string())?),
            "rangeSample":samples,"rangeSampleTruncated":identities.len()>12,
            "diagnosticsChecked":diagnostics.len(),"diagnosticsSHA256":hash(&serde_json::to_vec(&diagnostics).map_err(|e|e.to_string())?),
            "diagnosticSample":diagnostics.iter().take(4).collect::<Vec<_>>(),"diagnosticSampleTruncated":diagnostics.len()>4}))?;
    }
    Ok(())
}

fn retail_query(analysis: &ProbeSnapshot) -> ProbeResult<Value> {
    let file=file_id(analysis,CALLBACK)?;
    let text=analysis.db.get_file_text(file).ok_or("missing CallbackRegistry content")?;
    // A fresh model and fresh reference/type/diagnostic calls every time; no result cache.
    let model=analysis.semantic_model(file);
    let mut methods=Vec::new();
    for name in ["RegisterCallback","UnregisterCallback","TriggerEvent"] {
        let needle=format!("function CallbackRegistryMixin:{name}(");
        let offset=text.find(&needle).ok_or_else(||format!("missing fixed method {name}"))?
            + "function CallbackRegistryMixin:".len();
        let expected=SemanticId::member(file,range(offset,name.len()));
        let member=model.members().ok_or("missing method facts")?.iter()
            .find(|member|member.id==expected&&member.is_method)
            .ok_or_else(||format!("exact definition identity not indexed: {name}"))?;
        let partition=reference_partition(analysis,&member.id)?;
        if !partition.partition_is_exact() {return Err("Retail reference partition is not exact".into());}
        let refs=analysis.original_locations(&partition.references)?;
        let definitions=analysis.original_locations(&sorted_ranges(analysis.db.member_definition_ranges(&member.id)))?;
        let ty=model.type_of_member(&member.id);
        let render=ty.as_ref().map(|t|humanize_semantic_type(&model,t));
        methods.push(json!({"name":name,"definition":id_value(analysis,Some(&expected))?,"referenceCount":refs.len(),
            "referencePartition":partition.evidence(analysis,12)?,
            "referencesSHA256":hash(&serde_json::to_vec(&refs).map_err(|e|e.to_string())?),
            "referenceSample":refs.iter().take(12).collect::<Vec<_>>(),"referenceSampleTruncated":refs.len()>12,
            "definitionCount":definitions.len(),"typeKnown":ty.as_ref().is_some_and(|t|!matches!(t,LuaType::Unknown)),
            "type":render.as_ref().map(|s|short(s,512)),"typeRenderTruncated":render.as_ref().is_some_and(|s|s.chars().count()>512)}));
    }
    let mut calls=Vec::new();
    let mut unknown=Vec::new();
    for call in model.chunk().ok_or("missing CallbackRegistry syntax")?.descendants::<LuaCallExpr>() {
        let Some(callee)=call.get_prefix_expr() else {continue};
        let id=callee_identity(&model,&callee);
        let resolved=matches!(id,Some(SemanticId::Decl(_)|SemanticId::Member(_)));
        calls.push(json!({"bytes":analysis.range_value(file,callee.get_range())?,"binding":id_value(analysis,id.as_ref())?}));
        if !resolved && unknown.len()<8 {
            unknown.push(json!({"bytes":analysis.range_value(file,callee.get_range())?,"callee":short(&callee.syntax().text().to_string(),96),
                "reason":"callee expression has no concrete declaration/member binding; runtime target not asserted"}));
        }
    }
    let call_count=calls.len();
    let unresolved=calls.iter().filter(|c| !matches!(c["binding"]["kind"].as_str(),Some("decl"|"member"))).count();
    let call_digest=hash(&serde_json::to_vec(&calls).map_err(|e|e.to_string())?);
    // Selected-file diagnostics only. Full workspace validate is explicitly not claimed.
    let diagnostics=analysis.diagnose_file(file,CancellationToken::new()).ok_or("selected-file diagnostics unavailable")?;
    let mut diagnostics_value=mapped_diagnostics(analysis,file,serde_json::to_value(&diagnostics).map_err(|e|e.to_string())?)?;
    diagnostics_value.sort_by_key(|d|d.to_string());
    let mut codes=BTreeMap::<String,usize>::new();
    for d in &diagnostics_value {
        *codes.entry(d["code"].as_str().unwrap_or("unknown").to_string()).or_default()+=1;
    }
    Ok(json!({"methods":methods,"calleeQueries":call_count,"calleeBindingsUnknown":unresolved,
        "calleeFactsSHA256":call_digest,"unknownSample":unknown,"unknownSampleTruncated":unresolved>8,
        "diagnosticScope":CALLBACK,"diagnosticCount":diagnostics.len(),"diagnosticCodes":codes,
        "diagnosticsSHA256":hash(&serde_json::to_vec(&diagnostics_value).map_err(|e|e.to_string())?),
        "evidenceKind":"static semantic binding and inferred types; runtime call targets remain unproven"}))
}

fn retail(out: &mut Output, manifest: &Path, source: &Path, environment: &Path, repeats: u32) -> ProbeResult<Vec<String>> {
    let sources=load_retail(out,manifest,source,environment)?;
    let env_bytes=read_bounded(environment,FILE_LIMIT)?;
    if env_bytes.len()!=1039688 || hash(&env_bytes)!=ENV_DIGEST {return Err("fixed WoW environment hash/length mismatch".into());}
    out.emit(json!({"event":"verified_environment","bytes":env_bytes.len(),"sha256":ENV_DIGEST,
        "knownExtractionGaps":3449,"environmentCompleteness":"partial inherited API extraction; no gap hidden"}))?;
    let environment=String::from_utf8(env_bytes).map_err(|_|"environment is not UTF8")?;
    let analysis=build_analysis(out,sources,Some(environment))?;
    let coverage=coverage(&analysis)?;
    out.check("retail_loaded_all_lua",json!([2551,23354176]),
        json!([coverage["sourceLuaFiles"],coverage["sourceLuaBytes"]]))?;
    out.emit(json!({"event":"coverage","coverage":coverage}))?;
    verify_retail_bom(out,&analysis)?;
    let mut baseline=None::<String>;
    let mut query_digests=Vec::with_capacity(repeats as usize);
    for iteration in 0..repeats {
        let start=Instant::now();
        let facts=retail_query(&analysis)?;
        let micros=start.elapsed().as_micros();
        let digest=hash(&serde_json::to_vec(&facts).map_err(|e|e.to_string())?);
        if let Some(expected)=&baseline {
            out.check(&format!("retail_query_{iteration}_same_facts"),json!(expected),json!(digest))?;
        } else {baseline=Some(digest.clone());}
        query_digests.push(digest.clone());
        out.emit(json!({"event":"actual_query","iteration":iteration,"queryMicros":micros,
            "freshSemanticModel":true,"resultCacheHit":false,"factsSHA256":digest,"facts":facts}))?;
        out.milestone("after_actual_query")?;
    }
    out.milestone("before_drop")?;
    drop(analysis);
    out.milestone("after_drop")?;
    Ok(query_digests)
}

fn retail_cycles(out: &mut Output, manifest: &Path, source: &Path, environment: &Path,
    cycles: u32, repeats: u32) -> ProbeResult<()> {
    out.emit(json!({"event":"cycle_plan","cycles":cycles,"queriesPerCycle":repeats,
        "cycleIndexBase":0,"processScope":"one process, sequential fresh snapshots",
        "deadlineScope":"one immutable external total deadline; never reset per cycle",
        "commitLimitScope":"one external process/job limit shared by all cycles",
        "resultCache":false,"resourceStabilityAssessment":"external held-native-handle samples required"}))?;
    let mut baseline=None::<Vec<String>>;
    for cycle in 0..cycles {
        out.cycle=Some(cycle);
        let checked_before=out.checked;
        let failed_before=out.failed;
        let started=Instant::now();
        out.emit(json!({"event":"cycle_start","cycles":cycles,"queriesRequested":repeats,
            "inputPolicy":"re-read and verify all 4036 documents and the complete API environment",
            "snapshotPolicy":"fresh EmmyLuaAnalysis; no snapshot or query-result reuse"}))?;
        // Reuse the complete Retail path, including input hashes, BOM assertions,
        // real queries and explicit Drop. Only digest strings survive this call.
        let digests=retail(out,manifest,source,environment,repeats)?;
        if digests.len()!=repeats as usize {return Err("cycle query count differs from request".into());}
        if let Some(expected)=&baseline {
            for (iteration,(expected,actual)) in expected.iter().zip(&digests).enumerate() {
                out.check(&format!("retail_cycle_{cycle}_query_{iteration}_same_facts"),json!(expected),json!(actual))?;
            }
        } else {baseline=Some(digests.clone());}
        out.emit(json!({"event":"cycle_end","state":"queries_returned_and_snapshot_dropped",
            "cycleMicrosIncludingHolds":started.elapsed().as_micros(),"queriesReturned":digests.len(),
            "queryFactsSHA256":digests,"bodyFactsSHA256":hash(&serde_json::to_vec(&digests).map_err(|e|e.to_string())?),
            "bodyFactsHashDefinition":"SHA256 of JSON array of ordered actual_query.factsSHA256 values",
            "assertions":out.checked-checked_before,"failedAssertions":out.failed-failed_before,
            "crossCycleBaseline":0,"resourceStabilityAssessment":"external; this event alone does not prove absence of leaks",
            "fullSourceAcceptance":false}))?;
    }
    out.cycle=None;
    Ok(())
}

fn bounded_number(arg: Option<&String>, default: u64, minimum: u64, maximum: u64) -> ProbeResult<u64> {
    let value=arg.map(|v|v.parse::<u64>().map_err(|_|"invalid unsigned integer".to_string())).transpose()?.unwrap_or(default);
    if value<minimum||value>maximum {return Err("numeric argument outside allowed bounds".into());}
    Ok(value)
}

fn run(out: &mut Output, args: &[String]) -> ProbeResult<()> {
    out.emit(json!({"event":"start","declaredEngineRevision":ENGINE,"argv":args,
        "scope":"direct library baseline only; not production or full Source acceptance",
        "hardDeadlineAndCommitLimitOwner":"external governed process runner"}))?;
    match args.first().map(String::as_str) {
        Some("golden") if args.len()<=2 => {
            out.hold_ms=bounded_number(args.get(1),0,0,5000)?;
            golden(out)?;
        }
        Some("retail") if (4..=6).contains(&args.len()) => {
            let repeats=bounded_number(args.get(4),3,1,100)? as u32;
            out.hold_ms=bounded_number(args.get(5),0,0,5000)?;
            retail(out,Path::new(&args[1]),Path::new(&args[2]),Path::new(&args[3]),repeats)?;
        }
        Some("retail-cycles") if (5..=7).contains(&args.len()) => {
            let cycles=bounded_number(args.get(4),3,2,3)? as u32;
            let repeats=bounded_number(args.get(5),3,2,20)? as u32;
            out.hold_ms=bounded_number(args.get(6),0,0,250)?;
            retail_cycles(out,Path::new(&args[1]),Path::new(&args[2]),Path::new(&args[3]),cycles,repeats)?;
        }
        _ => return Err("usage: emmy_library_probe golden [hold-ms] | retail <absolute-manifest> <absolute-source-root> <absolute-definitions.lua> [repeats 1..100] [hold-ms 0..5000] | retail-cycles <absolute-manifest> <absolute-source-root> <absolute-definitions.lua> <cycles 2..3> [repeats 2..20] [hold-ms 0..250]".into()),
    }
    out.emit(json!({"event":"not_supported","items":["TOC and XML semantic adaptation","XML source map",
        "WoW Mixin propagation","general alias mutation and execution order","cross-CLI reuse",
        "persistent cache invalidation","mid-analysis cooperative cancellation","deadline recovery",
        "all 16 Source business golden cases","full workspace diagnostics","offline rebuild and package acceptance"],
        "state":"not_run_or_not_implemented"}))?;
    out.emit(json!({"event":"end","terminal":true,"baselinePassed":out.failed==0,
        "businessBaselinePassed":out.failed==out.raw_engine_failed,
        "businessAssertions":out.checked-out.raw_engine_checked,"businessFailedAssertions":out.failed-out.raw_engine_failed,
        "rawEngineSyntaxCapability":{"assertions":out.raw_engine_checked,"failedAssertions":out.raw_engine_failed,
            "passed":out.raw_engine_failed==0,"syntaxAuthority":"separate pinned luac 5.1.5, not run by this probe"},
        "assertions":out.checked,"failedAssertions":out.failed,"fullSourceAcceptance":false}))
}

fn main() {
    let args:Vec<_>=std::env::args().skip(1).collect();
    let mut out=Output{started:Instant::now(),bytes:0,failed:0,checked:0,raw_engine_failed:0,raw_engine_checked:0,hold_ms:0,cycle:None};
    let result=run(&mut out,&args);
    let code=match result {
        Ok(()) if out.failed==0 => 0,
        Ok(()) => 1,
        Err(error) => {
            let _=out.emit(json!({"event":"error","terminal":true,"baselinePassed":false,
                "message":short(&error,2000),"fullSourceAcceptance":false}));
            2
        }
    };
    std::process::exit(code);
}
