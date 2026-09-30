//! Link a concrete import to its checked declaration and selected protocol facts.
use crate::{
    callable::{CallableImplementation, EngineNativeBinding, NativeBinding},
    native_import::{EngineNativeImport, NativeSignature, NativeWitnessImplementation, keys, sets},
    standard::{
        bindings::{NativeDefaultMethod, NativeProtocolMethod},
        intrinsic,
        traits::StandardTrait,
    },
    types::{
        AbiType, ConcreteFunctionIdentity, ConstraintAbi, GenericBoundAbi, InterfaceTableAbi,
        NativeDeclaration, matching,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, associated_type_id},
};
use std::{collections::HashSet, iter};

mod destinations;
mod lazy;
mod protocols;
mod snapshots;
mod sources;

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
        let Some(snapshot_obligations) = snapshots::obligations(self, catalog, &table, cancel)?
        else {
            return Ok(false);
        };
        let mut obligations = requirements;
        obligations.extend(snapshot_obligations.obligations);
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
            let Some((_, witness)) = self.witnesses.iter().enumerate().find(|(index, witness)| {
                Some(*index) != snapshot_obligations.result
                    && witness.receiver == *receiver
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
        if matches!(
            self.binding,
            EngineNativeBinding::TraitDefault(
                NativeDefaultMethod::ListLast
                    | NativeDefaultMethod::ListBinarySearch
                    | NativeDefaultMethod::ListContains
                    | NativeDefaultMethod::ListStartsWith
                    | NativeDefaultMethod::ListEndsWith
            )
        ) {
            let Some(list) = self.witnesses.iter().find(|witness| {
                StandardTrait::from_id(&witness.interface.declaration) == Some(StandardTrait::List)
                    && self.signature.params.first() == Some(&witness.receiver)
            }) else {
                return Ok(false);
            };
            let Some(iterable) = catalog
                .ancestry(&list.interface, &list.receiver, cancel)?
                .into_iter()
                .find(|interface| {
                    StandardTrait::from_id(&interface.declaration) == Some(StandardTrait::Iterable)
                })
            else {
                return Ok(false);
            };
            obligations.push(GenericBoundAbi {
                ty: list.receiver.clone(),
                constraints: vec![ConstraintAbi::Trait(iterable)],
            });
        }
        let Some(lazy_obligations) = lazy::obligations(self, catalog, cancel)? else {
            return Ok(false);
        };
        obligations.extend(lazy_obligations);
        let Some(array_obligations) = sources::obligations(self, catalog, cancel)? else {
            return Ok(false);
        };
        obligations.extend(array_obligations);
        let Some(destinations) = destinations::applications(self, catalog, &table, cancel)? else {
            return Ok(false);
        };
        obligations.extend(destinations.obligations);
        if matches!(
            self.binding,
            EngineNativeBinding::Protocol(
                NativeProtocolMethod::NumericSum | NativeProtocolMethod::NumericProduct
            )
        ) {
            let Some(source) = self.signature.params.first() else {
                return Ok(false);
            };
            let Some(interface) = obligations
                .iter()
                .filter(|bound| &bound.ty == source)
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
                return Ok(false);
            };
            let iterator = catalog.normalize(
                &AbiType::Projection {
                    receiver: Box::new(source.clone()),
                    interface: Box::new(interface.clone()),
                    member: associated_type_id(&interface.declaration, "Iter"),
                    arguments: vec![],
                },
                cancel,
            )?;
            let mut interface = intrinsic::applied(StandardTrait::Iterator, vec![]);
            interface.associated_types.insert(
                associated_type_id(&interface.declaration, "Item"),
                self.signature.result.clone(),
            );
            obligations.push(GenericBoundAbi {
                ty: iterator,
                constraints: vec![ConstraintAbi::Trait(interface)],
            });
        }
        if matches!(
            self.binding,
            EngineNativeBinding::TraitDefault(
                NativeDefaultMethod::ListStartsWith | NativeDefaultMethod::ListEndsWith
            )
        ) {
            let Some(AbiType::Trait(list)) = self.signature.params.get(1) else {
                return Ok(false);
            };
            let receiver = self.signature.params[1].clone();
            obligations.push(GenericBoundAbi {
                ty: receiver.clone(),
                constraints: vec![ConstraintAbi::Trait(list.clone())],
            });
            let Some(iterable) =
                catalog
                    .ancestry(list, &receiver, cancel)?
                    .into_iter()
                    .find(|interface| {
                        StandardTrait::from_id(&interface.declaration)
                            == Some(StandardTrait::Iterable)
                    })
            else {
                return Ok(false);
            };
            obligations.push(GenericBoundAbi {
                ty: receiver,
                constraints: vec![ConstraintAbi::Trait(iterable)],
            });
        }
        if let Some(key) = keys::key(self) {
            obligations.push(GenericBoundAbi {
                ty: key.clone(),
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
        let Some(set_obligations) = sets::obligations(self, catalog, cancel)? else {
            return Ok(false);
        };
        obligations.extend(set_obligations);
        let mut consumed = HashSet::new();
        // A readonly result table is an explicit physical application. It may
        // share receiver/interface facts with the source List while selecting a
        // native bridge for dynamic publication. Both applications are checked.
        if let Some(index) = snapshot_obligations.result {
            consumed.insert(index);
        }
        for bound in &obligations {
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
                return Ok(false);
            }
            for constraint in &bound.constraints {
                let ConstraintAbi::Trait(interface) = constraint else {
                    continue;
                };
                let Some((index, witness)) = self
                    .witnesses
                    .iter()
                    .enumerate()
                    .filter(|(_, witness)| {
                        witness.receiver == bound.ty && witness.interface == *interface
                    })
                    .min_by_key(|(index, _)| Some(*index) == snapshot_obligations.result)
                else {
                    return Ok(false);
                };
                consumed.insert(index);
                let valid = match &witness.implementation {
                    NativeWitnessImplementation::Primitive => {
                        !matches!(bound.ty, AbiType::Host(_) | AbiType::Trait(_))
                            && !catalog.has_explicit_implementation(interface, &bound.ty, cancel)?
                    }
                    NativeWitnessImplementation::Derived => {
                        matches!(
                            StandardTrait::from_id(&interface.declaration),
                            Some(StandardTrait::PartialEq | StandardTrait::Hash)
                        ) && matches!(
                            bound.ty,
                            AbiType::Tuple(_) | AbiType::Enum(_) | AbiType::StandardEnum { .. }
                        ) && !catalog.has_explicit_implementation(interface, &bound.ty, cancel)?
                            && catalog.uses_custom_equality(&bound.ty, cancel)?
                    }
                    NativeWitnessImplementation::Host => matches!(bound.ty, AbiType::Host(_)),
                    NativeWitnessImplementation::Interface => matches!(bound.ty, AbiType::Trait(_)),
                    NativeWitnessImplementation::Table(instance) => {
                        if let Some(table) = table(&instance.declaration) {
                            if table.native_bridge && Some(index) != snapshot_obligations.result {
                                return Ok(false);
                            }
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
        protocols::valid(
            self,
            catalog,
            table,
            callable,
            &destinations.applications,
            cancel,
        )
    }
}
