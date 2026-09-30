//! Concrete native applications lowered from checked callable facts. No source
//! names or parameter catalogs participate in executable validation.
use crate::{
    callable::EngineNativeBinding,
    native_import::signature::validate,
    standard::StandardIntrinsic,
    types::{
        AbiType, ConcreteFunctionIdentity, ConstraintAbi, GenericBoundAbi, NominalAbiType,
        substitution::MAX_TYPE_NODES, verify::concrete_type_valid,
    },
};
use kagari_common::identity::DefinitionId;
use serde::{Deserialize, Serialize};

pub mod contract;
mod linked;
mod signature;

pub const ENGINE_NATIVE_BINDING_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSignature {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub params: Vec<AbiType>,
    pub result: AbiType,
}

/// A selected protocol implementation, including its instantiated owner. Primitive
/// language protocols have a closed engine implementation; tables and host
/// contracts are resolved within the linked dependency closure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeWitnessImplementation {
    Primitive,
    Table(ConcreteFunctionIdentity),
    Host,
    Interface,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeWitness {
    pub receiver: AbiType,
    pub interface: NominalAbiType,
    pub implementation: NativeWitnessImplementation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineNativeImport {
    pub instance: ConcreteFunctionIdentity,
    pub binding: EngineNativeBinding,
    pub binding_version: u32,
    pub signature: NativeSignature,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub requirements: Vec<GenericBoundAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub witnesses: Vec<NativeWitness>,
}

impl EngineNativeImport {
    /// Resolve only a concrete operation whose consumed storage and result shape
    /// agree with the carried application. Effects and work charges come from
    /// this operation's trusted implementation, never from artifact assertions.
    pub fn direct_operation(&self) -> Option<StandardIntrinsic> {
        let valid =
            |ty: &AbiType| ty.within_wire_limits() && concrete_type_valid(ty, &Default::default());
        let nominal = |ty: &NominalAbiType| {
            ty.declaration.within_path_limit()
                && ty
                    .arguments
                    .iter()
                    .chain(ty.associated_types.values())
                    .all(&valid)
                && ty
                    .associated_types
                    .keys()
                    .all(DefinitionId::within_path_limit)
        };
        if self.signature.params.len() > MAX_TYPE_NODES
            || self.instance.arguments.len() > MAX_TYPE_NODES
            || self.requirements.len() > MAX_TYPE_NODES
            || self.witnesses.len() > MAX_TYPE_NODES
            || self.requirements.iter().any(|bound| {
                !valid(&bound.ty)
                    || bound.constraints.len() > MAX_TYPE_NODES
                    || bound.constraints.iter().any(|constraint| match constraint {
                        ConstraintAbi::Standard(_) => false,
                        ConstraintAbi::Trait(interface) => !nominal(interface),
                    })
            })
            || self.witnesses.iter().any(|witness| {
                !valid(&witness.receiver)
                    || !nominal(&witness.interface)
                    || match &witness.implementation {
                        NativeWitnessImplementation::Table(instance) => {
                            !instance.declaration.within_path_limit()
                                || instance.arguments.len() > MAX_TYPE_NODES
                                || !instance.arguments.iter().all(&valid)
                        }
                        _ => false,
                    }
            })
            || self.binding_version != ENGINE_NATIVE_BINDING_VERSION
            || !self.instance.declaration.within_path_limit()
            || !self.instance.arguments.iter().all(&valid)
            || !self.signature.params.iter().all(&valid)
            || !valid(&self.signature.result)
        {
            return None;
        }
        validate(self.binding, &self.signature)
    }
}
