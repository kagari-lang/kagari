//! Carry checked traversal facts for runtime-owned lazy iterators.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::{
    native_import::NativeWitness,
    standard::{bindings::NativeDefaultMethod, traits::StandardTrait},
};
use kagari_common::identity::associated_type_id;
use kagari_hir::{builtin::traits::StandardTraitSemantics, types::TypeId};

impl FunctionLowerer<'_, '_> {
    pub(super) fn native_lazy_witnesses(
        &mut self,
        operation: NativeDefaultMethod,
        params: &[TypeId],
        result: &TypeId,
        witnesses: &mut Vec<NativeWitness>,
    ) -> Result<(), MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("checked lazy iterator traversal");
        let source = match operation {
            NativeDefaultMethod::Zip | NativeDefaultMethod::Chain => Some(params[1].clone()),
            NativeDefaultMethod::FlatMap => {
                let TypeId::Function { result, .. } = &params[1] else {
                    return Err(invalid());
                };
                Some(result.as_ref().clone())
            }
            NativeDefaultMethod::Flatten => Some(self.iterator_item(&params[0])?),
            NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks => {
                Some(params[0].clone())
            }
            _ => None,
        };
        if let Some(source) = source {
            let item = self.iteration_output(StandardTrait::Iterable, &source, "Item")?;
            let iterator = self.iteration_output(StandardTrait::Iterable, &source, "Iter")?;
            let mut iterable = StandardTrait::Iterable.nominal();
            iterable.associated_types.insert(
                associated_type_id(&iterable.declaration, "Item"),
                item.clone(),
            );
            iterable.associated_types.insert(
                associated_type_id(&iterable.declaration, "Iter"),
                iterator.clone(),
            );
            let mut next = StandardTrait::Iterator.nominal();
            next.associated_types
                .insert(associated_type_id(&next.declaration, "Item"), item);
            for witness in [
                self.lower_native_witness(&source, &iterable, &[])?,
                self.lower_native_witness(&iterator, &next, &[])?,
            ] {
                if !witnesses.contains(&witness) {
                    witnesses.push(witness);
                }
            }
        }
        if matches!(
            operation,
            NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks
        ) {
            let TypeId::Iter(item) = result else {
                return Err(invalid());
            };
            witnesses.push(self.native_list_result(item)?);
        }
        Ok(())
    }
}
