//! Checked List/Iterable/Iterator applications for native collection sources.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::NativeWitness,
    standard::{StandardIntrinsic, traits::StandardTrait},
};
use kagari_common::identity::associated_type_id;
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};

impl FunctionLowerer<'_, '_> {
    pub(super) fn native_collection_source(
        &mut self,
        binding: EngineNativeBinding,
        params: &[TypeId],
        witnesses: &mut Vec<NativeWitness>,
    ) -> Result<(), MirLoweringError> {
        let index = match binding {
            EngineNativeBinding::Intrinsic(
                StandardIntrinsic::ArrayListFrom
                | StandardIntrinsic::LinkedHashMapFrom
                | StandardIntrinsic::LinkedHashSetFrom,
            ) => 0,
            EngineNativeBinding::Intrinsic(
                StandardIntrinsic::ArrayCopyFrom | StandardIntrinsic::ArrayExtend,
            ) => 1,
            _ => return Ok(()),
        };
        let TypeId::Trait(list) = &params[index] else {
            return Err(MirLoweringError::MissingBinding(
                "native collection source List",
            ));
        };
        let source = &params[index];
        witnesses.push(self.lower_native_witness(source, list, &[])?);
        let mut iterable = StandardTrait::Iterable.nominal();
        for name in ["Item", "Iter"] {
            let output = self.iteration_output(StandardTrait::Iterable, source, name)?;
            iterable
                .associated_types
                .insert(associated_type_id(&iterable.declaration, name), output);
        }
        witnesses.push(self.lower_native_witness(source, &iterable, &[])?);
        let iterator = self.iteration_output(StandardTrait::Iterable, source, "Iter")?;
        let mut next = StandardTrait::Iterator.nominal();
        let item = self.iterator_item(&iterator)?;
        next.associated_types
            .insert(associated_type_id(&next.declaration, "Item"), item);
        witnesses.push(self.lower_native_witness(&iterator, &next, &[])?);
        Ok(())
    }
}
