//! Validated Lua ABI for the current runtime-root Mailbox reader.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    pub id: String,
    pub state_globals: u64,
    pub tag_offset: u64,
    pub string_length: u64,
    pub string_data: u64,
    pub table_nodes: u64,
    pub table_log2: u64,
    pub node_bytes: u64,
}
