use crate::{
    callable::generic::GenericBody,
    native_import::{
        callables::NativeCallableRequirement,
        protocol::{adapter_arguments, adapter_contract},
    },
    representation::ValueType,
    types::{AbiType, ConcreteFunctionIdentity, verify::concrete_type_valid},
};
use kagari_common::identity::DefinitionPath;

use kagari_common::identity::reference::DefinitionReference;
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use std::collections::BTreeMap;

/// Semantic contracts supplement the physical frame layout.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct SemanticSlots<I = DefinitionPath> {
    pub generic: Option<GenericBody<I>>,
    /// Applied checked language protocol implemented by this generated function.
    pub protocol_adapter: Option<NativeCallableRequirement<I>>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub params: BTreeMap<usize, AbiType<I>>,
    pub result: Option<AbiType<I>>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub locals: BTreeMap<usize, AbiType<I>>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub registers: BTreeMap<usize, AbiType<I>>,
}

impl SemanticSlots {
    pub fn types_valid(
        &self,
        identity: Option<&ConcreteFunctionIdentity>,
        cancel: &CancellationToken,
    ) -> bool {
        let types = self
            .params
            .values()
            .chain(self.locals.values())
            .chain(self.registers.values())
            .chain(self.result.iter());
        match &self.generic {
            Some(body) => {
                identity.is_some_and(|identity| body.valid(identity, cancel))
                    && body.types_valid(types, cancel)
            }
            None => types
                .into_iter()
                .all(|ty| ty.within_wire_limits() && concrete_type_valid(ty, cancel)),
        }
    }

    /// Generic physical slots always have an explicit scoped semantic type.
    /// Missing metadata must not turn the tagged representation into unchecked Any.
    pub fn generic_layout_valid(
        &self,
        params: impl IntoIterator<Item = ValueType>,
        locals: impl IntoIterator<Item = ValueType>,
        registers: impl IntoIterator<Item = ValueType>,
        result: ValueType,
    ) -> bool {
        let valid = |representation: ValueType, semantic: Option<&AbiType>| {
            let generic_semantic =
                semantic.is_some_and(|ty| ty.representation() == ValueType::Generic);
            if representation == ValueType::Generic || generic_semantic {
                self.generic.is_some() && generic_semantic && representation == ValueType::Generic
            } else {
                true
            }
        };
        params
            .into_iter()
            .enumerate()
            .all(|(index, ty)| valid(ty, self.params.get(&index)))
            && locals
                .into_iter()
                .enumerate()
                .all(|(index, ty)| valid(ty, self.locals.get(&index)))
            && registers
                .into_iter()
                .enumerate()
                .all(|(index, ty)| valid(ty, self.registers.get(&index)))
            && valid(result, self.result.as_ref())
    }

    /// Local shape checks precede dependency-dependent protocol eligibility.
    pub fn protocol_adapter_valid(&self, identity: Option<&ConcreteFunctionIdentity>) -> bool {
        let Some(required) = &self.protocol_adapter else {
            return true;
        };
        let Some(identity) = identity else {
            return false;
        };
        let Some((kind, signature)) = adapter_contract(required) else {
            return false;
        };
        required.member.within_path_limit()
            && required.receiver.within_wire_limits()
            && concrete_type_valid(&required.receiver, &Default::default())
            && identity.arguments == adapter_arguments(required)
            && identity.declaration.path.len() == 1
            && identity.declaration.path[0].kind == DefinitionKind::Function
            && identity.declaration.path[0].name == format!("$derived_{}", kind.name())
            && identity.declaration.path[0].occurrence == 0
            && self.params.len() == signature.params.len()
            && signature
                .params
                .iter()
                .enumerate()
                .all(|(index, ty)| self.params.get(&index) == Some(ty))
            && self.result.as_ref() == Some(&signature.result)
    }
}

impl<I> Default for SemanticSlots<I> {
    fn default() -> Self {
        Self {
            generic: Default::default(),
            protocol_adapter: Default::default(),
            params: Default::default(),
            result: Default::default(),
            locals: Default::default(),
            registers: Default::default(),
        }
    }
}

mod mapping;
