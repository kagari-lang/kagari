//! A preselected conversion from a native body's concrete result to its interface.
use crate::types::ConcreteFunctionIdentity;
use kagari_common::identity::DefinitionPath;
use kagari_common::identity::{
    reference::DefinitionReference,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::verify::types_in_scope_in,
    ty::{GenericParam, Ty},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct NativeResultAdapter<I = DefinitionPath> {
    pub receiver: Ty<I>,
    pub implementation: ConcreteFunctionIdentity<I>,
}

impl NativeResultAdapter<DefinitionId> {
    pub fn structurally_valid(
        &self,
        parameters: &[GenericParam<DefinitionId>],
        definitions: &DefinitionTable,
    ) -> bool {
        definitions.resolve(self.implementation.declaration).is_ok()
            && self.implementation.arguments.len() <= 4096
            && types_in_scope_in(
                self.implementation.arguments.iter().chain([&self.receiver]),
                parameters,
                &Default::default(),
                Some(definitions),
            )
    }
}

mod mapping;
