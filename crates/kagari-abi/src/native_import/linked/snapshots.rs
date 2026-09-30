//! Source-free obligations for native readonly List construction and Map traversal.
use crate::{
    callable::EngineNativeBinding,
    native_import::{EngineNativeImport, NativeWitnessImplementation},
    standard::{
        StandardIntrinsic, bindings::NativeDefaultMethod, intrinsic, traits::StandardTrait,
    },
    types::{
        AbiType, ConstraintAbi, GenericBoundAbi, InterfaceTableAbi, proofs::ProofCatalog,
        substitution::TypeTransformError,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    collection::CollectionAccess,
    identity::{DefinitionId, associated_type_id},
};
pub(super) struct Applications {
    pub obligations: Vec<GenericBoundAbi>,
    pub result: Option<usize>,
}
pub(super) fn obligations<'a>(
    import: &EngineNativeImport,
    catalog: &ProofCatalog<'_>,
    table: &impl Fn(&DefinitionId) -> Option<&'a InterfaceTableAbi>,
    cancel: &CancellationToken,
) -> Result<Option<Applications>, TypeTransformError> {
    let readonly_result = matches!(
        import.binding,
        EngineNativeBinding::Intrinsic(
            StandardIntrinsic::MapKeys
                | StandardIntrinsic::MapValues
                | StandardIntrinsic::MapEntries
                | StandardIntrinsic::ArrayRemoveRange
        ) | EngineNativeBinding::TraitDefault(
            NativeDefaultMethod::MapKeysView
                | NativeDefaultMethod::MapValuesView
                | NativeDefaultMethod::MapEntriesView
                | NativeDefaultMethod::ListWindows
                | NativeDefaultMethod::ListChunks
        )
    );
    if !readonly_result {
        return Ok(Some(Applications {
            obligations: vec![],
            result: None,
        }));
    }
    let mut obligations = Vec::new();
    let result = match &import.signature.result {
        AbiType::Iter(item) => item.as_ref(),
        result => result,
    };
    let AbiType::Trait(interface) = result else {
        return Ok(None);
    };
    let [item] = interface.arguments.as_slice() else {
        return Ok(None);
    };
    let storage = AbiType::Array(Box::new(item.clone()), CollectionAccess::Mutable);
    let Some((result_index, factory)) = import.witnesses.iter().enumerate().find(|(_,witness)| {
        witness.receiver == storage && witness.interface == *interface
            && matches!(&witness.implementation, NativeWitnessImplementation::Table(instance) if table(&instance.declaration).is_some_and(|table| table.native_bridge))
    })
    else {
        return Ok(None);
    };
    let NativeWitnessImplementation::Table(instance) = &factory.implementation else {
        return Ok(None);
    };
    let Some(factory_table) = table(&instance.declaration) else {
        return Ok(None);
    };
    if !factory_table.native_bridge
        || factory_table.host_bridge
        || !instance.arguments.is_empty()
        || factory_table.for_type != storage
        || factory_table.trait_type != AbiType::Trait(interface.clone())
    {
        return Ok(None);
    }
    obligations.push(GenericBoundAbi {
        ty: storage,
        constraints: vec![ConstraintAbi::Trait(interface.clone())],
    });
    if matches!(
        import.binding,
        EngineNativeBinding::TraitDefault(
            NativeDefaultMethod::MapKeysView
                | NativeDefaultMethod::MapValuesView
                | NativeDefaultMethod::MapEntriesView
        )
    ) {
        let Some(source) = import.witnesses.iter().find(|witness| {
            import.signature.params.first() == Some(&witness.receiver)
                && StandardTrait::from_id(&witness.interface.declaration)
                    == Some(StandardTrait::Map)
        }) else {
            return Ok(None);
        };
        let Some(iterable) = catalog
            .ancestry(&source.interface, &source.receiver, cancel)?
            .into_iter()
            .find(|interface| {
                StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::Iterable)
            })
        else {
            return Ok(None);
        };
        let iterator = catalog.normalize(
            &AbiType::Projection {
                receiver: Box::new(source.receiver.clone()),
                interface: Box::new(iterable.clone()),
                member: associated_type_id(&iterable.declaration, "Iter"),
                arguments: vec![],
            },
            cancel,
        )?;
        let [key, value] = source.interface.arguments.as_slice() else {
            return Ok(None);
        };
        let mut next = intrinsic::applied(StandardTrait::Iterator, vec![]);
        next.associated_types.insert(
            associated_type_id(&next.declaration, "Item"),
            AbiType::Tuple(vec![key.clone(), value.clone()]),
        );
        obligations.push(GenericBoundAbi {
            ty: source.receiver.clone(),
            constraints: vec![ConstraintAbi::Trait(iterable)],
        });
        obligations.push(GenericBoundAbi {
            ty: iterator,
            constraints: vec![ConstraintAbi::Trait(next)],
        });
    }
    Ok(Some(Applications {
        obligations,
        result: Some(result_index),
    }))
}
