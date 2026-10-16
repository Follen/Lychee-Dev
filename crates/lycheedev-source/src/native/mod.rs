//! Direct-library Source facts shared by source research and flow.
use std::{collections::BTreeMap, io::Write, path::PathBuf, sync::Arc};
use emmylua_code_analysis::{EmmyLuaAnalysis, Emmyrc, file_path_to_uri};
use emmylua_parser::{LuaAstNode, LuaAstToken, LuaBlock, LuaChunk, LuaClosureExpr, LuaExpr, LuaIndexKey, LuaLiteralToken, LuaNameExpr, LuaStat};
use rowan::TextRange;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, String>;


pub const PROFILE: &str = "fixture-wow-shallow-copy-v1";
pub const ENGINE_COMMIT: &str = "08b0873962e83bb8307e9a81052e3f41578fbc78";
pub const ENGINE_ARCHIVE_SHA256: &str = "ae529a06b551dbb301744bea2b58ba94cb3e30340f63a8c83f9335be9086fdb7";
pub const RULES_VERSION: &str = "wow-binding-value-graph-v7-conditional-footprints";
pub const SOURCE_MAP_PROFILE: &str = "identity-original-utf8-no-transform-v1";
pub const DEPENDENCY_LOCK: &[u8] = include_bytes!("../../../../Cargo.lock");
pub fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
pub fn digest(value: &impl Serialize) -> String { hash(&serde_json::to_vec(value).expect("serializable bounded facts")) }

include!("evidence.rs");
include!("inputs.rs");
include!("host.rs");
include!("origin.rs");
include!("harvest_evidence.rs");
include!("graph.rs");
include!("invocation.rs");
include!("entry.rs");
include!("xml.rs");
include!("host_runtime.rs");
include!("control.rs");
include!("context.rs");
include!("lifecycle.rs");
include!("syntax.rs");
include!("queries.rs");
include!("flow.rs");
include!("research.rs");

#[cfg(test)]
mod sharing_tests;
