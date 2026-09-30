//! Validate the complete selected List/Map/Set requirement sets, including native storage.
use crate::{
    callable::{CallableImplementation, EngineNativeBinding, NativeBinding},
    native_import::{NativeSignature, NativeWitness, NativeWitnessImplementation},
    scalar::BuiltinType,
    standard::{StandardIntrinsic, surface::StandardEnum, traits::StandardTrait},
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

pub(super) fn valid<'a>(
    witness: &NativeWitness,
    catalog: &ProofCatalog<'_>,
    table: &impl Fn(&DefinitionId) -> Option<&'a InterfaceTableAbi>,
    callable: &impl Fn(&ConcreteFunctionIdentity) -> Option<NativeSignature>,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    let table = match &witness.implementation {
        NativeWitnessImplementation::Interface if witness.methods.is_empty() => None,
        NativeWitnessImplementation::Table(instance) => {
            let Some(table) = table(&instance.declaration)
                .and_then(|table| table.instantiate(&instance.arguments))
            else {
                return Ok(false);
            };
            Some((instance, table))
        }
        _ => return Ok(false),
    };
    let protocol = StandardTrait::from_id(&witness.interface.declaration);
    let item = match (protocol, witness.interface.arguments.as_slice()) {
        (Some(StandardTrait::List | StandardTrait::Set), [item]) => item.clone(),
        (Some(StandardTrait::Map), [key, value]) => {
            AbiType::Tuple(vec![key.clone(), value.clone()])
        }
        _ => return Ok(false),
    };
    let Some(iterable) = catalog
        .ancestry(&witness.interface, &witness.receiver, cancel)?
        .into_iter()
        .find(|interface| {
            StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::Iterable)
        })
    else {
        return Ok(false);
    };
    if iterable
        .associated_types
        .get(&associated_type_id(&iterable.declaration, "Item"))
        != Some(&item)
        || iterable
            .associated_types
            .get(&associated_type_id(&iterable.declaration, "Iter"))
            != Some(&AbiType::Iter(Box::new(item.clone())))
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
    let mut consumed = Vec::new();
    let mut required = 0;
    let mut slot = 0;
    while let Some(declared) = catalog.method(&witness.interface.declaration, slot) {
        slot += 1;
        if declared.implementation != CallableImplementation::Required {
            continue;
        }
        let normalize = |ty: &AbiType| catalog.normalize(&substitution.apply(ty, cancel)?, cancel);
        let expected = NativeSignature {
            params: declared
                .params
                .iter()
                .map(|p| normalize(&p.ty))
                .collect::<Result<_, _>>()?,
            result: normalize(&declared.return_type)?,
        };
        let index = AbiType::Builtin(BuiltinType::USize);
        let shape = match required {
            0 => expected.params == [witness.receiver.clone()] && expected.result == index,
            1 => {
                expected.params == [witness.receiver.clone()]
                    && expected.result == AbiType::Builtin(BuiltinType::Bool)
            }
            2 if protocol == Some(StandardTrait::Map) => {
                expected.params
                    == [
                        witness.receiver.clone(),
                        witness.interface.arguments[0].clone(),
                    ]
                    && expected.result == AbiType::Builtin(BuiltinType::Bool)
            }
            3 if protocol == Some(StandardTrait::Map) => {
                expected.params
                    == [
                        witness.receiver.clone(),
                        witness.interface.arguments[0].clone(),
                    ]
                    && expected.result
                        == AbiType::StandardEnum {
                            kind: StandardEnum::Option,
                            args: vec![witness.interface.arguments[1].clone()],
                        }
            }
            2 if protocol == Some(StandardTrait::Set) => {
                expected.params == [witness.receiver.clone(), item.clone()]
                    && expected.result == AbiType::Builtin(BuiltinType::Bool)
            }
            2 => {
                expected.params == [witness.receiver.clone(), index]
                    && expected.result
                        == AbiType::StandardEnum {
                            kind: StandardEnum::Option,
                            args: vec![item.clone()],
                        }
            }
            _ => false,
        };
        if !shape || !declared.generic_params.is_empty() {
            return Ok(false);
        }
        for bound in substitution.apply_bounds(&declared.bounds, cancel)? {
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
                return Ok(false);
            }
        }
        let Some((instance, table)) = &table else {
            required += 1;
            continue;
        };
        let Some(method) = table
            .methods
            .iter()
            .find(|method| method.name == declared.name)
        else {
            return Ok(false);
        };
        if let CallableImplementation::Native(NativeBinding::Engine(
            EngineNativeBinding::Intrinsic(binding),
        )) = method.implementation
        {
            let (operation, storage) = if protocol == Some(StandardTrait::List) {
                (
                    [
                        StandardIntrinsic::ArrayLen,
                        StandardIntrinsic::ArrayIsEmpty,
                        StandardIntrinsic::ArrayGet,
                    ][required],
                    matches!(&witness.receiver,AbiType::Array(element,_) if element.as_ref()==&item),
                )
            } else if protocol == Some(StandardTrait::Set) {
                (
                    [
                        StandardIntrinsic::SetLen,
                        StandardIntrinsic::SetIsEmpty,
                        StandardIntrinsic::SetContains,
                    ][required],
                    matches!(&witness.receiver,AbiType::Set(element,_) if element.as_ref()==&item),
                )
            } else {
                (
                    [
                        StandardIntrinsic::MapLen,
                        StandardIntrinsic::MapIsEmpty,
                        StandardIntrinsic::MapContainsKey,
                        StandardIntrinsic::MapGet,
                    ][required],
                    matches!(&witness.receiver,AbiType::Map {key,value,..} if witness.interface.arguments==[key.as_ref().clone(),value.as_ref().clone()]),
                )
            };
            if binding != operation || !storage {
                return Ok(false);
            }
        } else {
            if method.implementation != CallableImplementation::Script {
                return Ok(false);
            }
            let mut target = (*instance).clone();
            target.declaration.path.push(DefinitionPathSegment {
                kind: DefinitionKind::Method,
                name: declared.name.clone(),
                occurrence: 0,
            });
            if callable(&target).as_ref() != Some(&expected) {
                return Ok(false);
            }
            consumed.push(target);
        }
        required += 1;
    }
    Ok(required
        == if protocol != Some(StandardTrait::Map) {
            3
        } else {
            4
        }
        && witness.methods == consumed)
}
