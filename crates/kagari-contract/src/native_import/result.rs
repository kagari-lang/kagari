//! A preselected conversion from a native body's concrete result to its interface.
use crate::types::ConcreteFunctionIdentity;
use kagari_common::identity::table::DefinitionTable;
use kagari_types::{
    declaration::verify::types_in_scope,
    ty::{GenericParam, Ty},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeResultAdapter {
    pub receiver: Ty,
    pub implementation: ConcreteFunctionIdentity,
}

impl NativeResultAdapter {
    pub fn structurally_valid(
        &self,
        parameters: &[GenericParam],
        definitions: &DefinitionTable,
    ) -> bool {
        definitions.resolve(self.implementation.declaration).is_ok()
            && self.implementation.arguments.len() <= 4096
            && types_in_scope(
                self.implementation.arguments.iter().chain([&self.receiver]),
                parameters,
                &Default::default(),
                definitions,
            )
    }
}

mod mapping;
