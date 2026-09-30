//! Select the carried conversion and next methods for one rooted source.
use crate::RuntimeError;
use kagari_abi::{
    native_import::{EngineNativeImport, NativeWitness},
    standard::{surface::StandardEnum, traits::StandardTrait},
    types::AbiType,
};
use kagari_common::identity::associated_type_id;

#[derive(Clone, Copy)]
pub(super) struct SourceSelection {
    pub root: usize,
    iterable: usize,
    next: usize,
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native source selection mismatch")
}
impl SourceSelection {
    pub(super) fn select(
        contract: &EngineNativeImport,
        source: &AbiType,
        iterator: Option<&AbiType>,
        root: usize,
    ) -> Result<Self, RuntimeError> {
        let iterable = contract
            .witnesses
            .iter()
            .position(|witness| {
                witness.receiver == *source
                    && StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::Iterable)
            })
            .ok_or_else(invalid)?;
        let conversion = &contract.witnesses[iterable];
        let item = conversion
            .interface
            .associated_types
            .get(&associated_type_id(
                &conversion.interface.declaration,
                "Item",
            ))
            .ok_or_else(invalid)?;
        let mut choices = contract
            .witnesses
            .iter()
            .enumerate()
            .filter(|(_, witness)| {
                StandardTrait::from_id(&witness.interface.declaration)
                    == Some(StandardTrait::Iterator)
                    && iterator.is_none_or(|iterator| witness.receiver == *iterator)
                    && witness
                        .interface
                        .associated_types
                        .get(&associated_type_id(&witness.interface.declaration, "Item"))
                        == Some(item)
            });
        let next = choices.next().ok_or_else(invalid)?.0;
        if choices.next().is_some() {
            return Err(invalid());
        }
        Ok(Self {
            root,
            iterable,
            next,
        })
    }
    pub(super) fn iterable(self, contract: &EngineNativeImport) -> &NativeWitness {
        &contract.witnesses[self.iterable]
    }
    pub(super) fn next(self, contract: &EngineNativeImport) -> &NativeWitness {
        &contract.witnesses[self.next]
    }
    pub(super) fn optional(self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        let next = self.next(contract);
        let item = next
            .interface
            .associated_types
            .get(&associated_type_id(&next.interface.declaration, "Item"))
            .ok_or_else(invalid)?;
        Ok(AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.clone()],
        })
    }
}
