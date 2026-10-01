//! Portable provider contracts. Keys identify implementations; installation grants authority.
use crate::{
    effects::EffectSet,
    native_import::NativeSignature,
    types::{AbiType, GenericParameterAbi, substitution::TypeSubstitution},
};
use kagari_common::{
    cancellation::CancellationToken, host_interface::HostFunctionDeclaration,
    identity::DefinitionId,
};
use serde::{Deserialize, Serialize};
use std::iter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeBindingKey {
    pub provider: u64,
    pub entry: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeParameterAccess {
    Value,
    Read,
    Write,
}

/// A provider-owned template, independent of a source declaration's binder names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeContract {
    pub key: NativeBindingKey,
    pub version: u32,
    pub binder: DefinitionId,
    pub generic_count: usize,
    pub signature: NativeSignature,
    pub effects: EffectSet,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub parameter_access: Vec<NativeParameterAccess>,
    pub host: Option<HostFunctionDeclaration>,
}

impl NativeContract {
    pub fn apply(&self, arguments: &[AbiType]) -> Option<NativeSignature> {
        if self.signature.params.len() > 4096
            || self.parameter_access.len() != self.signature.params.len()
            || !self
                .signature
                .params
                .iter()
                .chain(iter::once(&self.signature.result))
                .all(AbiType::within_wire_limits)
            || arguments.len() != self.generic_count
            || self.generic_count > 4096
            || self.version == 0
            || !self.binder.within_path_limit()
        {
            return None;
        }
        let substitution = TypeSubstitution::for_owner(&self.binder, arguments);
        let cancel = CancellationToken::default();
        Some(NativeSignature {
            params: self
                .signature
                .params
                .iter()
                .map(|ty| substitution.apply(ty, &cancel))
                .collect::<Result<_, _>>()
                .ok()?,
            result: substitution.apply(&self.signature.result, &cancel).ok()?,
        })
    }

    pub fn matches_signature(
        &self,
        parameters: &[GenericParameterAbi],
        signature: &NativeSignature,
    ) -> bool {
        let arguments = parameters
            .iter()
            .map(|p| AbiType::Parameter {
                owner: p.owner.clone(),
                position: p.position,
            })
            .collect::<Vec<_>>();
        self.apply(&arguments).is_some_and(|expected| {
            expected.result == signature.result
                && expected.params.len() == signature.params.len()
                && expected
                    .params
                    .iter()
                    .zip(&signature.params)
                    .enumerate()
                    .all(|(slot, (declared, actual))| {
                        self.accepts_parameter(slot, declared, actual)
                    })
        })
    }

    pub fn accepts_parameter(&self, slot: usize, declared: &AbiType, actual: &AbiType) -> bool {
        declared == actual
            || (self.parameter_access.get(slot) == Some(&NativeParameterAccess::Read)
                && declared.can_weaken_to(actual))
    }

    pub fn matches_application(&self, arguments: &[AbiType], actual: &NativeSignature) -> bool {
        self.apply(arguments).is_some_and(|expected| {
            expected.result == actual.result
                && expected.params.len() == actual.params.len()
                && expected.params.iter().zip(&actual.params).enumerate().all(
                    |(slot, (declared, actual))| self.accepts_parameter(slot, declared, actual),
                )
        })
    }
}
