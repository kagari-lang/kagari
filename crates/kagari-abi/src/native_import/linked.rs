//! Link a concrete import to its checked declaration and selected protocol facts.
use crate::{
    callable::{CallableImplementation, NativeBinding},
    native_import::{EngineNativeImport, NativeWitnessImplementation},
    standard::{intrinsic, traits::StandardTrait},
    types::{
        AbiType, ConstraintAbi, InterfaceTableAbi, NativeDeclaration, matching,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind},
};
use std::{collections::HashSet, iter};

impl EngineNativeImport {
    pub fn matches_declaration<'a>(
        &self,
        declaration: &NativeDeclaration,
        catalog: &ProofCatalog<'_>,
        table: impl Fn(&DefinitionId) -> Option<&'a InterfaceTableAbi>,
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let function = &declaration.function;
        if self.instance.declaration != declaration.declaration
            || function.implementation
                != CallableImplementation::Native(NativeBinding::Engine(self.binding))
            || self.instance.arguments.len() != function.generic_params.len()
            || self.direct_operation().is_none()
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
        let mut consumed = HashSet::new();
        for bound in &requirements {
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
        Ok(consumed.len() == self.witnesses.len())
    }
}
