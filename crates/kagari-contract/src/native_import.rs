//! Checked native applications. Declarations own signatures; IDs select installed entries.
use crate::{
    callable::{generic::GenericBody, receiver::access_valid, witness::OperationWitness},
    native_import::{linked::matches_declaration, result::NativeResultAdapter},
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    callable::Signature,
    declaration::{
        NativeDeclaration,
        verify::{native_bounds_valid, types_in_scope},
    },
    reference::ReferenceAccess,
    ty::{GenericBound, Ty, substitution::TypeTransformError},
};

pub mod callables;
mod linked;
pub mod protocol;
pub mod result;

/// An installed declaration names an entry within its module's binding namespace.
/// The name does not select compiler/verifier policy or grant registration authority.
pub fn binding_id(module: &ModuleIdentity, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: module.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Function,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeImport {
    /// Checked declaration's required loan; permits a scoped view of this
    /// signature's nominal receiver without changing entry or target identity.
    pub receiver_access: Option<ReferenceAccess>,
    /// Convert a validated Rust body result using this preselected interface table.
    pub result_adapter: Option<NativeResultAdapter>,
    /// A shared native entry retains method binders in its template application.
    pub generic: Option<GenericBody>,
    pub instance: ConcreteFunctionIdentity,
    pub binding: DefinitionId,
    pub signature: Signature,
    pub requirements: Vec<GenericBound>,
    pub callables: Vec<OperationWitness>,
}

impl NativeImport {
    pub fn structurally_valid(&self, definitions: &DefinitionTable) -> bool {
        let parameters = self
            .generic
            .as_ref()
            .map_or(&[][..], |body| body.parameters.as_slice());
        let valid =
            |ty: &Ty| types_in_scope([ty], parameters, &CancellationToken::default(), definitions);
        access_valid(self.receiver_access, &self.signature.params)
            && !self.signature.result.contains_stored_reference()
            && self.signature.result.reference_position_valid()
            && self
                .signature
                .params
                .iter()
                .all(Ty::reference_position_valid)
            && self.generic.as_ref().is_none_or(|body| {
                !body.parameters.is_empty()
                    && body
                        .parameters
                        .iter()
                        .enumerate()
                        .all(|(position, parameter)| {
                            parameter.position == position
                                && definitions.resolve(parameter.owner).is_ok()
                        })
                    && body.bounds == self.requirements
                    && native_bounds_valid(
                        &body.bounds,
                        &body.parameters,
                        &Default::default(),
                        definitions,
                    )
            })
            && definitions.resolve(self.binding).is_ok_and(|view| {
                !view.module().package.0.is_empty()
                    && !view.module().path.iter().any(String::is_empty)
                    && view.last().is_some_and(|part| {
                        !part.name.is_empty()
                            && matches!(
                                part.kind,
                                DefinitionKind::Function | DefinitionKind::Method
                            )
                    })
            })
            && definitions.resolve(self.instance.declaration).is_ok()
            && self.instance.arguments.len() <= 4096
            && self.signature.params.len() <= 4096
            && self.requirements.len() <= 4096
            && self.callables.len() <= 4096
            && self
                .result_adapter
                .as_ref()
                .is_none_or(|adapter| adapter.structurally_valid(parameters, definitions))
            && self
                .callables
                .iter()
                .all(|call| call.structurally_valid(parameters, definitions))
            && self.instance.arguments.iter().all(valid)
            && self.signature.params.iter().all(valid)
            && valid(&self.signature.result)
    }
}

impl NativeImport {
    pub fn matches_declaration(
        &self,
        declaration: &NativeDeclaration,
        catalog: &ProofCatalog<'_>,
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        matches_declaration(self, declaration, catalog, cancel)
    }
}

mod mapping;

#[doc(hidden)]
pub mod wire;
