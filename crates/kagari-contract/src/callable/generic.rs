//! Binder scope of a shared executable body, separate from its concrete owner.
use crate::types::ConcreteFunctionIdentity;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::verify::{native_bounds_valid, types_in_scope},
    ty::{GenericBound, GenericParam, Ty},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericBody {
    pub parameters: Vec<GenericParam<DefinitionId>>,
    pub bounds: Vec<GenericBound<DefinitionId>>,
}

impl GenericBody {
    pub fn valid(
        &self,
        identity: &ConcreteFunctionIdentity,
        cancel: &CancellationToken,
        definitions: &DefinitionTable,
    ) -> bool {
        !self.parameters.is_empty()
            && self
                .parameters
                .iter()
                .enumerate()
                .all(|(position, parameter)| {
                    definitions.contains(parameter.owner, identity.declaration) == Ok(true)
                        && definitions
                            .resolve(parameter.owner)
                            .ok()
                            .and_then(|view| view.last())
                            .is_some_and(|part| {
                                matches!(
                                    part.kind,
                                    DefinitionKind::Function | DefinitionKind::Method
                                )
                            })
                        && parameter.position == position
                })
            && native_bounds_valid(&self.bounds, &self.parameters, cancel, definitions)
    }

    pub fn types_valid<'a>(
        &self,
        types: impl IntoIterator<Item = &'a Ty<DefinitionId>>,
        cancel: &CancellationToken,
        definitions: &DefinitionTable,
    ) -> bool {
        types_in_scope(types, &self.parameters, cancel, definitions)
    }
}

mod mapping;
