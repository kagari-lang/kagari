//! Declared native dependencies and their concrete checked trait-member targets.
use crate::{
    callable::CallableImplementation,
    effects::EffectSet,
    native_import::NativeSignature,
    types::{
        AbiType, ConcreteFunctionIdentity, NominalAbiType,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCallableRequirement {
    pub receiver: AbiType,
    pub interface: NominalAbiType,
    pub member: DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType>,
}
impl NativeCallableRequirement {
    pub fn normalized(
        &self,
        catalog: &ProofCatalog<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        let AbiType::Trait(interface) =
            catalog.normalize(&AbiType::Trait(self.interface.clone()), cancel)?
        else {
            return Err(TypeTransformError::InvalidContract);
        };
        Ok(Self {
            receiver: catalog.normalize(&self.receiver, cancel)?,
            interface,
            member: self.member.clone(),
            arguments: self
                .arguments
                .iter()
                .map(|ty| catalog.normalize(ty, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
    pub fn apply(
        &self,
        substitution: &TypeSubstitution<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        Ok(Self {
            receiver: substitution.apply(&self.receiver, cancel)?,
            interface: substitution.apply_nominal(&self.interface, cancel)?,
            member: self.member.clone(),
            arguments: self
                .arguments
                .iter()
                .map(|ty| substitution.apply(ty, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeCallableOrigin {
    Implementation,
    /// A checked language protocol is materialized as an ordinary function.
    ProtocolAdapter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCallableApplication {
    pub origin: NativeCallableOrigin,
    pub requirement: NativeCallableRequirement,
    pub instance: ConcreteFunctionIdentity,
    pub implementation: CallableImplementation,
    pub signature: NativeSignature,
    /// A common conservative callback boundary, checked independently of claims.
    pub effects: EffectSet,
}
