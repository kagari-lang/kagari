//! Shared script entries retain method binders while specializing the receiver.
use crate::source::lower::{
    MirLoweringError,
    abi::checked_bounds,
    instances::{Instance, InstancePlanner},
};
use kagari_common::{identity::DefinitionPath, span::Span};
use kagari_contract::{
    callable::{generic::GenericBody, witness::OperationWitness},
    native_import::callables::NativeCallableRequirement,
    types::GenericParam,
};
use kagari_hir::{
    resolver::resolved::ResolvedName,
    typeck::{GenericBounds, TypedFunction, table::ConstraintTarget},
    types::{
        GenericParameterType, NominalType, TypeId, TypeSubstitution,
        abi::{lower_nominal_type, lower_type},
    },
};

impl InstancePlanner<'_> {
    /// Supply the member operations promised by receiver and method bounds.
    /// Receiver implementations may differ, so this follows the declaration's
    /// bounds rather than inspecting one selected default body.
    pub(crate) fn method_operations(
        &mut self,
        method: &DefinitionPath,
        interface: &NominalType,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<Vec<OperationWitness>, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("generic method operation environment");
        let signature = self.catalog.trait_method(method).ok_or_else(invalid)?;
        let trait_ = self
            .catalog
            .trait_(&interface.declaration)
            .ok_or_else(invalid)?;
        let substitution: TypeSubstitution = signature
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().chain(arguments).cloned())
            .collect();
        let mut bounds = trait_.bounds.clone();
        for (ty, constraints) in &signature.bounds {
            bounds
                .entry(ty.clone())
                .or_default()
                .extend(constraints.iter().cloned());
        }
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
                    for operation in &contract.methods {
                        if operation
                            .params
                            .first()
                            .is_none_or(|param| param.name != "self")
                        {
                            continue;
                        }
                        let required = NativeCallableRequirement {
                            receiver: lower_type(&receiver),
                            interface: lower_nominal_type(&interface),
                            member: operation.id.clone(),
                            arguments: vec![],
                        };
                        if !requirements.contains(&required) {
                            requirements.push(required);
                        }
                    }
                }
            }
        }
        // HIR bounds are a hash map. Keep selected entry order and artifact bytes
        // independent of its iteration order.
        requirements.sort_by(|left, right| {
            (
                &left.receiver,
                &left.interface,
                &left.member,
                &left.arguments,
            )
                .cmp(&(
                    &right.receiver,
                    &right.interface,
                    &right.member,
                    &right.arguments,
                ))
        });
        self.bind_operations(requirements, span)
    }

    pub(crate) fn enqueue_interface_method(
        &mut self,
        declaration: &DefinitionPath,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<(), MirLoweringError> {
        let mut owner = declaration.clone();
        owner.path.pop();
        let conditional = self
            .registered_native_declaration(declaration)
            .zip(self.catalog.implementation_signature(&owner))
            .is_some_and(|(native, implementation)| {
                let outer = checked_bounds(&implementation.bounds);
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
                        owner: declaration.clone(),
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
        let parameters =
            if let Some((implementation, method)) = self.catalog.default_method(declaration) {
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
                owner: declaration.clone(),
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
    pub(crate) fn shared_body(&self, typed: &TypedFunction) -> Option<GenericBody> {
        let parameters: Vec<_> = self
            .key
            .arguments
            .iter()
            .filter_map(|argument| {
                let TypeId::Generic(parameter) = argument else {
                    return None;
                };
                Some(GenericParam {
                    owner: parameter.owner.clone(),
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
            bounds: checked_bounds(&bounds),
        })
    }
}
