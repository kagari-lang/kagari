//! Shared script entries retain method binders while specializing the receiver.
use crate::source::lower::{
    MirLoweringError,
    abi::checked_bounds,
    instances::{Instance, InstancePlanner},
};
use kagari_common::{
    identity::table::{DefinitionId, DefinitionTable},
    span::Span,
};
use kagari_contract::callable::{
    generic::GenericBody, interface::TypeNormalization, witness::OperationWitness,
};
use kagari_hir::{
    aggregates::AggregateCatalog,
    resolver::resolved::ResolvedName,
    typeck::{GenericBounds, TypedFunction, table::ConstraintTarget},
    types::{GenericParameterType, NominalType, TypeId, TypeSubstitution},
};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    ty::{GenericParam, Ty},
};

impl InstancePlanner<'_> {
    /// Preserve associated projections that disappear while applying a method's
    /// checked signature. Executable verification rechecks these equalities.
    pub(crate) fn method_normalizations(
        &self,
        method: &DefinitionId,
        interface: &NominalType,
        receiver: &TypeId,
        arguments: &[TypeId],
    ) -> Result<Vec<TypeNormalization>, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("interface signature normalization");
        let signature = self.catalog.trait_method(method).ok_or_else(invalid)?;
        if signature.generic_params.len() != interface.arguments.len() + arguments.len() {
            return Err(invalid());
        }
        let mut substitution: TypeSubstitution = signature
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().chain(arguments).cloned())
            .collect();
        substitution.insert_receiver(interface.declaration, receiver.clone());
        let mut facts = Vec::new();
        for ty in signature
            .params
            .iter()
            .filter(|param| param.name != "self")
            .map(|param| &param.ty)
            .chain([&signature.return_type])
        {
            let source = ty
                .instantiate(&substitution)
                .with_associated_types(interface, receiver);
            let result =
                self.encode_type(&self.catalog.normalize_type(&self.definitions(), &source));
            let source = self.encode_type(&source);
            if source != result
                && !facts
                    .iter()
                    .any(|fact: &TypeNormalization| fact.source == source)
            {
                facts.push(TypeNormalization { source, result });
            }
        }
        Ok(facts)
    }

    /// Supply the member operations promised by receiver and method bounds.
    /// Receiver implementations may differ, so this follows the declaration's
    /// bounds rather than inspecting one selected default body.
    pub(crate) fn method_operations(
        &mut self,
        method: &DefinitionId,
        interface: &NominalType,
        receiver: &TypeId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<Vec<OperationWitness>, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("generic method operation environment");
        let signature = self.catalog.trait_method(method).ok_or_else(invalid)?;
        let trait_ = self
            .catalog
            .trait_(&interface.declaration)
            .ok_or_else(invalid)?;
        let mut substitution: TypeSubstitution = signature
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().chain(arguments).cloned())
            .collect();
        substitution.insert_receiver(trait_.id, receiver.clone());
        let mut bounds = trait_.bounds.clone();
        for (ty, constraints) in &signature.bounds {
            bounds
                .entry(ty.clone())
                .or_default()
                .extend(constraints.iter().cloned());
        }
        let bounds = bounds
            .into_iter()
            .map(|(ty, targets)| {
                (
                    ty.with_associated_types(interface, &TypeId::SelfType(interface.declaration)),
                    targets
                        .into_iter()
                        .map(|target| match target {
                            ConstraintTarget::Trait(required) => {
                                let TypeId::Trait(required) = TypeId::Trait(required)
                                    .with_associated_types(
                                        interface,
                                        &TypeId::SelfType(interface.declaration),
                                    )
                                else {
                                    unreachable!()
                                };
                                ConstraintTarget::Trait(required)
                            }
                            target => target,
                        })
                        .collect(),
                )
            })
            .collect();
        self.bound_operations(&bounds, &substitution, span)
    }

    pub(crate) fn bound_operations(
        &mut self,
        bounds: &GenericBounds,
        substitution: &TypeSubstitution,
        span: Span,
    ) -> Result<Vec<OperationWitness>, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("generic bound operation environment");
        let mut requirements = vec![];
        for (receiver, constraints) in bounds {
            let receiver = receiver.instantiate(substitution);
            for constraint in constraints {
                let ConstraintTarget::Trait(interface) = constraint else {
                    continue;
                };
                let interface = interface.instantiate(substitution);
                let closure = self
                    .catalog
                    .trait_closure(&interface, &receiver, &self.options.cancel)
                    .map_err(|_| invalid())?;
                for interface in closure {
                    let contract = self
                        .catalog
                        .trait_(&interface.declaration)
                        .ok_or_else(invalid)?;
                    let mut sources = vec![(receiver.clone(), interface.clone())];
                    let mut applied = TypeSubstitution::default();
                    applied.insert_receiver(interface.declaration, receiver.clone());
                    for (parameter, argument) in
                        contract.generic_params.iter().zip(&interface.arguments)
                    {
                        applied.insert(parameter.clone(), argument.clone());
                    }
                    for (member, constraints) in &contract.associated_types {
                        if contract
                            .associated_type_parameters
                            .get(member)
                            .is_some_and(|family| !family.parameters.is_empty())
                        {
                            continue;
                        }
                        let projected = TypeId::Projection {
                            receiver: Box::new(receiver.clone()),
                            interface: Box::new(interface.clone()),
                            member: *member,
                            arguments: vec![],
                        }
                        .with_associated_types(&interface, &receiver);
                        for constraint in constraints {
                            let ConstraintTarget::Trait(required) = constraint else {
                                continue;
                            };
                            let TypeId::Trait(required) =
                                TypeId::Trait(required.instantiate(&applied))
                                    .with_associated_types(&interface, &receiver)
                            else {
                                unreachable!()
                            };
                            let closure = self
                                .catalog
                                .trait_closure(&required, &projected, &self.options.cancel)
                                .map_err(|_| invalid())?;
                            sources.extend(
                                closure
                                    .into_iter()
                                    .map(|interface| (projected.clone(), interface)),
                            );
                        }
                    }
                    for (receiver, interface) in sources {
                        let contract = self
                            .catalog
                            .trait_(&interface.declaration)
                            .ok_or_else(invalid)?;
                        for operation in &contract.methods {
                            let required = NativeCallableRequirement {
                                receiver: self.encode_type(&receiver),
                                interface: self.encode_nominal(&interface),
                                member: operation.id,
                                arguments: vec![],
                            };
                            if !requirements.contains(&required) {
                                requirements.push(required);
                            }
                        }
                    }
                }
            }
        }
        // HIR bounds are a hash map and their IDs are context-local. Order
        // selected entries by exact semantic content, not either storage order.
        let definitions = self.definitions();
        let type_order = |left: &Ty, right: &Ty| {
            left.canonical_cmp(right, &definitions, &Default::default())
                .expect("checked callable requirement types")
        };
        requirements.sort_by(|left, right| {
            type_order(&left.receiver, &right.receiver)
                .then_with(|| {
                    type_order(
                        &Ty::Trait(left.interface.clone()),
                        &Ty::Trait(right.interface.clone()),
                    )
                })
                .then_with(|| {
                    definitions
                        .compare(left.member, right.member)
                        .expect("checked callable member identities")
                })
                .then_with(|| {
                    left.arguments
                        .iter()
                        .zip(&right.arguments)
                        .map(|(left, right)| type_order(left, right))
                        .find(|order| !order.is_eq())
                        .unwrap_or_else(|| left.arguments.len().cmp(&right.arguments.len()))
                })
        });
        self.check()?;
        self.bind_operations(requirements, span)
    }

    pub(crate) fn enqueue_interface_method(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<(), MirLoweringError> {
        let owner = self
            .definitions()
            .parent(*declaration)
            .ok()
            .flatten()
            .ok_or(MirLoweringError::MissingBinding("shared method owner"))?;
        let conditional = self
            .registered_native_declaration(declaration)
            .zip(self.catalog.implementation_signature(&owner))
            .is_some_and(|(native, implementation)| {
                let outer =
                    checked_bounds(self.catalog, &self.definitions(), &implementation.bounds);
                native.function.bounds.iter().any(|bound| {
                    bound.constraints.iter().any(|constraint| {
                        !outer.iter().any(|assumed| {
                            assumed.ty == bound.ty && assumed.constraints.contains(constraint)
                        })
                    })
                })
            });
        let shared_receiver = arguments.iter().any(|argument| !argument.is_concrete())
            || (conditional && !arguments.is_empty());
        let canonical = arguments
            .iter()
            .enumerate()
            .map(|(position, argument)| {
                if shared_receiver {
                    TypeId::Generic(GenericParameterType {
                        owner: *declaration,
                        position,
                        name: format!("T{position}"),
                    })
                } else {
                    argument.clone()
                }
            })
            .collect::<Vec<_>>();
        let arguments = canonical.as_slice();
        if self
            .registered_native_declaration(declaration)
            .is_some_and(|declaration| {
                declaration.function.generic_params.len() > arguments.len()
                    && declaration
                        .function
                        .params
                        .first()
                        .is_none_or(|parameter| parameter.name != "self")
            })
        {
            return Ok(());
        }
        if self.prepare_native_target(declaration, arguments, span)? {
            return Ok(());
        }
        let parameters = if let Some((implementation, method)) = self
            .catalog
            .default_method(&self.definitions(), declaration)
        {
            let contract = self
                .catalog
                .trait_(&method.owner)
                .ok_or(MirLoweringError::MissingBinding("shared default trait"))?;
            if method
                .params
                .first()
                .is_none_or(|parameter| parameter.name != "self")
                && arguments.len()
                    < implementation.generic_params.len() + method.generic_params.len()
                        - contract.generic_params.len()
            {
                return Ok(());
            }
            let supplied = arguments
                .len()
                .checked_sub(implementation.generic_params.len())
                .ok_or(MirLoweringError::MissingBinding(
                    "shared default receiver arguments",
                ))?;
            method
                .generic_params
                .get(contract.generic_params.len() + supplied..)
                .ok_or(MirLoweringError::MissingBinding(
                    "shared default method arguments",
                ))?
                .to_vec()
        } else if let Some(function) = self.native_function(declaration) {
            function.generic_params[arguments.len()..].to_vec()
        } else {
            let Some(ResolvedName::Function(function)) =
                self.module.declarations.definition_target(declaration)
            else {
                return Err(MirLoweringError::MissingBinding(
                    "shared method declaration",
                ));
            };
            let typed = self
                .module
                .typed
                .functions
                .iter()
                .find(|typed| typed.id == function)
                .ok_or(MirLoweringError::MissingTypedFunction(function))?;
            if typed.generic_params.len() > arguments.len()
                && typed
                    .params
                    .first()
                    .is_none_or(|parameter| parameter.name != "self")
            {
                return Ok(());
            }
            typed
                .generic_params
                .get(arguments.len()..)
                .ok_or(MirLoweringError::MissingBinding(
                    "shared receiver arguments",
                ))?
                .to_vec()
        };
        let mut applied = arguments.to_vec();
        let offset = if shared_receiver { arguments.len() } else { 0 };
        applied.extend(parameters.iter().enumerate().map(|(position, parameter)| {
            TypeId::Generic(GenericParameterType {
                owner: *declaration,
                position: position + offset,
                name: parameter.name.clone(),
            })
        }));
        if !self.prepare_native_target(declaration, &applied, span)? {
            self.enqueue_declaration(declaration, applied, span)?;
        }
        Ok(())
    }
}

impl Instance {
    pub(crate) fn shared_body(
        &self,
        typed: &TypedFunction,
        catalog: &AggregateCatalog,
        definitions: &DefinitionTable,
    ) -> Option<GenericBody> {
        let parameters: Vec<_> = self
            .key
            .arguments
            .iter()
            .filter_map(|argument| {
                let TypeId::Generic(parameter) = argument else {
                    return None;
                };
                Some(GenericParam {
                    owner: parameter.owner,
                    position: parameter.position,
                })
            })
            .collect();
        if parameters.is_empty() {
            return None;
        }
        let bounds: GenericBounds = typed
            .bounds
            .iter()
            .map(|(ty, targets)| {
                (
                    ty.instantiate(&self.substitution),
                    targets
                        .iter()
                        .map(|target| match target {
                            ConstraintTarget::Standard(kind) => ConstraintTarget::Standard(*kind),
                            ConstraintTarget::Trait(interface) => {
                                ConstraintTarget::Trait(interface.instantiate(&self.substitution))
                            }
                        })
                        .collect(),
                )
            })
            .collect();
        Some(GenericBody {
            parameters,
            bounds: checked_bounds(catalog, definitions, &bounds),
        })
    }
}
