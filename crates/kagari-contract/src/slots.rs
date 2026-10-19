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
        types_valid(self, identity, cancel, definitions)
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
        protocol_adapter_valid(
            self.protocol_adapter.as_ref(),
            self.generic.as_ref(),
            &self.params,
            self.result.as_ref(),
            identity,
            definitions,
        )
    }
}

/// Validate semantic facts independently of their executable slot numbering.
pub fn types_valid(
    signature: &impl FunctionTypeFacts,
    identity: Option<&ConcreteFunctionIdentity>,
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> bool {
    if signature
        .result()
        .is_some_and(Ty::contains_stored_reference)
        || signature
            .params()
            .values()
            .chain(signature.value_types())
            .chain(signature.result())
            .any(|ty| !ty.reference_position_valid())
    {
        return false;
    }
    let types = signature
        .params()
        .values()
        .chain(signature.value_types())
        .chain(signature.result());
    match signature.generic() {
        Some(body) => {
            identity.is_some_and(|identity| body.valid(identity, cancel, definitions))
                && body.types_valid(types, cancel, definitions)
        }
        None => types
            .into_iter()
            .all(|ty| ty.within_wire_limits() && types_in_scope([ty], &[], cancel, definitions)),
    }
}

/// Check a protocol adapter against its callable facts, without a slot layout.
pub fn protocol_adapter_valid(
    adapter: Option<&NativeCallableRequirement>,
    generic: Option<&GenericBody>,
    params: &BTreeMap<usize, Ty>,
    result: Option<&Ty>,
    identity: Option<&ConcreteFunctionIdentity>,
    definitions: &DefinitionTable,
) -> bool {
    let Some(required) = adapter else {
        return true;
    };
    let Some(identity) = identity else {
        return false;
    };
    let Some((kind, signature)) = adapter_contract(required, definitions) else {
        return false;
    };
    let (parameters, identity_valid) = match &generic {
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
        && params.len() == signature.params.len()
        && signature
            .params
            .iter()
            .enumerate()
            .all(|(index, ty)| params.get(&index) == Some(ty))
        && result == Some(&signature.result)
}

mod mapping;

#[doc(hidden)]
pub mod wire;

/// Borrowed callable/type facts independent of a backend's value numbering.
pub trait FunctionTypeFacts {
    fn generic(&self) -> Option<&GenericBody>;

    fn protocol_adapter(&self) -> Option<&NativeCallableRequirement>;

    fn params(&self) -> &BTreeMap<usize, Ty>;

    fn result(&self) -> Option<&Ty>;

    fn value_types(&self) -> impl Iterator<Item = &Ty>;
}

impl FunctionTypeFacts for SemanticSlots {
    fn generic(&self) -> Option<&GenericBody> {
        self.generic.as_ref()
    }

    fn protocol_adapter(&self) -> Option<&NativeCallableRequirement> {
        self.protocol_adapter.as_ref()
    }

    fn params(&self) -> &BTreeMap<usize, Ty> {
        &self.params
    }

    fn result(&self) -> Option<&Ty> {
        self.result.as_ref()
    }

    fn value_types(&self) -> impl Iterator<Item = &Ty> {
        self.locals.values().chain(self.registers.values())
    }
}
