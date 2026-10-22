// Adapted from EmmyLua 08b0873962e83bb8307e9a81052e3f41578fbc78 (MIT).
// Owned per-query CFG; public AST/facts only, no upstream private types.
use std::collections::HashMap;

use emmylua_parser::{LuaAstPtr, LuaExpr, LuaNameToken, LuaSyntaxId};
use rowan::TextSize;

use super::{
    DeclMultiReturnRefAt, FlowAntecedent, FlowEffect, FlowId, FlowNode, FlowNodeKind, FlowTree,
    LuaClosureId,
};
use emmylua_code_analysis::FileFacts;
use emmylua_code_analysis::FileId;
use emmylua_code_analysis::SemanticId;

pub struct FlowBinder<'a, 'control> {
    pub facts: &'a FileFacts,
    pub control: &'control mut dyn FnMut() -> bool,
    pub file_id: FileId,
    pub decl_bind_expr_ref: HashMap<SemanticId, LuaAstPtr<LuaExpr>>,
    pub decl_multi_return_ref: HashMap<SemanticId, Vec<DeclMultiReturnRefAt>>,
    pub start: FlowId,
    pub unreachable: FlowId,
    pub loop_label: FlowId,
    pub break_target_label: FlowId,
    pub true_target: FlowId,
    pub false_target: FlowId,
    flow_nodes: Vec<FlowNode>,
    node_effects: HashMap<FlowId, Vec<FlowEffect>>,
    multiple_antecedents: Vec<Vec<FlowId>>,
    labels: HashMap<LuaClosureId, HashMap<String, FlowId>>,
    goto_stats: Vec<GotoCache>,
    bindings: HashMap<LuaSyntaxId, FlowId>,
}

impl<'a, 'control> FlowBinder<'a, 'control> {
    pub fn new(
        file_id: FileId,
        facts: &'a FileFacts,
        control: &'control mut dyn FnMut() -> bool,
    ) -> Self {
        let mut binder = FlowBinder {
            facts,
            control,
            file_id,
            flow_nodes: Vec::new(),
            node_effects: HashMap::new(),
            multiple_antecedents: Vec::new(),
            decl_bind_expr_ref: HashMap::new(),
            decl_multi_return_ref: HashMap::new(),
            labels: HashMap::new(),
            start: FlowId::default(),
            unreachable: FlowId::default(),
            break_target_label: FlowId::default(),
            bindings: HashMap::new(),
            goto_stats: Vec::new(),
            loop_label: FlowId::default(),
            true_target: FlowId::default(),
            false_target: FlowId::default(),
        };

        binder.start = binder.create_start();
        binder.unreachable = binder.create_unreachable();
        binder.break_target_label = binder.unreachable;
        binder.loop_label = binder.unreachable;
        binder.true_target = binder.unreachable;
        binder.false_target = binder.unreachable;

        binder
    }

    pub fn create_node(&mut self, kind: FlowNodeKind) -> FlowId {
        let id = FlowId(self.flow_nodes.len() as u32);
        let flow_node = FlowNode {
            id,
            kind,
            antecedent: None,
        };
        self.flow_nodes.push(flow_node);
        id
    }

    pub fn create_branch_label(&mut self) -> FlowId {
        self.create_node(FlowNodeKind::BranchLabel)
    }

    pub fn create_loop_label(&mut self) -> FlowId {
        self.create_node(FlowNodeKind::LoopLabel)
    }

    pub fn create_name_label(&mut self, name: &str, closure_id: LuaClosureId) -> FlowId {
        let label_id = self.create_node(FlowNodeKind::NamedLabel(name.to_owned()));
        self.labels
            .entry(closure_id)
            .or_default()
            .insert(name.to_owned(), label_id);

        label_id
    }

    pub fn get_label(&self, closure_id: LuaClosureId, name: &str) -> Option<FlowId> {
        self.labels
            .get(&closure_id)
            .and_then(|labels| labels.get(name).copied())
    }

    pub fn create_start(&mut self) -> FlowId {
        self.create_node(FlowNodeKind::Start)
    }

    pub fn create_unreachable(&mut self) -> FlowId {
        self.create_node(FlowNodeKind::Unreachable)
    }

    pub fn create_break(&mut self) -> FlowId {
        self.create_node(FlowNodeKind::Break)
    }

    pub fn create_continue(&mut self) -> FlowId {
        self.create_node(FlowNodeKind::Continue)
    }

    pub fn create_return(&mut self) -> FlowId {
        self.create_node(FlowNodeKind::Return)
    }

    pub fn create_decl(&mut self, position: TextSize) -> FlowId {
        self.create_node(FlowNodeKind::DeclPosition(position))
    }

    pub fn add_antecedent(&mut self, node_id: FlowId, antecedent: FlowId) {
        if antecedent == self.unreachable || node_id == self.unreachable {
            // If the antecedent is the unreachable node, we don't need to add it
            return;
        }

        if let Some(existing) = self.flow_nodes.get_mut(node_id.0 as usize) {
            match existing.antecedent {
                Some(FlowAntecedent::Single(existing_id)) => {
                    // If the existing antecedent is a single node, convert it to multiple
                    if existing_id == antecedent {
                        return; // No change needed if it's the same antecedent
                    }
                    existing.antecedent = Some(FlowAntecedent::Multiple(
                        self.multiple_antecedents.len() as u32,
                    ));
                    self.multiple_antecedents
                        .push(vec![existing_id, antecedent]);
                }
                Some(FlowAntecedent::Multiple(index)) => {
                    // Add to multiple antecedents
                    if let Some(multiple) = self.multiple_antecedents.get_mut(index as usize) {
                        multiple.push(antecedent);
                    } else {
                        self.multiple_antecedents.push(vec![antecedent]);
                    }
                }
                _ => {
                    // Set new antecedent
                    existing.antecedent = Some(FlowAntecedent::Single(antecedent));
                }
            };
        }
    }

    pub fn bind_syntax_node(&mut self, syntax_id: LuaSyntaxId, flow_id: FlowId) {
        self.bindings.insert(syntax_id, flow_id);
    }

    pub fn get_bind_flow(&self, syntax_id: LuaSyntaxId) -> Option<FlowId> {
        self.bindings.get(&syntax_id).copied()
    }

    pub fn cache_goto_flow(
        &mut self,
        closure_id: LuaClosureId,
        label_token: LuaNameToken,
        label: &str,
        flow_id: FlowId,
    ) {
        self.goto_stats.push(GotoCache {
            closure_id,
            label_token,
            label: label.to_owned(),
            flow_id,
        });
    }

    #[allow(unused)]
    pub fn get_goto_caches(&mut self) -> Vec<GotoCache> {
        std::mem::take(&mut self.goto_stats)
    }

    pub fn get_flow(&self, flow_id: FlowId) -> Option<&FlowNode> {
        self.flow_nodes.get(flow_id.0 as usize)
    }

    pub fn add_effect(&mut self, flow_id: FlowId, effect: FlowEffect) {
        self.node_effects.entry(flow_id).or_default().push(effect);
    }

    pub fn finish(mut self) -> FlowTree {
        // Post-process goto edges: the flow recorded for `goto label` becomes the
        // antecedent of the NamedLabel node, so type backtracking after the label
        // includes the goto source path (FlowTree no longer needs to store labels).
        let goto_stats = std::mem::take(&mut self.goto_stats);
        for goto in goto_stats {
            if !(self.control)() {
                break;
            }
            if let Some(label_id) = self.get_label(goto.closure_id, goto.label.as_str()) {
                self.add_antecedent(label_id, goto.flow_id);
            }
        }
        FlowTree::new(
            self.decl_bind_expr_ref,
            self.decl_multi_return_ref,
            self.flow_nodes,
            self.node_effects,
            self.multiple_antecedents,
            // self.labels,
            self.bindings,
            self.control,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GotoCache {
    pub closure_id: LuaClosureId,
    pub label_token: LuaNameToken,
    pub label: String,
    pub flow_id: FlowId,
}
