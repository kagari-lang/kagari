//! Checked native applications. Declarations own signatures; IDs select installed entries.
use crate::{
    callable::{generic::GenericBody, witness::OperationWitness},
    native_import::{linked::matches_declaration, result::NativeResultAdapter},
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
        reference::DefinitionReference,
    },
};
use kagari_types::{
    callable::Signature,
    declaration::{
        NativeDeclaration,
        verify::{native_bounds_valid, types_in_scope},
    },
    host_interface::HostFunctionDeclaration,
    ty::{GenericBound, Ty, substitution::TypeTransformError},
};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct NativeImport<I = DefinitionPath> {
    /// Convert a validated Rust body result using this preselected interface table.
    pub result_adapter: Option<NativeResultAdapter<I>>,
    /// A shared native entry retains method binders in its template application.
    pub generic: Option<GenericBody<I>>,
    pub instance: ConcreteFunctionIdentity<I>,
    pub binding: I,
    pub signature: Signature<I>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub requirements: Vec<GenericBound<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub callables: Vec<OperationWitness<I>>,
    /// Existing host borrow/schema/authority facts belong to the host adapter.
    pub host: Option<HostFunctionDeclaration<I>>,
}

impl NativeImport {
    pub fn from_host(declaration: &HostFunctionDeclaration) -> Self {
        Self {
            result_adapter: None,
            generic: None,
            instance: ConcreteFunctionIdentity {
                declaration: declaration.id.clone(),
                arguments: vec![],
            },
            binding: declaration.id.clone(),
            signature: Signature {
                params: declaration
                    .params
                    .iter()
                    .map(|p| Ty::from_host_type(&p.ty))
                    .collect(),
                result: Ty::from_host_type(&declaration.return_type),
            },
            requirements: vec![],
            callables: vec![],
            host: Some(declaration.clone()),
        }
    }

    pub fn structurally_valid(&self) -> bool {
        let parameters = self
            .generic
            .as_ref()
            .map_or(&[][..], |body| body.parameters.as_slice());
        let valid = |ty: &Ty| types_in_scope([ty], parameters, &CancellationToken::default());
        self.generic.as_ref().is_none_or(|body| {
            !body.parameters.is_empty()
                && body
                    .parameters
                    .iter()
                    .enumerate()
                    .all(|(position, parameter)| {
                        parameter.position == position && parameter.owner.within_path_limit()
                    })
                && body.bounds == self.requirements
                && native_bounds_valid(&body.bounds, &body.parameters, &Default::default())
        }) && self.host.as_ref().is_none_or(|declaration| {
            declaration.validate().is_ok() && *self == Self::from_host(declaration)
        }) && self.binding.within_path_limit()
            && !self.binding.module.package.0.is_empty()
            && !self.binding.module.path.is_empty()
            && !self.binding.module.path.iter().any(String::is_empty)
            && self.binding.path.last().is_some_and(|part| {
                !part.name.is_empty()
                    && matches!(part.kind, DefinitionKind::Function | DefinitionKind::Method)
            })
            && self.instance.declaration.within_path_limit()
            && self.instance.arguments.len() <= 4096
            && self.signature.params.len() <= 4096
            && self.requirements.len() <= 4096
            && self.callables.len() <= 4096
            && self
                .result_adapter
                .as_ref()
                .is_none_or(|adapter| adapter.structurally_valid(parameters))
            && self
                .callables
                .iter()
                .all(|call| call.structurally_valid(parameters))
            && self.instance.arguments.iter().all(valid)
            && self.signature.params.iter().all(valid)
            && valid(&self.signature.result)
    }

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
