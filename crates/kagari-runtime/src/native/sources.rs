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
    next: IteratorSelection,
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
            .enumerate()
            .filter(|(_, witness)| {
                witness.receiver == *source
                    && StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::Iterable)
                    && witness
                        .interface
                        .associated_types
                        .contains_key(&associated_type_id(&witness.interface.declaration, "Item"))
            })
            .min_by_key(|(_, witness)| {
                !witness
                    .interface
                    .associated_types
                    .contains_key(&associated_type_id(&witness.interface.declaration, "Iter"))
            })
            .map(|(index, _)| index)
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
        let iterator = iterator.or_else(|| {
            conversion
                .interface
                .associated_types
                .get(&associated_type_id(
                    &conversion.interface.declaration,
                    "Iter",
                ))
        });
        let next = IteratorSelection::matching(contract, iterator, item)?;
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
        self.next.witness(contract)
    }
    pub(super) fn optional(self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        self.next.optional(contract)
    }
}

/// Select next independently of source conversion for public Iterator terminals.
#[derive(Clone, Copy)]
pub(super) struct IteratorSelection {
    index: usize,
}
impl IteratorSelection {
    pub(super) fn select(
        contract: &EngineNativeImport,
        receiver: &AbiType,
    ) -> Result<Self, RuntimeError> {
        let mut choices = contract
            .witnesses
            .iter()
            .enumerate()
            .filter(|(_, witness)| {
                witness.receiver == *receiver
                    && StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::Iterator)
            });
        let index = choices.next().ok_or_else(invalid)?.0;
        if choices.next().is_some() {
            return Err(invalid());
        }
        Ok(Self { index })
    }
    fn matching(
        contract: &EngineNativeImport,
        receiver: Option<&AbiType>,
        item: &AbiType,
    ) -> Result<Self, RuntimeError> {
        let mut choices = contract
            .witnesses
            .iter()
            .enumerate()
            .filter(|(_, witness)| {
                StandardTrait::from_id(&witness.interface.declaration)
                    == Some(StandardTrait::Iterator)
                    && receiver.is_none_or(|receiver| witness.receiver == *receiver)
                    && witness
                        .interface
                        .associated_types
                        .get(&associated_type_id(&witness.interface.declaration, "Item"))
                        == Some(item)
            });
        let index = choices.next().ok_or_else(invalid)?.0;
        if choices.next().is_some() {
            return Err(invalid());
        }
        Ok(Self { index })
    }
    pub(super) fn witness(self, contract: &EngineNativeImport) -> &NativeWitness {
        &contract.witnesses[self.index]
    }
    pub(super) fn item(self, contract: &EngineNativeImport) -> Result<&AbiType, RuntimeError> {
        let witness = self.witness(contract);
        witness
            .interface
            .associated_types
            .get(&associated_type_id(&witness.interface.declaration, "Item"))
            .ok_or_else(invalid)
    }
    pub(super) fn optional(self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        Ok(AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![self.item(contract)?.clone()],
        })
    }
}
