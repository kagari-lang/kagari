//! Validate both selected RangeBounds methods and their physical Bound outputs.
use crate::{
    callable::{CallableImplementation, EngineNativeBinding, NativeBinding},
    native_import::{NativeSignature, NativeWitness, NativeWitnessImplementation},
    operations,
    scalar::BuiltinType,
    standard::{bindings::NativeProtocolMethod, surface::StandardEnum, traits::StandardTrait},
    types::{
        AbiType, ConcreteFunctionIdentity, InterfaceTableAbi,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
};
use std::slice;
pub(super) fn valid<'a>(
    witness: &NativeWitness,
    catalog: &ProofCatalog<'_>,
    table: &impl Fn(&DefinitionId) -> Option<&'a InterfaceTableAbi>,
    callable: &impl Fn(&ConcreteFunctionIdentity) -> Option<NativeSignature>,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    let NativeWitnessImplementation::Table(instance) = &witness.implementation else {
        return Ok(false);
    };
    let Some(table) = table(&instance.declaration).and_then(|t| t.instantiate(&instance.arguments))
    else {
        return Ok(false);
    };
    let index = AbiType::Builtin(BuiltinType::USize);
    if StandardTrait::from_id(&witness.interface.declaration) != Some(StandardTrait::RangeBounds)
        || witness.interface.arguments != [index.clone()]
    {
        return Ok(false);
    }
    let output = AbiType::StandardEnum {
        kind: StandardEnum::Bound,
        args: vec![index],
    };
    let Some(parameters) = catalog.parameters(&witness.interface.declaration) else {
        return Ok(false);
    };
    if parameters.len() != 1 {
        return Ok(false);
    }
    let mut substitution = TypeSubstitution::default();
    substitution.bind_receiver(&witness.interface.declaration, &witness.receiver);
    substitution.bind(
        &parameters[0].owner,
        parameters[0].position,
        &witness.interface.arguments[0],
    );
    let mut consumed = Vec::new();
    for slot in 0..2 {
        let Some(declared) = catalog.method(&witness.interface.declaration, slot) else {
            return Ok(false);
        };
        let normalize = |ty: &AbiType| catalog.normalize(&substitution.apply(ty, cancel)?, cancel);
        let expected = NativeSignature {
            params: declared
                .params
                .iter()
                .map(|p| normalize(&p.ty))
                .collect::<Result<_, _>>()?,
            result: normalize(&declared.return_type)?,
        };
        if declared.implementation != CallableImplementation::Required
            || !declared.generic_params.is_empty()
            || expected.params.as_slice() != slice::from_ref(&witness.receiver)
            || expected.result != output
        {
            return Ok(false);
        }
        for bound in substitution.apply_bounds(&declared.bounds, cancel)? {
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
                return Ok(false);
            }
        }
        let Some(method) = table.methods.iter().find(|m| m.name == declared.name) else {
            return Ok(false);
        };
        match method.implementation {
            CallableImplementation::Native(NativeBinding::Engine(
                EngineNativeBinding::Protocol(binding),
            )) if binding
                == [
                    NativeProtocolMethod::RangeStartBound,
                    NativeProtocolMethod::RangeEndBound,
                ][slot]
                && operations::range_bound_valid(&witness.receiver, &output) => {}
            CallableImplementation::Script => {
                let mut target = instance.clone();
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
            _ => return Ok(false),
        }
    }
    Ok(catalog.method(&witness.interface.declaration, 2).is_none() && witness.methods == consumed)
}
