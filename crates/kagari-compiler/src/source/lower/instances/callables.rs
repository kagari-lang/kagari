//! Specialize declared native trait dependencies using the checked source catalog.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::{
    identity::{DefinitionKind, table::DefinitionId},
    span::Span,
};
use kagari_contract::{
    callable::{
        adapter::SharedAdapterWitness,
        witness::{OperationWitness, SharedMethodWitness},
    },
    effects::EffectSet,
    native_import::{
        NativeImport,
        callables::{NativeCallableApplication, NativeCallableOrigin},
    },
    types::ConcreteFunctionIdentity,
};
use kagari_hir::{
    aggregates::{implementations::ImplementationSearchLimits, traits::MethodDefault},
    native::NativeBinding,
    typeck::FunctionImplementation,
    types::{
        TypeId, TypeSubstitution as HirSubstitution,
        semantic::{raise_nominal_type, raise_type},
    },
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{NativeDeclaration, requirement::NativeCallableRequirement},
    language::Protocol,
    ty::{Ty, substitution::TypeSubstitution},
};

impl InstancePlanner<'_> {
    pub(crate) fn registered_native_declaration(
        &self,
        declaration: &DefinitionId,
    ) -> Option<&NativeDeclaration> {
        self.modules
            .get(&self.module_identity(*declaration))?
            .lowered
            .registered_native_declarations()
            .iter()
            .find(|item| &item.declaration == declaration)
    }

    pub(crate) fn native_callables(
        &mut self,
        import: &mut NativeImport,
        span: Span,
    ) -> Result<Vec<OperationWitness>, MirLoweringError> {
        import.result_adapter = self.native_result_adapter(import, span)?;
        let Some(declaration) = self
            .registered_native_declaration(&import.instance.declaration)
            .cloned()
        else {
            return Ok(vec![]);
        };
        let invalid = || MirLoweringError::MissingBinding("checked native callable requirement");
        let mut bindings = TypeSubstitution::default();
        for (param, argument) in declaration
            .function
            .generic_params
            .iter()
            .zip(&import.instance.arguments)
        {
            bindings.bind(&param.owner, param.position, argument);
        }
        let requirements = declaration
            .callable_requirements
            .iter()
            .map(|required| {
                required
                    .apply(&bindings, &self.options.cancel)
                    .map_err(|_| invalid())
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.bind_operations(requirements, span)
    }

    pub(crate) fn bind_operations(
        &mut self,
        requirements: Vec<NativeCallableRequirement>,
        span: Span,
    ) -> Result<Vec<OperationWitness>, MirLoweringError> {
        if self.operation_depth >= 64 {
            return Err(MirLoweringError::MissingBinding(
                "operation composition depth",
            ));
        }
        self.operation_depth += 1;
        let result = self.bind_operations_inner(requirements, span);
        self.operation_depth -= 1;
        result
    }

    fn bind_operations_inner(
        &mut self,
        requirements: Vec<NativeCallableRequirement>,
        span: Span,
    ) -> Result<Vec<OperationWitness>, MirLoweringError> {
        let mut operations = vec![];
        for mut required in requirements {
            required = self.normalize_callable_requirement(required)?;
            if matches!(&required.receiver, Ty::Tuple(items) if !items.is_empty())
                && let Some(protocol @ (Protocol::PartialEq | Protocol::Hash)) =
                    Protocol::from_definition(&required.interface.declaration, &self.definitions())
            {
                let function = self.enqueue_selected_protocol(
                    protocol,
                    &raise_type(&required.receiver),
                    &raise_nominal_type(&required.interface),
                    span,
                    true,
                )?;
                let call = self.tuple_protocol_call(function, required.clone(), span)?;
                operations.push(OperationWitness::SharedAdapter(Box::new(
                    SharedAdapterWitness {
                        requirement: required,
                        call,
                    },
                )));
                continue;
            }

            // A nominal receiver containing binders can still have a selected
            // implementation. Forward only when that table is unavailable.
            if required.arguments.is_empty()
                && (!required.receiver.is_concrete()
                    || self
                        .catalog
                        .trait_method(&required.member)
                        .is_some_and(|method| {
                            self.catalog
                                .trait_(&required.interface.declaration)
                                .is_some_and(|contract| {
                                    method.generic_params.len() > contract.generic_params.len()
                                })
                        }))
            {
                let invalid =
                    || MirLoweringError::MissingBinding("shared constraint method implementation");
                let implementation = self
                    .catalog
                    .concrete_interface_implementation(
                        &self.definitions(),
                        &raise_nominal_type(&required.interface),
                        &raise_type(&required.receiver),
                        &Default::default(),
                        ImplementationSearchLimits {
                            max_checks: 100_000,
                            max_depth: 64,
                        },
                        &self.options.cancel,
                    )
                    .map_err(|_| invalid())?;
                let implementation = match implementation {
                    Some(implementation) => Some(implementation),
                    None if !required.receiver.is_concrete() => {
                        self.operation_candidate(&required)?
                    }
                    None => None,
                };
                let Some((declaration, arguments)) = implementation else {
                    if !required.receiver.is_concrete() {
                        operations.push(OperationWitness::Forward(Box::new(required)));
                        continue;
                    }
                    return Err(invalid());
                };
                let dependencies =
                    self.implementation_operations(&declaration, &arguments, span)?;
                self.record_interface(&declaration, &arguments, span)?;
                operations.push(OperationWitness::SharedMethod(Box::new(
                    SharedMethodWitness {
                        operations: dependencies,
                        requirement: required,
                        implementation: ConcreteFunctionIdentity {
                            declaration,
                            arguments: arguments.iter().map(|ty| self.encode_type(ty)).collect(),
                        },
                    },
                )));
            } else if !required.receiver.is_concrete()
                || !required
                    .interface
                    .arguments
                    .iter()
                    .all(|ty| ty.is_concrete())
                || !required
                    .interface
                    .associated_types
                    .values()
                    .all(|ty| ty.is_concrete())
                || !required.arguments.iter().all(|ty| ty.is_concrete())
            {
                operations.push(OperationWitness::Forward(Box::new(required)));
            } else {
                operations.extend(
                    self.select_callables(vec![required], span)?
                        .into_iter()
                        .map(|selected| OperationWitness::Selected(Box::new(selected))),
                );
            }
        }
        Ok(operations)
    }

    fn normalize_callable_requirement(
        &self,
        mut required: NativeCallableRequirement,
    ) -> Result<NativeCallableRequirement, MirLoweringError> {
        let receiver = self
            .catalog
            .normalize_type(&self.definitions(), &raise_type(&required.receiver));
        let TypeId::Trait(mut interface) = self.catalog.normalize_type(
            &self.definitions(),
            &TypeId::Trait(raise_nominal_type(&required.interface)),
        ) else {
            return Err(MirLoweringError::MissingBinding(
                "normalized callable interface",
            ));
        };
        if matches!(receiver, TypeId::Trait(_))
            && Protocol::from_definition(&interface.declaration, &self.definitions())
                .is_some_and(Protocol::iteration)
        {
            for name in ["Item", "Iter"] {
                if name == "Iter"
                    && Protocol::from_definition(&interface.declaration, &self.definitions())
                        != Some(Protocol::Iterable)
                {
                    continue;
                }
                let member = self
                    .definitions()
                    .lookup_child(
                        interface.declaration,
                        DefinitionKind::AssociatedType,
                        name,
                        0,
                    )
                    .expect("registered associated member");
                if !interface.associated_types.contains_key(&member) {
                    let output = self.catalog.normalize_type(
                        &self.definitions(),
                        &TypeId::Projection {
                            receiver: Box::new(receiver.clone()),
                            interface: Box::new(interface.clone()),
                            member,
                            arguments: vec![],
                        },
                    );
                    interface.associated_types.insert(member, output);
                }
            }
        }
        required.receiver = self.encode_type(&receiver);
        required.interface = self.encode_nominal(&interface);
        required.arguments = required
            .arguments
            .iter()
            .map(|ty| {
                self.encode_type(
                    &self
                        .catalog
                        .normalize_type(&self.definitions(), &raise_type(ty)),
                )
            })
            .collect();
        Ok(required)
    }

    pub(crate) fn select_callables(
        &mut self,
        requirements: Vec<NativeCallableRequirement>,
        span: Span,
    ) -> Result<Vec<NativeCallableApplication>, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("checked callable requirement");
        let mut selected = vec![];
        for mut required in requirements {
            required = self.normalize_callable_requirement(required)?;
            let receiver = raise_type(&required.receiver);
            let interface = raise_nominal_type(&required.interface);
            let implementation = self
                .catalog
                .concrete_interface_implementation(
                    &self.definitions(),
                    &interface,
                    &receiver,
                    &Default::default(),
                    ImplementationSearchLimits {
                        max_checks: 100_000,
                        max_depth: 64,
                    },
                    &self.options.cancel,
                )
                .map_err(|_| invalid())?;
            let Some((implementation, arguments)) = implementation else {
                if self
                    .catalog
                    .trait_method(&required.member)
                    .is_some_and(|method| {
                        matches!(
                            method.default,
                            Some(MethodDefault::Native(NativeBinding::Default(_)))
                        )
                    })
                {
                    let import = self.native_default_import(
                        &receiver,
                        &interface,
                        &required.member,
                        &required
                            .arguments
                            .iter()
                            .map(raise_type)
                            .collect::<Vec<_>>(),
                        span,
                    )?;
                    let target = self
                        .registered_native_declaration(&import.instance.declaration)
                        .ok_or_else(invalid)?;
                    let implementation = target.function.implementation.clone();
                    self.record_layout_root(&receiver, &Default::default(), span)?;
                    selected.push(NativeCallableApplication {
                        origin: NativeCallableOrigin::Implementation,
                        requirement: required,
                        instance: import.instance,
                        implementation,
                        signature: import.signature,
                        effects: EffectSet::native_call(),
                    });
                    continue;
                }
                let application = self
                    .catalog
                    .implicit_protocol_application(
                        &self.definitions(),
                        &required,
                        &receiver,
                        &interface,
                        &self.options.cancel,
                    )
                    .map_err(|_| invalid())?
                    .ok_or_else(invalid)?;
                let id = self.enqueue_selected_protocol(
                    application.kind,
                    &receiver,
                    &interface,
                    span,
                    false,
                )?;
                let instance = self.instances[id.index()].key.lower(
                    self.catalog,
                    self.options,
                    &self.definitions(),
                    span,
                )?;
                selected.push(NativeCallableApplication {
                    origin: NativeCallableOrigin::ProtocolAdapter,
                    requirement: application.requirement,
                    instance,
                    implementation: CallableImplementation::Script,
                    signature: application.signature,
                    effects: EffectSet::native_call(),
                });
                continue;
            };
            let contract = self
                .catalog
                .implementation_signature(&implementation)
                .ok_or_else(invalid)?;
            let signature = self
                .catalog
                .trait_method(&required.member)
                .ok_or_else(invalid)?;
            let trait_ = self
                .catalog
                .trait_(&interface.declaration)
                .ok_or_else(invalid)?;
            let default = if !contract.methods.contains_key(&required.member)
                && matches!(
                    signature.default,
                    Some(MethodDefault::Native(NativeBinding::Default(_)))
                ) {
                Some(
                    self.native_default_import(
                        &receiver,
                        &interface,
                        &required.member,
                        &required
                            .arguments
                            .iter()
                            .map(raise_type)
                            .collect::<Vec<_>>(),
                        span,
                    )?,
                )
            } else {
                None
            };
            let target = contract
                .methods
                .get(&required.member)
                .cloned()
                .or_else(|| default.as_ref().map(|import| import.instance.declaration))
                .or_else(|| {
                    if signature.default != Some(MethodDefault::Script) {
                        return None;
                    }
                    let definitions = self.definitions();
                    let member = definitions.resolve(required.member).ok()?.last()?;
                    definitions.lookup_child(
                        implementation,
                        member.kind,
                        member.name,
                        member.occurrence,
                    )
                })
                .ok_or_else(invalid)?;
            let own = &signature.generic_params[trait_.generic_params.len()..];
            if own.len() != required.arguments.len() {
                return Err(invalid());
            }
            let mut substitution: HirSubstitution = trait_
                .generic_params
                .iter()
                .cloned()
                .zip(interface.arguments.iter().cloned())
                .chain(
                    own.iter()
                        .cloned()
                        .zip(required.arguments.iter().map(raise_type)),
                )
                .collect();
            substitution.insert_receiver(interface.declaration, receiver.clone());
            let normalize = |ty: &TypeId| {
                self.encode_type(
                    &self.catalog.normalize_type(
                        &self.definitions(),
                        &ty.instantiate(&substitution)
                            .with_associated_types(&interface, &receiver),
                    ),
                )
            };
            let applied = Signature {
                params: signature
                    .params
                    .iter()
                    .map(|param| normalize(&param.ty))
                    .collect(),
                result: normalize(&signature.return_type),
            };
            let kind = if let Some(function) = self.native_function(&target) {
                let FunctionImplementation::Native(NativeBinding::Entry(binding)) =
                    &function.implementation
                else {
                    return Err(invalid());
                };
                CallableImplementation::Native(*binding)
            } else {
                CallableImplementation::Script
            };
            let instance_arguments = if let Some(import) = &default {
                if import.signature != applied {
                    return Err(invalid());
                }
                import
                    .instance
                    .arguments
                    .iter()
                    .map(raise_type)
                    .collect::<Vec<_>>()
            } else {
                arguments
                    .iter()
                    .cloned()
                    .chain(required.arguments.iter().map(raise_type))
                    .collect::<Vec<_>>()
            };
            self.record_interface(&implementation, &arguments, span)?;
            self.record_layout_root(&receiver, &Default::default(), span)?;
            if kind == CallableImplementation::Script
                && self.module_identity(target) == *self.module.lowered.source.module_identity()
            {
                self.enqueue_declaration(&target, instance_arguments.clone(), span)?;
            }
            selected.push(NativeCallableApplication {
                origin: NativeCallableOrigin::Implementation,
                requirement: required,
                instance: ConcreteFunctionIdentity {
                    declaration: target,
                    arguments: instance_arguments
                        .iter()
                        .map(|ty| self.encode_type(ty))
                        .collect(),
                },
                implementation: kind,
                signature: applied,
                effects: EffectSet::native_call(),
            });
        }
        Ok(selected)
    }
}
