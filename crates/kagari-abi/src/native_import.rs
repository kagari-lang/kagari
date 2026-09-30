//! Concrete native applications lowered from checked callable facts. No source
//! names or parameter catalogs participate in executable validation.
use crate::{
    callable::EngineNativeBinding,
    effects::{EffectSet, standard_intrinsic_effects},
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

/// A trusted linked engine entry. Resumable implementations are driven by the
/// execution session instead of recursively invoking script from a Rust helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineNativeOperation {
    Direct(StandardIntrinsic),
    Resumable(EngineNativeBinding),
}

impl EngineNativeOperation {
    pub fn effects(self) -> EffectSet {
        match self {
            Self::Direct(operation) => standard_intrinsic_effects(operation),
            Self::Resumable(_) => EffectSet {
                allocates: true,
                ..EffectSet::runtime_call()
            },
        }
    }
}

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
    /// Resolve a concrete operation whose consumed storage and result shape
    /// agree with the carried application. Effects and work charges come from
    /// this operation's trusted implementation, never from artifact assertions.
    pub fn resolve(&self) -> Option<EngineNativeOperation> {
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
        if let Some(operation) = validate(self.binding, &self.signature) {
            return Some(EngineNativeOperation::Direct(operation));
        }
        if matches!(
            self.binding,
            EngineNativeBinding::Intrinsic(
                StandardIntrinsic::OptionUnwrapOrElse
                    | StandardIntrinsic::OptionOrElse
                    | StandardIntrinsic::OptionMapOr
                    | StandardIntrinsic::OptionMapOrElse
                    | StandardIntrinsic::OptionFilter
                    | StandardIntrinsic::OptionIsSomeAnd
                    | StandardIntrinsic::OptionZip
                    | StandardIntrinsic::OptionFlatten
                    | StandardIntrinsic::OptionTranspose
                    | StandardIntrinsic::ResultUnwrapOrElse
                    | StandardIntrinsic::ResultOrElse
                    | StandardIntrinsic::ResultMapOr
                    | StandardIntrinsic::ResultMapOrElse
                    | StandardIntrinsic::ResultOk
                    | StandardIntrinsic::ResultErr
                    | StandardIntrinsic::ResultIsOkAnd
                    | StandardIntrinsic::ResultIsErrAnd
                    | StandardIntrinsic::ResultFlatten
                    | StandardIntrinsic::ResultTranspose
                    | StandardIntrinsic::OptionMap
                    | StandardIntrinsic::OptionAndThen
                    | StandardIntrinsic::OptionOkOr
                    | StandardIntrinsic::OptionOkOrElse
                    | StandardIntrinsic::ResultMap
                    | StandardIntrinsic::ResultMapErr
                    | StandardIntrinsic::ResultAndThen
            )
        ) && contract::binding_signature_valid(self.binding, &self.signature, &self.requirements)
        {
            return Some(EngineNativeOperation::Resumable(self.binding));
        }
        None
    }
}
