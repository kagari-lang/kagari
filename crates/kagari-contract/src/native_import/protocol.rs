//! Closed language protocol contracts eligible for generated callable adapters.
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement, language::Protocol, ty::Ty,
};

/// Applied associated outputs are part of a generated iterator adapter's identity.
pub fn adapter_arguments(
    required: &NativeCallableRequirement<DefinitionId>,
    definitions: &DefinitionTable,
) -> Vec<Ty<DefinitionId>> {
    let mut arguments = vec![required.receiver.clone()];
    if Protocol::from_reference(&required.interface.declaration, Some(definitions))
        .is_some_and(|kind| kind.iteration() || matches!(kind, Protocol::Fn | Protocol::From))
    {
        arguments.push(Ty::Trait(required.interface.clone()));
    }
    arguments
}
