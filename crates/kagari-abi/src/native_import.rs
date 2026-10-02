//! Checked native applications. Declarations own signatures; IDs select installed entries.
use crate::{
    callable::CallableImplementation,
    effects::EffectSet,
    native_import::{callables::NativeCallableOrigin, linked::matches_declaration},
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
pub mod callables;
mod linked;
pub mod protocol;

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
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub callables: Vec<callables::NativeCallableApplication>,
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
            callables: vec![],
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
            && self.callables.len() <= 4096
            && self.callables.iter().all(|call| {
                call.effects == EffectSet::native_call()
                    && (call.origin != NativeCallableOrigin::ProtocolAdapter
                        || call.implementation == CallableImplementation::Script)
                    && call.instance.declaration.within_path_limit()
                    && call.instance.arguments.len() <= 4096
                    && call.instance.arguments.iter().all(valid)
                    && call.requirement.member.within_path_limit()
                    && call.requirement.arguments.len() <= 4096
                    && call.requirement.arguments.iter().all(valid)
                    && valid(&call.requirement.receiver)
                    && valid(&AbiType::Trait(call.requirement.interface.clone()))
                    && call.signature.params.len() <= 4096
                    && call.signature.params.iter().all(valid)
                    && valid(&call.signature.result)
                    && match &call.implementation {
                        CallableImplementation::Script => true,
                        CallableImplementation::Native(binding) => binding.within_path_limit(),
                        CallableImplementation::Required
                        | CallableImplementation::NativeDefault(_) => false,
                    }
            })
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
