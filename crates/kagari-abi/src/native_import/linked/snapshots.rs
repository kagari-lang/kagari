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
pub(super) fn obligations<'a>(
    import: &EngineNativeImport,
    catalog: &ProofCatalog<'_>,
    table: &impl Fn(&DefinitionId) -> Option<&'a InterfaceTableAbi>,
    cancel: &CancellationToken,
) -> Result<Option<Vec<GenericBoundAbi>>, TypeTransformError> {
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
        )
    );
    if !readonly_result {
        return Ok(Some(vec![]));
    }
    let mut obligations = Vec::new();
    let AbiType::Trait(interface) = &import.signature.result else {
        return Ok(None);
    };
    let [item] = interface.arguments.as_slice() else {
        return Ok(None);
    };
    let storage = AbiType::Array(Box::new(item.clone()), CollectionAccess::Mutable);
    let Some(factory) = import
        .witnesses
        .iter()
        .find(|witness| witness.receiver == storage && witness.interface == *interface)
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
    if matches!(import.binding, EngineNativeBinding::TraitDefault(_)) {
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
    Ok(Some(obligations))
}
