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
pub const SOURCE_MAP_PROFILE: &str = "identity-original-utf8-no-transform-v1";
pub const DEPENDENCY_LOCK: &[u8] = include_bytes!("../../../../Cargo.lock");

/// Test-only typed stop: a live deadline with a cancelled signal yields the
/// cancellation cause, a past deadline with a live signal yields the deadline
/// cause. No raw token is exported beyond the operation lineage.
#[cfg(test)]
pub(crate) fn stopped_control(
    expired: bool,
    live_deadline: std::time::Instant,
) -> lycheedev_core::control::OperationControl {
    let signal = lycheedev_core::control::TrackedCancellation::new();
    if !expired {
        signal.cancel();
    }
    lycheedev_core::control::OperationControl::tracked(
        if expired {
            std::time::Instant::now() - std::time::Duration::from_secs(1)
        } else {
            live_deadline
        },
        signal,
    )
}
pub fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
struct JsonDigestWriter(Sha256);
impl Write for JsonDigestWriter {
    fn write(&mut self,bytes:&[u8])->std::io::Result<usize>{self.0.update(bytes);Ok(bytes.len())}
    fn flush(&mut self)->std::io::Result<()>{Ok(())}
}
pub fn digest(value: &impl Serialize) -> String {
    let mut writer=JsonDigestWriter(Sha256::new());
    serde_json::to_writer(&mut writer,value).expect("serializable bounded facts");
    format!("{:x}",writer.0.finalize())
}

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

#[cfg(test)]
mod digest_tests;

#[cfg(test)]
mod admission_contract_tests;
#[cfg(test)]
mod query_contract_tests;
#[cfg(test)]
mod entry_contract_tests;
#[cfg(test)]
mod owned_context_contract_tests;
