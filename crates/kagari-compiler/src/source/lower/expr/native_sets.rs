//! Selected membership and traversal facts for Set defaults, without algorithm expansion.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::NativeWitness,
    standard::{bindings::NativeDefaultMethod, traits::StandardTrait},
};
use kagari_common::identity::associated_type_id;
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};
impl FunctionLowerer<'_, '_> {
    pub(super) fn set_binding(&self, binding: EngineNativeBinding) -> bool {
        matches!(
            binding,
            EngineNativeBinding::TraitDefault(
                NativeDefaultMethod::SetUnion
                    | NativeDefaultMethod::SetIntersection
                    | NativeDefaultMethod::SetDifference
                    | NativeDefaultMethod::SetSymmetricDifference
                    | NativeDefaultMethod::SetIsSubset
                    | NativeDefaultMethod::SetIsSuperset
                    | NativeDefaultMethod::SetIsDisjoint
            )
        )
    }
    pub(super) fn native_set_sources(
        &mut self,
        binding: EngineNativeBinding,
        params: &[TypeId],
        witnesses: &mut Vec<NativeWitness>,
    ) -> Result<(), MirLoweringError> {
        if !self.set_binding(binding) {
            return Ok(());
        }
        let TypeId::Trait(other) = &params[1] else {
            return Err(MirLoweringError::MissingBinding("checked Set operand"));
        };
        let selected = self.lower_native_witness(&params[1], other, &[])?;
        if !witnesses.contains(&selected) {
            witnesses.push(selected);
        }
        for source in params {
            let mut iterable = StandardTrait::Iterable.nominal();
            for name in ["Item", "Iter"] {
                let output = self.iteration_output(StandardTrait::Iterable, source, name)?;
                iterable
                    .associated_types
                    .insert(associated_type_id(&iterable.declaration, name), output);
            }
            let selected = self.lower_native_witness(source, &iterable, &[])?;
            if !witnesses.contains(&selected) {
                witnesses.push(selected);
            }
            let iterator = self.iteration_output(StandardTrait::Iterable, source, "Iter")?;
            let mut next = StandardTrait::Iterator.nominal();
            let item = self.iterator_item(&iterator)?;
            next.associated_types
                .insert(associated_type_id(&next.declaration, "Item"), item);
            let selected = self.lower_native_witness(&iterator, &next, &[])?;
            if !witnesses.contains(&selected) {
                witnesses.push(selected);
            }
        }
        Ok(())
    }
}
