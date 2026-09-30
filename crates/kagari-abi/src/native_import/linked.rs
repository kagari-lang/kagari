//! Link a concrete import to its checked declaration and selected protocol facts.
use crate::{
    callable::{CallableImplementation, EngineNativeBinding, NativeBinding},
    native_import::{EngineNativeImport, NativeSignature, NativeWitnessImplementation},
    standard::{intrinsic, surface::StandardEnum, traits::StandardTrait},
    types::{
        AbiType, ConcreteFunctionIdentity, ConstraintAbi, GenericBoundAbi, InterfaceTableAbi,
        NativeDeclaration, matching,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, associated_type_id},
};
use std::{collections::HashSet, iter, slice};

impl EngineNativeImport {
    pub fn matches_declaration<'a>(
        &self,
        declaration: &NativeDeclaration,
        catalog: &ProofCatalog<'_>,
        table: impl Fn(&DefinitionId) -> Option<&'a InterfaceTableAbi>,
        callable: impl Fn(&ConcreteFunctionIdentity) -> Option<NativeSignature>,
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let function = &declaration.function;
        if self.instance.declaration != declaration.declaration
            || function.implementation
                != CallableImplementation::Native(NativeBinding::Engine(self.binding))
            || self.instance.arguments.len() != function.generic_params.len()
            || self.resolve().is_none()
        {
            return Ok(false);
        }
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in function.generic_params.iter().zip(&self.instance.arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let mut receiver_owner = declaration.declaration.clone();
        receiver_owner.path.pop();
        if receiver_owner
            .path
            .last()
            .is_some_and(|part| part.kind == DefinitionKind::Trait)
            && let Some(receiver) = self.signature.params.first()
        {
            substitution.bind_receiver(&receiver_owner, receiver);
        }
        let normalize = |ty: &AbiType| catalog.normalize(&substitution.apply(ty, cancel)?, cancel);
        if function.params.len() != self.signature.params.len()
            || normalize(&function.return_type)? != self.signature.result
        {
            return Ok(false);
        }
        for (declared, actual) in function.params.iter().zip(&self.signature.params) {
            if normalize(&declared.ty)? != *actual {
                return Ok(false);
            }
        }
        let mut requirements = substitution.apply_bounds(&function.bounds, cancel)?;
        for bound in &mut requirements {
            bound.ty = catalog.normalize(&bound.ty, cancel)?;
            for constraint in &mut bound.constraints {
                if let ConstraintAbi::Trait(interface) = constraint {
                    let AbiType::Trait(normalized) =
                        catalog.normalize(&AbiType::Trait(interface.clone()), cancel)?
                    else {
                        return Ok(false);
                    };
                    *interface = normalized;
                }
            }
        }
        if requirements != self.requirements {
            return Ok(false);
        }
        // Hash-backed storage needs coherent key equality even if an artifact
        // deletes the corresponding obligation from its declaration template.
        for ty in self
            .signature
            .params
            .iter()
            .chain(iter::once(&self.signature.result))
        {
            let key = match ty {
                AbiType::Map { key, .. } | AbiType::Set(key, _) => key,
                _ => continue,
            };
            for protocol in [StandardTrait::Eq, StandardTrait::Hash] {
                if !catalog.holds(&intrinsic::applied(protocol, vec![]), key, &[], cancel)? {
                    return Ok(false);
                }
            }
        }
        // A native default also consumes the selected implementation of its
        // declaring trait. This is the implicit Self obligation of the checked
        // trait method, independently of its written where-clause.
        let mut obligations = requirements;
        if matches!(self.binding, EngineNativeBinding::TraitDefault(_)) {
            let Some(receiver) = self.signature.params.first() else {
                return Ok(false);
            };
            let owner_arguments: Vec<_> = function
                .generic_params
                .iter()
                .zip(&self.instance.arguments)
                .filter(|(parameter, _)| parameter.owner == receiver_owner)
                .map(|(_, argument)| argument.clone())
                .collect();
            let Some(witness) = self.witnesses.iter().find(|witness| {
                witness.receiver == *receiver
                    && witness.interface.declaration == receiver_owner
                    && witness.interface.arguments == owner_arguments
            }) else {
                return Ok(false);
            };
            obligations.push(GenericBoundAbi {
                ty: receiver.clone(),
                constraints: vec![ConstraintAbi::Trait(witness.interface.clone())],
            });
        }
        let mut consumed = HashSet::new();
        for bound in &obligations {
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
                return Ok(false);
            }
            for constraint in &bound.constraints {
                let ConstraintAbi::Trait(interface) = constraint else {
                    continue;
                };
                let Some((index, witness)) =
                    self.witnesses.iter().enumerate().find(|(_, witness)| {
                        witness.receiver == bound.ty && witness.interface == *interface
                    })
                else {
                    return Ok(false);
                };
                consumed.insert(index);
                let valid = match &witness.implementation {
                    NativeWitnessImplementation::Primitive => {
                        !matches!(bound.ty, AbiType::Host(_) | AbiType::Trait(_))
                            && !catalog.has_explicit_implementation(interface, &bound.ty, cancel)?
                    }
                    NativeWitnessImplementation::Host => matches!(bound.ty, AbiType::Host(_)),
                    NativeWitnessImplementation::Interface => matches!(bound.ty, AbiType::Trait(_)),
                    NativeWitnessImplementation::Table(instance) => {
                        if let Some(table) = table(&instance.declaration) {
                            if let Some(matched) =
                                matching::match_implementation(table, interface, &bound.ty, cancel)?
                            {
                                table.generic_params.len() == instance.arguments.len()
                                    && table.generic_params.iter().zip(&instance.arguments).all(
                                        |(parameter, argument)| {
                                            matched.parameter(&parameter.owner, parameter.position)
                                                == Some(argument)
                                        },
                                    )
                                    && catalog.implementation_count(
                                        interface,
                                        &bound.ty,
                                        &[],
                                        cancel,
                                    )? == 1
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                };
                if !valid {
                    return Ok(false);
                }
            }
        }
        if consumed.len() != self.witnesses.len() {
            return Ok(false);
        }
        if matches!(self.binding, EngineNativeBinding::TraitDefault(_)) {
            for witness in &self.witnesses {
                let protocol = StandardTrait::from_id(&witness.interface.declaration);
                if !matches!(protocol, Some(StandardTrait::Iterator | StandardTrait::Ord))
                    || (protocol == Some(StandardTrait::Iterator)
                        && matches!(witness.receiver, AbiType::Iter(_)))
                    || (protocol == Some(StandardTrait::Ord)
                        && witness.implementation == NativeWitnessImplementation::Primitive)
                {
                    continue;
                }
                let NativeWitnessImplementation::Table(instance) = &witness.implementation else {
                    return Ok(false);
                };
                let Some(declared) = catalog.method(&witness.interface.declaration, 0) else {
                    return Ok(false);
                };
                let Some(table) = table(&instance.declaration)
                    .and_then(|table| table.instantiate(&instance.arguments))
                else {
                    return Ok(false);
                };
                if !table
                    .methods
                    .iter()
                    .any(|method| method.name == declared.name)
                {
                    return Ok(false);
                }
                let mut target = instance.clone();
                target.declaration.path.push(DefinitionPathSegment {
                    kind: DefinitionKind::Method,
                    name: declared.name.clone(),
                    occurrence: 0,
                });
                if declared.generic_params.len() != witness.interface.arguments.len() {
                    return Ok(false);
                }
                let mut substitution = TypeSubstitution::default();
                substitution.bind_receiver(&witness.interface.declaration, &witness.receiver);
                for (parameter, argument) in declared
                    .generic_params
                    .iter()
                    .zip(&witness.interface.arguments)
                {
                    substitution.bind(&parameter.owner, parameter.position, argument);
                }
                let normalize =
                    |ty: &AbiType| catalog.normalize(&substitution.apply(ty, cancel)?, cancel);
                let expected = NativeSignature {
                    params: declared
                        .params
                        .iter()
                        .map(|parameter| normalize(&parameter.ty))
                        .collect::<Result<_, _>>()?,
                    result: normalize(&declared.return_type)?,
                };
                // The declaration supplies the signature; the native consumer
                // additionally checks the storage/callback shape it actually uses.
                let valid = match protocol {
                    Some(StandardTrait::Iterator) => {
                        expected.params.as_slice() == [witness.receiver.clone()]
                            && witness.interface.associated_types.get(&associated_type_id(&witness.interface.declaration, "Item")).is_some_and(|item| matches!(&expected.result, AbiType::StandardEnum {kind:StandardEnum::Option,args} if args.as_slice() == slice::from_ref(item)))
                    }
                    Some(StandardTrait::Ord) => {
                        expected.params.as_slice()
                            == [witness.receiver.clone(), witness.receiver.clone()]
                            && matches!(&expected.result, AbiType::StandardEnum {kind:StandardEnum::Ordering,args} if args.is_empty())
                    }
                    _ => false,
                };
                if !valid {
                    return Ok(false);
                }
                if callable(&target).as_ref() != Some(&expected) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
