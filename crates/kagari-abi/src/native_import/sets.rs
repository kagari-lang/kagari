//! Set defaults consume checked membership policies and two guarded traversals.
use crate::{
    callable::EngineNativeBinding,
    native_import::EngineNativeImport,
    standard::{bindings::NativeDefaultMethod, intrinsic, traits::StandardTrait},
    types::{
        AbiType, ConstraintAbi, GenericBoundAbi, proofs::ProofCatalog,
        substitution::TypeTransformError,
    },
};
use kagari_common::{cancellation::CancellationToken, identity::associated_type_id};
pub(super) fn selected(binding: EngineNativeBinding) -> bool {
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
pub(super) fn algebra(binding: EngineNativeBinding) -> bool {
    matches!(
        binding,
        EngineNativeBinding::TraitDefault(
            NativeDefaultMethod::SetUnion
                | NativeDefaultMethod::SetIntersection
                | NativeDefaultMethod::SetDifference
                | NativeDefaultMethod::SetSymmetricDifference
        )
    )
}
pub(super) fn obligations(
    import: &EngineNativeImport,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<Option<Vec<GenericBoundAbi>>, TypeTransformError> {
    if !selected(import.binding) {
        return Ok(Some(vec![]));
    }
    let [left, right] = import.signature.params.as_slice() else {
        return Ok(None);
    };
    let Some(source) = import.witnesses.iter().find(|w| {
        w.receiver == *left
            && StandardTrait::from_id(&w.interface.declaration) == Some(StandardTrait::Set)
    }) else {
        return Ok(None);
    };
    let AbiType::Trait(other) = right else {
        return Ok(None);
    };
    if source.interface != *other || source.interface.arguments.len() != 1 {
        return Ok(None);
    }
    let item = &source.interface.arguments[0];
    let mut obligations = vec![GenericBoundAbi {
        ty: right.clone(),
        constraints: vec![ConstraintAbi::Trait(other.clone())],
    }];
    for receiver in [left, right] {
        let Some(iterable) = catalog
            .ancestry(&source.interface, receiver, cancel)?
            .into_iter()
            .find(|i| StandardTrait::from_id(&i.declaration) == Some(StandardTrait::Iterable))
        else {
            return Ok(None);
        };
        let iterator = catalog.normalize(
            &AbiType::Projection {
                receiver: Box::new(receiver.clone()),
                interface: Box::new(iterable.clone()),
                member: associated_type_id(&iterable.declaration, "Iter"),
                arguments: vec![],
            },
            cancel,
        )?;
        if iterator != AbiType::Iter(Box::new(item.clone())) {
            return Ok(None);
        }
        let mut next = intrinsic::applied(StandardTrait::Iterator, vec![]);
        next.associated_types
            .insert(associated_type_id(&next.declaration, "Item"), item.clone());
        obligations.push(GenericBoundAbi {
            ty: receiver.clone(),
            constraints: vec![ConstraintAbi::Trait(iterable)],
        });
        obligations.push(GenericBoundAbi {
            ty: iterator,
            constraints: vec![ConstraintAbi::Trait(next)],
        });
    }
    Ok(Some(obligations))
}
