// Adapted from EmmyLua 08b0873962e83bb8307e9a81052e3f41578fbc78 (MIT).
//! # flow -- per-file control flow graph (CFG)
//!
//! Ported from the old `compilation/analyzer/flow` system; the decl model is now `SemanticId`.
//! `FlowTree` is the per-file fact layer (like `file_facts`): it is invalidated automatically on file changes.
//! Used by later flow-sensitive type queries (narrowing / assignment flow).

mod bind_binary_expr;
mod binder;
mod comment;
mod engine;
mod exprs;
mod flow_node;
mod flow_tree;
mod stats;

use emmylua_parser::LuaChunk;

use emmylua_code_analysis::{FileFacts, FileId};

pub use binder::FlowBinder;
pub use flow_node::*;
pub use flow_tree::*;

/// Merges a label's incoming flow (folds back predecessors), matching the old `bind_analyze::finish_flow_label`.
fn finish_flow_label(binder: &mut FlowBinder, label: FlowId, default: FlowId) -> FlowId {
    if let Some(flow_node) = binder.get_flow(label) {
        if let Some(antecedent) = &flow_node.antecedent {
            if let FlowAntecedent::Single(existing_id) = antecedent {
                return *existing_id;
            }
        } else {
            return default;
        }
    } else {
        return binder.unreachable;
    }
    label
}

/// Build only the requested file from the same immutable public AST and facts.
/// The caller retains the original typed stop reason. Partial nodes never enter
/// the query cache. The iterative engine checks control at every task/resume.
pub(super) fn build<E>(
    file: FileId,
    facts: &FileFacts,
    chunk: LuaChunk,
    check: &mut dyn FnMut() -> Result<(), E>,
) -> Result<FlowTree, E> {
    check()?;
    let mut stopped = None;
    let flow = {
        let mut control = || {
            if stopped.is_some() {
                return false;
            }
            match check() {
                Ok(()) => true,
                Err(error) => {
                    stopped = Some(error);
                    false
                }
            }
        };
        let mut binder = FlowBinder::new(file, facts, &mut control);
        let start = binder.start;
        if let Some(block) = chunk.get_block() {
            engine::run_bind_block(&mut binder, block, start);
        }
        if (binder.control)() {
            Some(binder.finish())
        } else {
            None
        }
    };
    if let Some(error) = stopped {
        return Err(error);
    }
    check()?;
    Ok(flow.expect("a stopped CFG build returns its original control error"))
}
