//! Selected source traversal for array construction and atomic snapshot copying.
use crate::{
    callable::EngineNativeBinding,
    native_import::EngineNativeImport,
    standard::{
        StandardIntrinsic, bindings::NativeProtocolMethod, intrinsic, traits::StandardTrait,
    },
    types::{
        AbiType, ConstraintAbi, GenericBoundAbi, proofs::ProofCatalog,
        substitution::TypeTransformError,
    },
};
use kagari_common::{
    cancellation::CancellationToken, collection::CollectionAccess, identity::associated_type_id,
};

pub(super) fn selected(import: &EngineNativeImport) -> bool {
    matches!(
        import.binding,
        EngineNativeBinding::Intrinsic(
            StandardIntrinsic::ArrayListFrom
                | StandardIntrinsic::ArrayCopyFrom
                | StandardIntrinsic::ArrayExtend
        )
    ) || import.binding
        == EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionFromIterator)
        && matches!(
            import.signature.result,
            AbiType::Array(_, CollectionAccess::Mutable)
        )
}

pub(super) fn obligations(
    import: &EngineNativeImport,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<Option<Vec<GenericBoundAbi>>, TypeTransformError> {
    if !selected(import) {
        return Ok(Some(vec![]));
    }
    let index = usize::from(matches!(
        import.binding,
        EngineNativeBinding::Intrinsic(
            StandardIntrinsic::ArrayCopyFrom | StandardIntrinsic::ArrayExtend
        )
    ));
    let Some(source) = import.signature.params.get(index) else {
        return Ok(None);
    };
    let item = if index == 1 {
        match &import.signature.params[0] {
            AbiType::Array(item, _) => item.as_ref(),
            _ => return Ok(None),
        }
    } else {
        match &import.signature.result {
            AbiType::Array(item, _) => item.as_ref(),
            _ => return Ok(None),
        }
    };
    let mut out = Vec::new();
    let iterable = if matches!(import.binding, EngineNativeBinding::Intrinsic(_)) {
        let AbiType::Trait(list) = source else {
            return Ok(None);
        };
        out.push(GenericBoundAbi {
            ty: source.clone(),
            constraints: vec![ConstraintAbi::Trait(list.clone())],
        });
        let Some(iterable) =
            catalog
                .ancestry(list, source, cancel)?
                .into_iter()
                .find(|interface| {
                    StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::Iterable)
                })
        else {
            return Ok(None);
        };
        out.push(GenericBoundAbi {
            ty: source.clone(),
            constraints: vec![ConstraintAbi::Trait(iterable.clone())],
        });
        iterable
    } else {
        let Some(iterable) = import
            .requirements
            .iter()
            .filter(|bound| bound.ty == *source)
            .flat_map(|bound| &bound.constraints)
            .find_map(|constraint| match constraint {
                ConstraintAbi::Trait(interface)
                    if StandardTrait::from_id(&interface.declaration)
                        == Some(StandardTrait::Iterable) =>
                {
                    Some(interface.clone())
                }
                _ => None,
            })
        else {
            return Ok(None);
        };
        iterable
    };
    let iterator = catalog.normalize(
        &AbiType::Projection {
            receiver: Box::new(source.clone()),
            interface: Box::new(iterable.clone()),
            member: associated_type_id(&iterable.declaration, "Iter"),
            arguments: vec![],
        },
        cancel,
    )?;
    let mut next = intrinsic::applied(StandardTrait::Iterator, vec![]);
    next.associated_types
        .insert(associated_type_id(&next.declaration, "Item"), item.clone());
    out.push(GenericBoundAbi {
        ty: iterator,
        constraints: vec![ConstraintAbi::Trait(next)],
    });
    Ok(Some(out))
}
