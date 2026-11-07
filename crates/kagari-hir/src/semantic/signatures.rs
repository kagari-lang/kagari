//! Callable and static contracts tied to one lowered module and semantic environment.

use crate::{
    hir::{
        ids::{FunctionId, ParamId, StaticId},
        writeability::Writeability,
    },
    native::NativeBinding,
    types::{GenericParameterType, NominalType, TypeId},
};
use smallvec::SmallVec;
use std::collections::HashMap;

mod mapping;

/// Checked function records with eight inline slots before heap allocation.
pub(crate) type TypedFunctionBuffer = SmallVec<[TypedFunction; 8]>;

/// Checked parameters with four inline slots before heap allocation.
pub(crate) type TypedParameterBuffer = SmallVec<[TypedParameter; 4]>;

/// A declared callable's semantic signature, separate from its lowered body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedFunction {
    /// The implementation belongs to the declaration, independently of call syntax.
    pub implementation: FunctionImplementation,
    /// Generic binders, including inherited implementation binders where applicable.
    pub generic_params: Vec<GenericParameterType>,
    /// Checked constraints keyed by the declaring parameter, including inherited
    /// impl parameters shadowed by a method parameter with the same name.
    pub bounds: HashMap<TypeId, Vec<NominalType>>,
    /// Local function slot in the matching lowered module.
    pub id: FunctionId,
    /// Diagnostic name of the function/method.
    pub name: String,
    /// Named typed parameters in call order.
    pub params: TypedParameterBuffer,
    /// Declared/inferred result; recovery can retain an error type.
    pub return_type: TypeId,
}

/// Implementation provenance carried with checked callable signatures.
/// The enclosing module/function identity selects a script body; required trait
/// methods await an implementation. Native bindings are installed input and do
/// not acquire authority from the declaration's name or source URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionImplementation {
    /// A source body selected through the enclosing function/module identity.
    Script,
    /// An installed native binding carrying its registered contract.
    Native(NativeBinding),
    /// A trait requirement awaiting an implementation, with no script body.
    Required,
}

/// A function parameter binding paired with its semantic type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedParameter {
    /// Source parameter identity belonging to the same lowering.
    pub id: ParamId,
    /// Declared parameter binding writeability.
    pub writeability: Writeability,
    /// Diagnostic parameter name.
    pub name: String,
    /// Resolved semantic type, possibly recovered after an error.
    pub ty: TypeId,
}

/// Immutable callable and static contracts normalized in one visible environment.
///
/// The signature stage resolves written types; body preparation then normalizes
/// those types against the complete aggregate catalog. Constants, selected bodies
/// and the full module result share this same owner through `Arc`. The local
/// function index contains positions, never another copy of each signature.
///
/// For `fn read(x: Item) -> i32 { x.value }`, the function record owns the parameter
/// and result contract. The body result owns the binding, field selection and
/// expression type of `x.value`. A body edit does not make old local IDs valid in
/// the new lowering: retain these contracts with their matching module snapshot.
/// Recovery may retain error types; this record alone does not authorize compilation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodySignatures {
    functions: TypedFunctionBuffer,
    positions: HashMap<FunctionId, usize>,
    statics: HashMap<StaticId, TypeId>,
}

impl BodySignatures {
    pub(crate) fn new(functions: TypedFunctionBuffer, statics: HashMap<StaticId, TypeId>) -> Self {
        let positions = functions
            .iter()
            .enumerate()
            .map(|(index, function)| (function.id, index))
            .collect();
        Self {
            functions,
            positions,
            statics,
        }
    }

    /// Borrows callable contracts in prepared declaration order.
    pub fn functions(&self) -> &[TypedFunction] {
        &self.functions
    }

    /// Finds a local function in the matching module's declaration table.
    ///
    /// Returns `None` when this module has no such slot. `FunctionId` is module-local;
    /// callers must retain the owning module rather than mixing numeric slots.
    pub fn function(&self, id: FunctionId) -> Option<&TypedFunction> {
        self.positions.get(&id).map(|index| &self.functions[*index])
    }

    /// Borrows the normalized type of a runtime static in this module.
    pub fn static_type(&self, id: StaticId) -> Option<&TypeId> {
        self.statics.get(&id)
    }

    /// Borrows all normalized runtime static types, keyed by local declaration ID.
    pub fn statics(&self) -> &HashMap<StaticId, TypeId> {
        &self.statics
    }
}
