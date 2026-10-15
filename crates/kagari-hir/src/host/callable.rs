//! Borrowed host callable signatures consumed by the same call checker as source functions.

use crate::{
    callable::CallableSignature,
    host::{HostFunctionId, signature_type},
    native::NativeBinding,
    typeck::FunctionImplementation,
    types::TypeId,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        scoped::DefinitionReferences,
        table::DefinitionId,
    },
};

use kagari_types::host_interface::HostFunctionDeclaration;

/// Semantic types are imported once when the immutable host interface is installed.
#[derive(Debug)]
pub(super) struct HostSignature {
    params: Vec<TypeId>,
    result: TypeId,
}

impl HostSignature {
    pub(super) fn new(declaration: &HostFunctionDeclaration) -> Self {
        Self {
            params: declaration
                .params
                .iter()
                .map(|parameter| signature_type(&parameter.ty))
                .collect(),
            result: signature_type(&declaration.return_type),
        }
    }
}

/// One checked signature and its original provider contract. The contract retains
/// effects and every parameter's declared type and passing style.
#[derive(Debug, Clone, Copy)]
pub struct HostCallable<'a> {
    pub(super) id: HostFunctionId,
    pub(super) declaration: &'a HostFunctionDeclaration,
    pub(super) signature: &'a HostSignature,
}

impl HostCallable<'_> {
    /// Borrows the original installed parameter/result/effect contract.
    pub fn contract(&self) -> &HostFunctionDeclaration {
        self.declaration
    }
}

impl CallableSignature for HostCallable<'_> {
    fn name(&self) -> &str {
        &self.declaration.symbol
    }

    fn implementation(&self) -> FunctionImplementation {
        FunctionImplementation::Native(NativeBinding::Host(self.id))
    }

    fn parameters(&self) -> impl ExactSizeIterator<Item = (&str, &TypeId)> {
        self.declaration
            .params
            .iter()
            .zip(&self.signature.params)
            .map(|(parameter, ty)| (parameter.name.as_str(), ty))
    }

    fn return_type(&self) -> &TypeId {
        &self.signature.result
    }
}

impl DefinitionReferences for HostSignature {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for parameter in &self.params {
            parameter.visit_definition_ids(visit, cancel)?;
        }
        self.result.visit_definition_ids(visit, cancel)
    }
}
