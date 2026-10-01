//! Concrete applications; offline validation is structural, runtime installation is trusted.
use crate::{
    effects::EffectSet,
    native_import::linked::matches_declaration,
    provider::{NativeBindingKey, NativeContract, NativeParameterAccess},
    types::{
        AbiType, ConcreteFunctionIdentity, GenericBoundAbi, NativeDeclaration,
        proofs::ProofCatalog, substitution::TypeTransformError, verify::concrete_type_valid,
    },
};
use bincode::serialize;
use kagari_common::cancellation::CancellationToken;
use kagari_common::host_interface::HostFunctionDeclaration;
use serde::{Deserialize, Serialize};
mod linked;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSignature {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub params: Vec<AbiType>,
    pub result: AbiType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeImport {
    pub instance: ConcreteFunctionIdentity,
    pub contract: NativeContract,
    pub signature: NativeSignature,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub requirements: Vec<GenericBoundAbi>,
}

impl NativeImport {
    pub fn from_host(declaration: &HostFunctionDeclaration) -> Self {
        let identity = serialize(&declaration.id).expect("definition identity serialization");
        let entry = identity.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        let signature = NativeSignature {
            params: declaration
                .params
                .iter()
                .map(|p| AbiType::from_host_type(&p.ty))
                .collect(),
            result: AbiType::from_host_type(&declaration.return_type),
        };
        Self {
            instance: ConcreteFunctionIdentity {
                declaration: declaration.id.clone(),
                arguments: vec![],
            },
            contract: NativeContract {
                key: NativeBindingKey {
                    provider: 0x6b6167617269686f,
                    entry,
                },
                version: 1,
                binder: declaration.id.clone(),
                generic_count: 0,
                signature: signature.clone(),
                effects: EffectSet::runtime_call(),
                parameter_access: vec![NativeParameterAccess::Value; signature.params.len()],
                host: Some(declaration.clone()),
            },
            signature,
            requirements: vec![],
        }
    }

    pub fn structurally_valid(&self) -> bool {
        let valid = |ty: &AbiType| {
            ty.within_wire_limits() && concrete_type_valid(ty, &CancellationToken::default())
        };
        let host_valid = self.contract.host.as_ref().is_none_or(|declaration| {
            declaration.validate().is_ok() && *self == Self::from_host(declaration)
        });
        host_valid
            && self.instance.declaration.within_path_limit()
            && self.instance.arguments.len() <= 4096
            && self.signature.params.len() <= 4096
            && self.requirements.len() <= 4096
            && self.instance.arguments.iter().all(valid)
            && self.signature.params.iter().all(valid)
            && valid(&self.signature.result)
            && self
                .contract
                .matches_application(&self.instance.arguments, &self.signature)
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
