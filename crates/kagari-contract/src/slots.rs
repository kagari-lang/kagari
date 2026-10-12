use crate::{
    callable::generic::GenericBody, native_import::protocol::adapter_arguments,
    representation::semantic_representation, types::ConcreteFunctionIdentity,
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_types::{
    declaration::{requirement::NativeCallableRequirement, verify::types_in_scope},
    language::{Protocol, adapter::adapter_contract},
    ty::Ty,
};
use std::collections::BTreeMap;

/// Semantic contracts supplement the physical frame layout.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SemanticSlots {
    pub generic: Option<GenericBody>,
    /// Applied checked language protocol implemented by this generated function.
    pub protocol_adapter: Option<NativeCallableRequirement>,
    pub params: BTreeMap<usize, Ty>,
    pub result: Option<Ty>,
    pub locals: BTreeMap<usize, Ty>,
    pub registers: BTreeMap<usize, Ty>,
}

impl SemanticSlots {
    pub fn types_valid(
        &self,
        identity: Option<&ConcreteFunctionIdentity>,
        cancel: &CancellationToken,
        definitions: &DefinitionTable,
    ) -> bool {
        let types = self
            .params
            .values()
            .chain(self.locals.values())
            .chain(self.registers.values())
            .chain(self.result.iter());
        match &self.generic {
            Some(body) => {
                identity.is_some_and(|identity| body.valid(identity, cancel, definitions))
                    && body.types_valid(types, cancel, definitions)
            }
            None => types.into_iter().all(|ty| {
                ty.within_wire_limits() && types_in_scope([ty], &[], cancel, definitions)
            }),
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
        let valid = |representation: ValueType, semantic: Option<&Ty>| {
            let generic_semantic =
                semantic.is_some_and(|ty| semantic_representation(ty) == ValueType::Generic);
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
    pub fn protocol_adapter_valid(
        &self,
        identity: Option<&ConcreteFunctionIdentity>,
        definitions: &DefinitionTable,
    ) -> bool {
        let Some(required) = &self.protocol_adapter else {
            return true;
        };
        let Some(identity) = identity else {
            return false;
        };
        let Some((kind, signature)) = adapter_contract(required, definitions) else {
            return false;
        };
        let (parameters, identity_valid) = match &self.generic {
            Some(body) => (
                body.parameters.as_slice(),
                matches!(kind, Protocol::PartialEq | Protocol::Hash)
                    && required.receiver
                        == Ty::Tuple(
                            body.parameters
                                .iter()
                                .map(|parameter| parameter.as_type())
                                .collect(),
                        )
                    && identity.arguments.is_empty()
                    && definitions.resolve(identity.declaration).is_ok_and(|view| {
                        view.segments().count() == 1
                            && view.last().is_some_and(|part| {
                                part.kind == DefinitionKind::Function
                                    && part.name == format!("$derived_tuple_{}", kind.name())
                                    && part.occurrence as usize == body.parameters.len()
                            })
                    }),
            ),
            None => (
                &[][..],
                identity.arguments == adapter_arguments(required, definitions)
                    && definitions.resolve(identity.declaration).is_ok_and(|view| {
                        view.segments().count() == 1
                            && view.last().is_some_and(|part| {
                                part.kind == DefinitionKind::Function
                                    && part.name == format!("$derived_{}", kind.name())
                                    && part.occurrence == 0
                            })
                    }),
            ),
        };
        definitions.resolve(required.member).is_ok()
            && required.receiver.within_wire_limits()
            && types_in_scope(
                [&required.receiver],
                parameters,
                &Default::default(),
                definitions,
            )
            && identity_valid
            && self.params.len() == signature.params.len()
            && signature
                .params
                .iter()
                .enumerate()
                .all(|(index, ty)| self.params.get(&index) == Some(ty))
            && self.result.as_ref() == Some(&signature.result)
    }
}

mod mapping;

#[doc(hidden)]
pub mod wire;
