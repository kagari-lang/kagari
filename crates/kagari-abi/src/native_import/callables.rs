//! Declared native dependencies and their concrete checked trait-member targets.
use crate::{
    callable::CallableImplementation,
    effects::EffectSet,
    language::Protocol,
    native_import::NativeSignature,
    types::{
        AbiType, ConcreteFunctionIdentity, NominalAbiType,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};

use kagari_common::identity::reference::DefinitionReference;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionPath, associated_type_id},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct NativeCallableRequirement<I = DefinitionPath> {
    pub receiver: AbiType<I>,
    pub interface: NominalAbiType<I>,
    pub member: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType<I>>,
}

impl NativeCallableRequirement {
    pub fn normalized(
        &self,
        catalog: &ProofCatalog<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        let AbiType::Trait(mut interface) =
            catalog.normalize(&AbiType::Trait(self.interface.clone()), cancel)?
        else {
            return Err(TypeTransformError::InvalidContract);
        };
        let receiver = catalog.normalize(&self.receiver, cancel)?;
        if matches!(receiver, AbiType::Trait(_))
            && Protocol::from_id(&interface.declaration).is_some_and(Protocol::iteration)
        {
            for name in ["Item", "Iter"] {
                if name == "Iter"
                    && Protocol::from_id(&interface.declaration) != Some(Protocol::Iterable)
                {
                    continue;
                }
                let member = associated_type_id(&interface.declaration, name);
                if !interface.associated_types.contains_key(&member) {
                    let output = catalog.normalize(
                        &AbiType::Projection {
                            receiver: Box::new(receiver.clone()),
                            interface: Box::new(interface.clone()),
                            member: member.clone(),
                            arguments: vec![],
                        },
                        cancel,
                    )?;
                    interface.associated_types.insert(member, output);
                }
            }
        }
        Ok(Self {
            receiver,
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
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct NativeCallableApplication<I = DefinitionPath> {
    pub origin: NativeCallableOrigin,
    pub requirement: NativeCallableRequirement<I>,
    pub instance: ConcreteFunctionIdentity<I>,
    pub implementation: CallableImplementation<I>,
    pub signature: NativeSignature<I>,
    /// A common conservative callback boundary, checked independently of claims.
    pub effects: EffectSet,
}

mod mapping;
