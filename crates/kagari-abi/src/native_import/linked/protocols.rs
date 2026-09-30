//! Validate selected required-method applications against carried declarations.
use super::sources;
use crate::{
    callable::{CallableImplementation, EngineNativeBinding, NativeBinding},
    native_import::{EngineNativeImport, NativeSignature, NativeWitnessImplementation, keys},
    scalar::BuiltinType,
    standard::{
        StandardIntrinsic,
        bindings::{NativeDefaultMethod, NativeProtocolMethod},
        surface::StandardEnum,
        traits::StandardTrait,
    },
    types::{
        AbiType, ConcreteFunctionIdentity, InterfaceTableAbi,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, associated_type_id},
};
use std::slice;

pub(super) fn valid<'a>(
    import: &EngineNativeImport,
    catalog: &ProofCatalog<'_>,
    table: impl Fn(&DefinitionId) -> Option<&'a InterfaceTableAbi>,
    callable: impl Fn(&ConcreteFunctionIdentity) -> Option<NativeSignature>,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    for witness in &import.witnesses {
        let list_query = matches!(
            import.binding,
            EngineNativeBinding::TraitDefault(
                NativeDefaultMethod::ListFirst
                    | NativeDefaultMethod::ListLast
                    | NativeDefaultMethod::ListBinarySearch
                    | NativeDefaultMethod::ListContains
                    | NativeDefaultMethod::ListStartsWith
                    | NativeDefaultMethod::ListEndsWith
            )
        );
        let snapshot = matches!(
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
        let collection = StandardTrait::from_id(&witness.interface.declaration);
        if matches!(
            import.binding,
            EngineNativeBinding::Intrinsic(
                StandardIntrinsic::ArrayCopyWithin | StandardIntrinsic::ArrayRemoveRange
            )
        ) && collection == Some(StandardTrait::RangeBounds)
        {
            if !ranges::valid(witness, catalog, &table, &callable, cancel)? {
                return Ok(false);
            }
            continue;
        }
        if (list_query || snapshot || sources::selected(import))
            && collection == Some(StandardTrait::List)
            || snapshot && collection == Some(StandardTrait::Map)
        {
            if !collections::valid(witness, catalog, &table, &callable, cancel)? {
                return Ok(false);
            }
            continue;
        }

        let protocol = StandardTrait::from_id(&witness.interface.declaration);
        let aggregate = matches!(
            import.binding,
            EngineNativeBinding::TraitDefault(
                NativeDefaultMethod::Sum | NativeDefaultMethod::Product
            )
        ) && protocol.is_some_and(StandardTrait::aggregation);
        let numeric = matches!(
            import.binding,
            EngineNativeBinding::Protocol(
                NativeProtocolMethod::NumericSum | NativeProtocolMethod::NumericProduct
            )
        );
        let conversion = numeric || list_query || snapshot || sources::selected(import);
        let prepared_order = matches!(
            import.binding,
            EngineNativeBinding::Intrinsic(
                StandardIntrinsic::ArraySort | StandardIntrinsic::ArraySortByKey
            )
        );
        let equality = (list_query
            || keys::key(import).is_some()
            || import.binding == EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayDedup))
            && protocol == Some(StandardTrait::PartialEq);
        let hashing = keys::key(import).is_some() && protocol == Some(StandardTrait::Hash);
        if (equality || hashing)
            && witness.implementation == NativeWitnessImplementation::Interface
            && matches!(&witness.receiver,AbiType::Trait(interface) if StandardTrait::from_id(&interface.declaration).is_some_and(StandardTrait::collection))
        {
            if !witness.methods.is_empty() {
                return Ok(false);
            }
            continue;
        }
        if (equality || hashing)
            && matches!(
                witness.implementation,
                NativeWitnessImplementation::Primitive | NativeWitnessImplementation::Derived
            )
        {
            let composed = catalog.uses_custom_equality(&witness.receiver, cancel)?;
            if witness.implementation == NativeWitnessImplementation::Primitive {
                if composed || !witness.methods.is_empty() {
                    return Ok(false);
                }
            } else {
                let [target] = witness.methods.as_slice() else {
                    return Ok(false);
                };
                if !composed
                    || target.arguments != [witness.receiver.clone()]
                    || target.declaration.path.as_slice()
                        != [DefinitionPathSegment {
                            kind: DefinitionKind::Function,
                            name: if hashing {
                                "$derived_Hash"
                            } else {
                                "$derived_PartialEq"
                            }
                            .into(),
                            occurrence: 0,
                        }]
                    || callable(target)
                        != Some(NativeSignature {
                            params: if hashing {
                                vec![witness.receiver.clone()]
                            } else {
                                vec![witness.receiver.clone(), witness.receiver.clone()]
                            },
                            result: AbiType::Builtin(if hashing {
                                BuiltinType::I64
                            } else {
                                BuiltinType::Bool
                            }),
                        })
                {
                    return Ok(false);
                }
            }
            continue;
        }
        let invoked = (conversion
            && matches!(
                protocol,
                Some(StandardTrait::Iterable | StandardTrait::Iterator)
            ))
            || matches!(import.binding, EngineNativeBinding::TraitDefault(_))
                && (matches!(protocol, Some(StandardTrait::Iterator | StandardTrait::Ord))
                    || aggregate
                    || equality)
            || prepared_order && protocol == Some(StandardTrait::Ord)
            || equality
            || hashing;
        if !invoked
            || (protocol == Some(StandardTrait::Iterator)
                && matches!(witness.receiver, AbiType::Iter(_)))
            || ((protocol == Some(StandardTrait::Ord) || aggregate)
                && witness.implementation == NativeWitnessImplementation::Primitive)
            || (conversion
                && protocol == Some(StandardTrait::Iterable)
                && matches!(
                    witness.implementation,
                    NativeWitnessImplementation::Primitive | NativeWitnessImplementation::Interface
                ))
        {
            if !witness.methods.is_empty() {
                return Ok(false);
            }
            continue;
        }
        let NativeWitnessImplementation::Table(instance) = &witness.implementation else {
            return Ok(false);
        };
        let Some(declared) = catalog.method(&witness.interface.declaration, 0) else {
            return Ok(false);
        };
        let Some(table) =
            table(&instance.declaration).and_then(|table| table.instantiate(&instance.arguments))
        else {
            return Ok(false);
        };
        let Some(method) = table
            .methods
            .iter()
            .find(|method| method.name == declared.name)
        else {
            return Ok(false);
        };
        let native = match method.implementation {
            CallableImplementation::Native(NativeBinding::Engine(
                EngineNativeBinding::Protocol(binding),
            )) => Some(binding),
            _ => None,
        };
        let consumed_native = match (protocol, native) {
            (Some(StandardTrait::Iterable), Some(NativeProtocolMethod::CollectionIter)) => {
                conversion
            }
            (Some(StandardTrait::Sum), Some(NativeProtocolMethod::NumericSum))
            | (Some(StandardTrait::Product), Some(NativeProtocolMethod::NumericProduct)) => {
                aggregate
            }
            _ => false,
        };
        if consumed_native {
            if !witness.methods.is_empty() {
                return Ok(false);
            }
            continue;
        }
        let mut target = instance.clone();
        target.declaration.path.push(DefinitionPathSegment {
            kind: DefinitionKind::Method,
            name: declared.name.clone(),
            occurrence: 0,
        });
        let method_arguments = if aggregate {
            &import.signature.params[..1]
        } else {
            &[]
        };
        target.arguments.extend_from_slice(method_arguments);
        if witness.methods.as_slice() != slice::from_ref(&target)
            || declared.generic_params.len() != method_arguments.len()
        {
            return Ok(false);
        }
        let mut substitution = TypeSubstitution::default();
        substitution.bind_receiver(&witness.interface.declaration, &witness.receiver);
        let Some(parameters) = catalog.parameters(&witness.interface.declaration) else {
            return Ok(false);
        };
        if parameters.len() != witness.interface.arguments.len() {
            return Ok(false);
        }
        for (parameter, argument) in parameters.iter().zip(&witness.interface.arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        for (parameter, argument) in declared.generic_params.iter().zip(method_arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let normalize = |ty: &AbiType| catalog.normalize(&substitution.apply(ty, cancel)?, cancel);
        let expected = NativeSignature {
            params: declared
                .params
                .iter()
                .map(|parameter| normalize(&parameter.ty))
                .collect::<Result<_, _>>()?,
            result: normalize(&declared.return_type)?,
        };
        for bound in substitution.apply_bounds(&declared.bounds, cancel)? {
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
                return Ok(false);
            }
        }
        // Declarations supply semantic contracts; consumers guard the physical
        // arguments and results they actually pass across the callback boundary.
        let valid = match protocol {
            Some(StandardTrait::Iterator) => {
                let item_id = associated_type_id(&witness.interface.declaration, "Item");
                expected.params.as_slice() == slice::from_ref(&witness.receiver)
                    && witness
                        .interface
                        .associated_types
                        .get(&item_id)
                        .is_some_and(|item| {
                            matches!(&expected.result, AbiType::StandardEnum {
                            kind: StandardEnum::Option, args
                        } if args.as_slice() == slice::from_ref(item))
                        })
            }
            Some(StandardTrait::Iterable) if conversion => {
                let iterator = catalog.normalize(
                    &AbiType::Projection {
                        receiver: Box::new(witness.receiver.clone()),
                        interface: Box::new(witness.interface.clone()),
                        member: associated_type_id(&witness.interface.declaration, "Iter"),
                        arguments: vec![],
                    },
                    cancel,
                )?;
                expected.params.as_slice() == slice::from_ref(&witness.receiver)
                    && expected.result == iterator
            }
            Some(StandardTrait::PartialEq) if equality => {
                expected.params.as_slice() == [witness.receiver.clone(), witness.receiver.clone()]
                    && expected.result == AbiType::Builtin(BuiltinType::Bool)
            }
            Some(StandardTrait::Hash) if hashing => {
                expected.params.as_slice() == slice::from_ref(&witness.receiver)
                    && expected.result == AbiType::Builtin(BuiltinType::I64)
            }
            Some(StandardTrait::Ord) => {
                expected.params.as_slice() == [witness.receiver.clone(), witness.receiver.clone()]
                    && matches!(&expected.result, AbiType::StandardEnum {
                        kind: StandardEnum::Ordering, args
                    } if args.is_empty())
            }
            Some(StandardTrait::Sum | StandardTrait::Product) if aggregate => {
                expected.params == import.signature.params
                    && expected.result == witness.receiver
                    && expected.result == import.signature.result
            }
            _ => false,
        };
        if !valid || callable(&target).as_ref() != Some(&expected) {
            return Ok(false);
        }
    }
    Ok(true)
}

mod collections;
mod ranges;
