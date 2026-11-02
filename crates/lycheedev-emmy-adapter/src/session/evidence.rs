//! Owned selected evidence. No patched engine types are referenced.
// Exact existing business vocabulary; dependency direction is owned -> public Emmy.
pub use lycheedev_binding_contracts::{
    BindingDecision, EnvironmentCallable, EnvironmentOrigin, EnvironmentOwner, EnvironmentRecord,
    EnvironmentReturn, EnvironmentSlot, EnvironmentTypeCatalog, FieldPresence, ObjectViewKey,
    ShapeEnumeration,
};

use crate::ByteRange;
use emmylua_code_analysis::{FileId, SemanticId};
use emmylua_parser::LuaSyntaxId;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BindingSite {
    pub file_id: FileId,
    pub syntax: LuaSyntaxId,
}
#[derive(Clone, Debug)]
pub struct BoundValue {
    pub identity: String,
    pub proof: String,
    pub value: SelectedValue,
}
#[derive(Clone, Debug)]
pub enum SelectedValue {
    Expression(BindingSite),
    Parameter(SemanticId),
    Function(FunctionRef),
    Object(ObjectRef),
    DerivedNumber(DerivedNumber),
    EnvironmentCallable(Arc<EnvironmentCallable>),
    EnvironmentReturn(EnvironmentReturn),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionRef {
    pub file_id: FileId,
    pub closure: LuaSyntaxId,
    pub declaration: Option<SemanticId>,
}
#[derive(Clone, Debug)]
pub struct SourceSpan {
    pub file_id: FileId,
    pub range: ByteRange,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalSourceSpan {
    pub path: String,
    pub content_sha256: String,
    pub range: ByteRange,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectOrigin {
    Lua(BindingSite),
    Xml(ExternalSourceSpan),
}
#[derive(Clone, Debug)]
pub enum FieldWriteOrigin {
    Lua(SourceSpan),
    Xml(ExternalSourceSpan),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectRef {
    pub identity: String,
    pub epoch: String,
    pub origin: ObjectOrigin,
    pub view: ObjectViewKey,
}
#[derive(Clone, Debug)]
pub struct ObjectShape {
    pub key: ObjectViewKey,
    pub fields: Vec<ObjectField>,
    pub enumeration: ShapeEnumeration,
    pub proof: String,
}
#[derive(Clone, Debug)]
pub struct ObjectField {
    pub key: String,
    pub presence: FieldPresence,
    pub value: BindingDecision<BoundValue>,
    pub destination_write: FieldWriteOrigin,
    pub source_write: Option<FieldWriteOrigin>,
    pub copy_origin: Option<FieldWriteOrigin>,
}
#[derive(Clone, Debug)]
pub struct DerivedNumber {
    pub value: f64,
    pub origin: SourceSpan,
    pub occurrence_identity: String,
    pub rule: DerivedNumberRule,
    pub proof: String,
}
#[derive(Clone, Debug)]
pub enum DerivedNumberRule {
    NumericForIteration {
        statement: BindingSite,
        header: Vec<BindingSite>,
        iteration: usize,
        predecessor: Option<String>,
    },
}
#[derive(Clone, Debug)]
pub struct CallPermit {
    pub callee: BindingSite,
    pub proof: String,
    pub capability_profile: String,
}
#[derive(Clone, Debug)]
pub enum ObservedCallee {
    Source(FunctionRef),
    Environment(Arc<EnvironmentCallable>),
}
#[derive(Clone, Debug)]
pub struct CallObservation {
    pub callee: ObservedCallee,
    pub selected_callee_identity: String,
    pub closure_identity: String,
    pub invocation_identity: String,
    pub returns: Vec<BindingDecision<BoundValue>>,
    /// Returns contains only the observed prefix; further slots are unknown.
    pub open_tail: bool,
    pub proof: String,
}
#[derive(Clone, Debug)]
pub struct BindingReference {
    pub site: BindingSite,
    pub proof: String,
    pub execution_context: String,
}

pub trait BindingContext: Send + Sync {
    fn identity(&self) -> &str;
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue>;
    fn call(&self, _site: BindingSite) -> BindingDecision<CallPermit> {
        BindingDecision::NotManaged
    }
    fn call_observation(&self, _site: BindingSite) -> BindingDecision<CallObservation> {
        BindingDecision::NotManaged
    }
    fn object_shape(&self, _key: &ObjectViewKey) -> BindingDecision<ObjectShape> {
        BindingDecision::Unavailable {
            proof: String::new(),
            reason: "object_shape_provider_unavailable".into(),
        }
    }
    fn references(&self, _identity: &str) -> BindingDecision<Vec<BindingReference>> {
        BindingDecision::Unavailable {
            proof: String::new(),
            reason: "reference_provider_unavailable".into(),
        }
    }
}
/// Public semantic-quality receipt. `native_projected` / `native_checked` are
/// the established completion states, independent of whether the algorithm
/// lives in the upstream engine or this owned backend. Actual owned checker
/// execution is reported separately by `DiagnosticRun::execution`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BindingCoverage {
    pub site: BindingSite,
    pub operation: &'static str,
    pub state: &'static str,
    pub proof: String,
    pub reason: Option<String>,
    pub parameter: Option<String>,
    pub parameter_index: Option<usize>,
    pub argument: Option<BindingSite>,
}
