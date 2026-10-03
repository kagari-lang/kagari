//! Binder scope of a shared executable body, separate from its concrete owner.
use crate::types::{
    AbiType, ConcreteFunctionIdentity, GenericBoundAbi, GenericParameterAbi,
    verify::{native_bounds_valid, types_in_scope},
};
use kagari_common::identity::DefinitionPath;

use kagari_common::identity::reference::DefinitionReference;
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct GenericBody<I = DefinitionPath> {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub parameters: Vec<GenericParameterAbi<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBoundAbi<I>>,
}

impl GenericBody {
    pub fn valid(&self, identity: &ConcreteFunctionIdentity, cancel: &CancellationToken) -> bool {
        !self.parameters.is_empty()
            && self
                .parameters
                .iter()
                .enumerate()
                .all(|(position, parameter)| {
                    parameter.owner.module == identity.declaration.module
                        && identity.declaration.path.starts_with(&parameter.owner.path)
                        && parameter.owner.path.last().is_some_and(|part| {
                            matches!(part.kind, DefinitionKind::Function | DefinitionKind::Method)
                        })
                        && parameter.position == position
                })
            && native_bounds_valid(&self.bounds, &self.parameters, cancel)
    }

    pub fn types_valid<'a>(
        &self,
        types: impl IntoIterator<Item = &'a AbiType>,
        cancel: &CancellationToken,
    ) -> bool {
        types_in_scope(types, &self.parameters, cancel)
    }
}

mod mapping;
