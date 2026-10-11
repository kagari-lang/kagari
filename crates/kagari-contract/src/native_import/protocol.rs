//! Closed language protocol contracts eligible for generated callable adapters.
use kagari_common::identity::table::DefinitionTable;
use kagari_types::{
    declaration::requirement::NativeCallableRequirement, language::Protocol, ty::Ty,
};

/// Applied associated outputs are part of a generated iterator adapter's identity.
pub fn adapter_arguments(
    required: &NativeCallableRequirement,
    definitions: &DefinitionTable,
) -> Vec<Ty> {
    let mut arguments = vec![required.receiver.clone()];
    if Protocol::from_definition(&required.interface.declaration, definitions)
        .is_some_and(|kind| kind.iteration() || matches!(kind, Protocol::Fn | Protocol::From))
    {
        arguments.push(Ty::Trait(required.interface.clone()));
    }
    arguments
}
