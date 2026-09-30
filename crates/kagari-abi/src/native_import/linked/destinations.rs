//! Verify bounded nested FromIterator applications and their source obligations.
use crate::{
    callable::{CallableImplementation, EngineNativeBinding, NativeBinding},
    native_import::{EngineNativeImport, NativeSignature, NativeWitnessImplementation, contract},
    standard::{
        bindings::NativeProtocolMethod, intrinsic, surface::StandardEnum, traits::StandardTrait,
    },
    types::{
        AbiType, ConstraintAbi, GenericBoundAbi, InterfaceTableAbi,
        proofs::ProofCatalog,
        substitution::{MAX_TYPE_NODES, TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    collection::CollectionAccess,
    identity::{DefinitionId, associated_type_id},
};
use std::slice;

pub(super) struct Application {
    pub witness: usize,
    pub source: AbiType,
    pub native: Option<NativeProtocolMethod>,
}
pub(super) struct Destinations {
    pub applications: Vec<Application>,
    pub obligations: Vec<GenericBoundAbi>,
}
pub(super) fn selected(import: &EngineNativeImport) -> bool {
    matches!(
        import.binding,
        EngineNativeBinding::Protocol(
            NativeProtocolMethod::OptionFromIterator | NativeProtocolMethod::ResultFromIterator
        )
    )
}
fn next(
    source: &AbiType,
    bounds: &[GenericBoundAbi],
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<Option<GenericBoundAbi>, TypeTransformError> {
    let Some(interface) = bounds
        .iter()
        .filter(|bound| bound.ty == *source)
        .flat_map(|bound| &bound.constraints)
        .find_map(|constraint| match constraint {
            ConstraintAbi::Trait(interface)
                if StandardTrait::from_id(&interface.declaration)
                    == Some(StandardTrait::Iterable) =>
            {
                Some(interface)
            }
            _ => None,
        })
    else {
        return Ok(None);
    };
    let projection = |name| {
        catalog.normalize(
            &AbiType::Projection {
                receiver: Box::new(source.clone()),
                interface: Box::new(interface.clone()),
                member: associated_type_id(&interface.declaration, name),
                arguments: vec![],
            },
            cancel,
        )
    };
    let mut interface = intrinsic::applied(StandardTrait::Iterator, vec![]);
    interface.associated_types.insert(
        associated_type_id(&interface.declaration, "Item"),
        projection("Item")?,
    );
    Ok(Some(GenericBoundAbi {
        ty: projection("Iter")?,
        constraints: vec![ConstraintAbi::Trait(interface)],
    }))
}
fn inner(output: &AbiType, bounds: &[GenericBoundAbi]) -> Option<(AbiType, AbiType)> {
    let AbiType::StandardEnum {
        kind: StandardEnum::Option | StandardEnum::Result,
        args,
    } = output
    else {
        return None;
    };
    let destination = args.first()?;
    let interface = bounds
        .iter()
        .filter(|bound| &bound.ty == destination)
        .flat_map(|bound| &bound.constraints)
        .find_map(|constraint| match constraint {
            ConstraintAbi::Trait(interface)
                if StandardTrait::from_id(&interface.declaration)
                    == Some(StandardTrait::FromIterator) =>
            {
                Some(interface)
            }
            _ => None,
        })?;
    let [item] = interface.arguments.as_slice() else {
        return None;
    };
    Some((
        destination.clone(),
        AbiType::Array(Box::new(item.clone()), CollectionAccess::Mutable),
    ))
}
pub(super) fn applications<'a>(
    import: &EngineNativeImport,
    catalog: &ProofCatalog<'_>,
    table: &impl Fn(&DefinitionId) -> Option<&'a InterfaceTableAbi>,
    cancel: &CancellationToken,
) -> Result<Option<Destinations>, TypeTransformError> {
    let mut result = Destinations {
        applications: vec![],
        obligations: vec![],
    };
    if !selected(import) {
        return Ok(Some(result));
    }
    let Some(source) = import.signature.params.first() else {
        return Ok(None);
    };
    let Some(original_next) = next(source, &import.requirements, catalog, cancel)? else {
        return Ok(None);
    };
    result.obligations.push(original_next);
    let Some(first) = inner(&import.signature.result, &import.requirements) else {
        return Ok(None);
    };
    let mut pending = vec![first];
    while let Some((destination, source)) = pending.pop() {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if result.applications.len() >= MAX_TYPE_NODES {
            return Ok(None);
        }
        let item = match &source {
            AbiType::Array(item, _) => item.as_ref(),
            _ => return Ok(None),
        };
        let Some((index, witness)) = import.witnesses.iter().enumerate().find(|(_, witness)| {
            witness.receiver == destination
                && StandardTrait::from_id(&witness.interface.declaration)
                    == Some(StandardTrait::FromIterator)
                && witness.interface.arguments.as_slice() == slice::from_ref(item)
        }) else {
            return Ok(None);
        };
        if result
            .applications
            .iter()
            .any(|application| application.witness == index)
        {
            return Ok(None);
        }
        let NativeWitnessImplementation::Table(instance) = &witness.implementation else {
            return Ok(None);
        };
        let Some(table) =
            table(&instance.declaration).and_then(|table| table.instantiate(&instance.arguments))
        else {
            return Ok(None);
        };
        let [method] = table.methods.as_slice() else {
            return Ok(None);
        };
        let [parameter] = method.generic_params.as_slice() else {
            return Ok(None);
        };
        let mut substitution = TypeSubstitution::default();
        substitution.bind(&parameter.owner, parameter.position, &source);
        let normalize = |ty: &AbiType| catalog.normalize(&substitution.apply(ty, cancel)?, cancel);
        if method.params.len() != 1
            || normalize(&method.params[0].ty)? != source
            || normalize(&method.return_type)? != destination
        {
            return Ok(None);
        }
        let mut bounds = table.bounds.clone();
        bounds.extend(substitution.apply_bounds(&method.bounds, cancel)?);
        for bound in &mut bounds {
            bound.ty = catalog.normalize(&bound.ty, cancel)?;
            for constraint in &mut bound.constraints {
                if let ConstraintAbi::Trait(interface) = constraint {
                    let AbiType::Trait(normalized) =
                        catalog.normalize(&AbiType::Trait(interface.clone()), cancel)?
                    else {
                        return Ok(None);
                    };
                    *interface = normalized;
                }
            }
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
                return Ok(None);
            }
        }
        let native = match method.implementation {
            CallableImplementation::Native(NativeBinding::Engine(
                EngineNativeBinding::Protocol(provider),
            )) => Some(provider),
            CallableImplementation::Script => None,
            _ => return Ok(None),
        };
        if let Some(provider) = native {
            if !matches!(
                provider,
                NativeProtocolMethod::CollectionFromIterator
                    | NativeProtocolMethod::OptionFromIterator
                    | NativeProtocolMethod::ResultFromIterator
            ) || !contract::binding_signature_valid(
                EngineNativeBinding::Protocol(provider),
                &NativeSignature {
                    params: vec![source.clone()],
                    result: destination.clone(),
                },
                &bounds,
            ) {
                return Ok(None);
            }
            let Some(next) = next(&source, &bounds, catalog, cancel)? else {
                return Ok(None);
            };
            result.obligations.extend(bounds.clone());
            result.obligations.push(next);
            if provider == NativeProtocolMethod::CollectionFromIterator {
                if !matches!(
                    destination,
                    AbiType::Array(_, CollectionAccess::Mutable)
                        | AbiType::Set(_, CollectionAccess::Mutable)
                        | AbiType::Map {
                            access: CollectionAccess::Mutable,
                            ..
                        }
                ) {
                    return Ok(None);
                }
                if let AbiType::Map { key, .. } | AbiType::Set(key, _) = &destination {
                    result.obligations.push(GenericBoundAbi {
                        ty: key.as_ref().clone(),
                        constraints: [
                            StandardTrait::Eq,
                            StandardTrait::Hash,
                            StandardTrait::PartialEq,
                        ]
                        .into_iter()
                        .map(|protocol| ConstraintAbi::Trait(intrinsic::applied(protocol, vec![])))
                        .collect(),
                    });
                }
            } else {
                let Some(child) = inner(&destination, &bounds) else {
                    return Ok(None);
                };
                pending.push(child);
            }
        }
        result.applications.push(Application {
            witness: index,
            source,
            native,
        });
    }
    Ok(Some(result))
}
