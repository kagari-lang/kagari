//! Closed language protocol contracts eligible for generated callable adapters.
use kagari_common::identity::table::DefinitionTable;
use kagari_types::{
    declaration::requirement::NativeCallableRequirement, language::Protocol, ty::Ty,
};

/// Preserve applied interfaces when their arguments or associated bindings select
/// the generated adapter's exact checked requirement.
pub fn adapter_arguments(
    required: &NativeCallableRequirement,
    definitions: &DefinitionTable,
) -> Vec<Ty> {
    let mut arguments = vec![required.receiver.clone()];
    if Protocol::from_definition(&required.interface.declaration, definitions).is_some_and(|kind| {
        kind.iteration()
            || matches!(
                kind,
                Protocol::Fn
                    | Protocol::From
                    | Protocol::Neg
                    | Protocol::Add
                    | Protocol::Sub
                    | Protocol::Mul
                    | Protocol::Div
                    | Protocol::Rem
            )
    }) {
        arguments.push(Ty::Trait(required.interface.clone()));
    }
    arguments
}
