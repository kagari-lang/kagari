//! Checked native applications. Declarations own signatures; IDs select installed entries.
use crate::{
    native_import::linked::matches_declaration,
    types::{
        AbiType, ConcreteFunctionIdentity, GenericBoundAbi, NativeDeclaration,
        proofs::ProofCatalog, substitution::TypeTransformError, verify::concrete_type_valid,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    host_interface::HostFunctionDeclaration,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity},
};
use serde::{Deserialize, Serialize};
mod linked;

/// An installed declaration names an entry within its module's binding namespace.
/// The name does not select compiler/verifier policy or grant registration authority.
pub fn binding_id(module: &ModuleIdentity, name: &str) -> DefinitionId {
    DefinitionId {
        module: module.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Function,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSignature {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub params: Vec<AbiType>,
    pub result: AbiType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeImport {
    pub instance: ConcreteFunctionIdentity,
    pub binding: DefinitionId,
    pub signature: NativeSignature,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub requirements: Vec<GenericBoundAbi>,
    /// Existing host borrow/schema/authority facts belong to the host adapter.
    pub host: Option<HostFunctionDeclaration>,
}

impl NativeImport {
    pub fn from_host(declaration: &HostFunctionDeclaration) -> Self {
        Self {
            instance: ConcreteFunctionIdentity {
                declaration: declaration.id.clone(),
                arguments: vec![],
            },
            binding: declaration.id.clone(),
            signature: NativeSignature {
                params: declaration
                    .params
                    .iter()
                    .map(|p| AbiType::from_host_type(&p.ty))
                    .collect(),
                result: AbiType::from_host_type(&declaration.return_type),
            },
            requirements: vec![],
            host: Some(declaration.clone()),
        }
    }

    pub fn structurally_valid(&self) -> bool {
        let valid = |ty: &AbiType| {
            ty.within_wire_limits() && concrete_type_valid(ty, &CancellationToken::default())
        };
        self.host.as_ref().is_none_or(|declaration| {
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
