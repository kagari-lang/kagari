//! A preselected conversion from a native body's concrete result to its interface.
use crate::types::{
    AbiType, ConcreteFunctionIdentity, GenericParameterAbi, verify::types_in_scope,
};
use kagari_common::identity::DefinitionPath;
use kagari_common::identity::reference::DefinitionReference;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct NativeResultAdapter<I = DefinitionPath> {
    pub receiver: AbiType<I>,
    pub implementation: ConcreteFunctionIdentity<I>,
}

impl NativeResultAdapter {
    pub fn structurally_valid(&self, parameters: &[GenericParameterAbi]) -> bool {
        self.implementation.declaration.within_path_limit()
            && self.implementation.arguments.len() <= 4096
            && types_in_scope(
                self.implementation.arguments.iter().chain([&self.receiver]),
                parameters,
                &Default::default(),
            )
    }
}

mod mapping;
