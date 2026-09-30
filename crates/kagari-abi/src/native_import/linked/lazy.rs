//! Source-free traversal and output obligations for lazy iterator construction.
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
use std::slice;

pub(super) fn selected(import: &EngineNativeImport) -> bool {
    matches!(import.binding, EngineNativeBinding::TraitDefault(operation) if operation.lazy())
}
pub(super) fn obligations(
    import: &EngineNativeImport,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<Option<Vec<GenericBoundAbi>>, TypeTransformError> {
    let EngineNativeBinding::TraitDefault(operation) = import.binding else {
        return Ok(Some(vec![]));
    };
    let source = match operation {
        NativeDefaultMethod::Zip | NativeDefaultMethod::Chain => import.signature.params.get(1),
        NativeDefaultMethod::FlatMap => match import.signature.params.get(1) {
            Some(AbiType::Function { result, .. }) => Some(result.as_ref()),
            _ => return Ok(None),
        },
        NativeDefaultMethod::Flatten => import
            .witnesses
            .iter()
            .find(|witness| {
                import.signature.params.first() == Some(&witness.receiver)
                    && StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::Iterator)
            })
            .and_then(|witness| {
                witness
                    .interface
                    .associated_types
                    .get(&associated_type_id(&witness.interface.declaration, "Item"))
            }),
        NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks => {
            import.signature.params.first()
        }
        _ => return Ok(Some(vec![])),
    };
    let Some(source) = source else {
        return Ok(None);
    };
    let mut iterable = intrinsic::applied(StandardTrait::Iterable, vec![]);
    for name in ["Item", "Iter"] {
        let member = associated_type_id(&iterable.declaration, name);
        let output = catalog.normalize(
            &AbiType::Projection {
                receiver: Box::new(source.clone()),
                interface: Box::new(iterable.clone()),
                member: member.clone(),
                arguments: vec![],
            },
            cancel,
        )?;
        iterable.associated_types.insert(member, output);
    }
    let item = &iterable.associated_types[&associated_type_id(&iterable.declaration, "Item")];
    let iterator = &iterable.associated_types[&associated_type_id(&iterable.declaration, "Iter")];
    let AbiType::Iter(output) = &import.signature.result else {
        return Ok(None);
    };
    let valid_output = match operation {
        NativeDefaultMethod::Zip => {
            matches!(output.as_ref(),AbiType::Tuple(items) if items.len()==2 && &items[1]==item)
        }
        NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks => {
            matches!(output.as_ref(),AbiType::Trait(list) if StandardTrait::from_id(&list.declaration)==Some(StandardTrait::List) && list.arguments.as_slice()==slice::from_ref(item))
        }
        _ => output.as_ref() == item,
    };
    if !valid_output {
        return Ok(None);
    }
    let mut next = intrinsic::applied(StandardTrait::Iterator, vec![]);
    next.associated_types
        .insert(associated_type_id(&next.declaration, "Item"), item.clone());
    // Linkage also compares the fully normalized declaration signature. These
    // obligations bind the runtime's selected conversion and next to that same
    // concrete source, rather than accepting an unrelated witness with its Item.
    Ok(Some(vec![
        GenericBoundAbi {
            ty: source.clone(),
            constraints: vec![ConstraintAbi::Trait(iterable.clone())],
        },
        GenericBoundAbi {
            ty: iterator.clone(),
            constraints: vec![ConstraintAbi::Trait(next)],
        },
    ]))
}
