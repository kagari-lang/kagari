use crate::{
    callable::CallableSignature,
    host::{HostFunctionId, signature_type},
    native::NativeBinding,
    typeck::FunctionImplementation,
    types::TypeId,
};
use kagari_common::host_interface::HostFunctionDeclaration;

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
/// capabilities, effects, resource charges and every parameter's passing style.
#[derive(Debug, Clone, Copy)]
pub struct HostCallable<'a> {
    pub(super) id: HostFunctionId,
    pub(super) declaration: &'a HostFunctionDeclaration,
    pub(super) signature: &'a HostSignature,
}

impl HostCallable<'_> {
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
