//! Bounded publication evidence, independent of source analysis and executable storage.
//!
//! Producers supply complete source templates (including unused declarations), pre-
//! simplification canonical bodies, and checked selections. Executable verifiers must
//! reconcile these records with their own inventory and surviving instructions; wire
//! records never certify their own completeness. Import publications and retained
//! demands through one `DeclarationCorrespondence` before comparing them.

pub mod admission;
pub mod correspondence;
pub mod demands;
pub mod limits;
mod mapping;
pub mod protected;
#[doc(hidden)]
pub mod wire;

use crate::{
    callable::{
        construction::evidence::ConstructionWitness, generic::GenericBody,
        interface::InterfaceCallContract, shared::SharedCall, witness::OperationWitness,
    },
    ids::FunctionRef,
    native_import::NativeImport,
    source::manifest::SourceManifest,
    types::ConcreteFunctionIdentity,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{
    callable::Signature,
    ty::{NominalTy, Ty},
};
use serde::{Deserialize, Serialize};

/// Mandatory publication record. An empty record is valid only for an actually empty
/// installation, as established by `admission::validate_inventory`, not by a flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateEvidence {
    /// Compiler-captured complete source anchors. None forbids source attachment.
    /// Part of canonical executable identity even when the text package is stripped.
    pub source_manifest: Option<SourceManifest>,
    pub sources: Vec<SourceRecord>,
    pub bodies: Vec<BodyRecord>,
    pub roots: Vec<ProtectedRoot>,
    pub targets: Vec<CurrentTarget>,
    /// Complete checked selection relationships, including otherwise unused ones.
    pub selections: Vec<CallRequirement>,
}

/// Canonical checked source, not source locations or a text hash. For `static X =
/// f(1 + 2)`, tokens retain `1`, `+`, `2` separately; inputs retain f's resolved ID
/// and checked constants/types. Whitespace/comments are omitted by the collector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    pub declaration: DefinitionId,
    pub generic: Option<GenericBody>,
    pub tokens: Vec<SourceToken>,
    pub inputs: Vec<CanonicalOperand>,
    pub calls: Vec<CallRequirement>,
    pub dependencies: Vec<Dependency>,
}

/// Token kind belongs to the collector's stable token vocabulary; payload is exact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceToken {
    pub kind: u32,
    pub text: String,
}

/// Explicit checked classification; zero captures never imply a live named function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyClassification {
    Named(DefinitionId),
    Closure(ConcreteFunctionIdentity),
    Async(ConcreteFunctionIdentity),
    Resume(ConcreteFunctionIdentity),
    /// Source-local callable adapter; the application is its actual enclosing body.
    Adapter(ConcreteFunctionIdentity),
    StaticInitializer(DefinitionId),
    /// Module-owned protocol helper or intrinsic interface bridge, not an authored body.
    IntrinsicAdapter(ConcreteFunctionIdentity),
}

/// Exact body-origin application plus a shared body's binder arguments. Generated
/// origins belong to their enclosing computation, not a signature-based match.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BodyApplication {
    pub body: ConcreteFunctionIdentity,
    pub arguments: Vec<Ty>,
}

/// Canonical executable before optional simplification. `format` identifies the
/// producer's instruction vocabulary. Registers/blocks are locally numbered;
/// references to definitions and types must use typed operands, never raw slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRecord {
    pub requirement: BodyRequirement,
    pub classification: BodyClassification,
    pub signature: Signature,
    pub captures: Vec<Ty>,
    pub format: u32,
    pub instructions: Vec<CanonicalInstruction>,
    /// Vocabulary and complete instruction record of the emitted executable, checked
    /// by independent backend extraction. Equal to the preserved fields when no
    /// simplification/lowering changes the canonical executable representation.
    pub actual_format: u32,
    pub actual_instructions: Vec<CanonicalInstruction>,
    pub dependencies: Vec<Dependency>,
}

/// Historical body edges survive replacement without retaining executable leases.
/// Two versions of the same origin can contribute different requirement records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRequirement {
    pub origin: ConcreteFunctionIdentity,
    pub generic: Option<GenericBody>,
    pub calls: Vec<CallRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalInstruction {
    pub opcode: u32,
    pub operands: Vec<CanonicalOperand>,
}

/// Literal bytes include their exact payload (including float bits). `Call` indexes
/// the owning record's checked calls. Slot-like integers are only local operands;
/// cross-body, native and declaration references must not be encoded as integers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalOperand {
    Local(u32),
    Block(u32),
    Unsigned(u64),
    Signed(i64),
    Bytes(Vec<u8>),
    Declaration(DefinitionId),
    Type(Ty),
    Application(ConcreteFunctionIdentity),
    Call(u32),
}

/// Existing semantic contracts are the sole authority for application/witness types.
/// Interface coverage uses the candidate's checked selection catalog, not a recorded
/// finite target list that could silently omit a newly admissible implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallRequirement {
    Direct(ConcreteFunctionIdentity),
    Shared(Box<SharedCall>),
    Interface(Box<InterfaceCallContract>),
    RetainedBody(BodyApplication),
    Native(Box<NativeImport>),
    Operation(Box<OperationWitness>),
    Construction(Box<ConstructionWitness>),
    Layout(Ty),
    Unknown(UnknownReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnknownReason {
    FunctionValue,
    InterfaceDispatch,
    NativeReentry,
}

/// Source edges preserve inputs erased by optimization. Body edges are exact
/// origins; calls separately describe current-call and retained-body semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Dependency {
    Source(DefinitionId),
    Body(ConcreteFunctionIdentity),
    Unknown(UnknownDependency),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnknownDependency {
    /// Local edge number, used to report the unknown cause within the root closure.
    pub edge: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RootKind {
    KeyHash,
    KeyEq,
    StaticInitializer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedRoot {
    pub declaration: ConcreteFunctionIdentity,
    pub kind: RootKind,
    pub dependencies: Vec<Dependency>,
    pub selections: Vec<CallRequirement>,
}

/// Placement is publication-local and is deliberately excluded from protected body
/// equality. The referenced body owns its signature/binder/capture contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentTarget {
    pub application: ConcreteFunctionIdentity,
    pub body: ConcreteFunctionIdentity,
    pub module: u32,
    pub function: FunctionRef,
}

/// Monotonic union owned by the installation, never a candidate-supplied baseline.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DemandEnvelope {
    pub applications: Vec<BodyApplication>,
    pub bodies: Vec<BodyRequirement>,
    pub requirements: Vec<CallRequirement>,
}

/// Missing candidate products found by the same walker used for admission. These
/// are requests, never proof or permission to execute. The compiler applies a batch
/// using its existing planner, then expands/revalidates again with the same budget.
/// Import the envelope/candidate into one correspondence scope before expansion;
/// these records support the existing importer for the return trip to compiler IDs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DemandRequests {
    pub functions: Vec<ConcreteFunctionIdentity>,
    /// Exact shared entry plus checked binder arguments; not a monomorphization.
    pub shared: Vec<BodyApplication>,
    pub interfaces: Vec<InterfaceRequest>,
    pub constructions: Vec<ConcreteFunctionIdentity>,
    pub layouts: Vec<Ty>,
}

impl DemandRequests {
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
            && self.shared.is_empty()
            && self.interfaces.is_empty()
            && self.constructions.is_empty()
            && self.layouts.is_empty()
    }
}

/// A closed checked dispatch application lacking candidate method coverage. This
/// carries the complete associated-output bindings, not a replacement impl query.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceRequest {
    pub interface: NominalTy,
    pub receiver: Option<Ty>,
    pub method_slot: u32,
    pub arguments: Vec<Ty>,
}

#[cfg(test)]
mod tests;
