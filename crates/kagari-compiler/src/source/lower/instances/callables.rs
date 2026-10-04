//! Specialize declared native trait dependencies using the checked source catalog.
use crate::source::{
    lower::{MirLoweringError, instances::InstancePlanner},
    types::{raise_nominal_type, raise_type},
};
use kagari_common::{
    identity::{DefinitionPath, associated_type_id},
    span::Span,
};
use kagari_contract::{
    callable::witness::{OperationWitness, SharedMethodWitness},
    effects::EffectSet,
    native_import::{
        NativeImport,
        callables::{NativeCallableApplication, NativeCallableOrigin},
    },
    types::ConcreteFunctionIdentity,
};
use kagari_hir::{
    aggregates::traits::MethodDefault,
    native::NativeBinding,
    typeck::FunctionImplementation,
    types::{
        TypeId, TypeSubstitution as HirSubstitution,
        semantic::{lower_nominal_type, lower_type},
    },
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{NativeDeclaration, requirement::NativeCallableRequirement},
    language::Protocol,
    ty::substitution::TypeSubstitution,
};

impl InstancePlanner<'_> {
    pub(crate) fn registered_native_declaration(
        &self,
        declaration: &DefinitionPath,
    ) -> Option<&NativeDeclaration> {
        self.modules
            .get(&declaration.module)?
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
        let mut operations = vec![];
        for mut required in requirements {
            // A projection with concrete inputs can already have a selected
            // implementation. Normalize it before deciding whether to forward.
            required.receiver =
                lower_type(&self.catalog.normalize_type(&raise_type(&required.receiver)));
            let TypeId::Trait(interface) = self
                .catalog
                .normalize_type(&TypeId::Trait(raise_nominal_type(&required.interface)))
            else {
                return Err(MirLoweringError::MissingBinding(
                    "normalized operation interface",
                ));
            };
            required.interface = lower_nominal_type(&interface);
            required.arguments = required
                .arguments
                .iter()
                .map(|ty| lower_type(&self.catalog.normalize_type(&raise_type(ty))))
                .collect();
            if !matches!(raise_type(&required.receiver), TypeId::Generic(_))
                && required.arguments.is_empty()
                && self
                    .catalog
                    .trait_method(&required.member)
                    .is_some_and(|method| {
                        self.catalog
                            .trait_(&required.interface.declaration)
                            .is_some_and(|contract| {
                                method.generic_params.len() > contract.generic_params.len()
                            })
                    })
            {
                let invalid =
                    || MirLoweringError::MissingBinding("shared constraint method implementation");
                let (declaration, arguments) = self
                    .catalog
                    .concrete_interface_implementation(
                        &raise_nominal_type(&required.interface),
                        &raise_type(&required.receiver),
                        &Default::default(),
                        100_000,
                        64,
                        &self.options.cancel,
                    )
                    .map_err(|_| invalid())?
                    .ok_or_else(invalid)?;
                self.record_interface(&declaration, &arguments, span)?;
                operations.push(OperationWitness::SharedMethod(Box::new(
                    SharedMethodWitness {
                        requirement: required,
                        implementation: ConcreteFunctionIdentity {
                            declaration,
                            arguments: arguments.iter().map(lower_type).collect(),
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

    pub(crate) fn select_callables(
        &mut self,
        requirements: Vec<NativeCallableRequirement>,
        span: Span,
    ) -> Result<Vec<NativeCallableApplication>, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("checked callable requirement");
        let mut selected = vec![];
        for mut required in requirements {
            let receiver = self.catalog.normalize_type(&raise_type(&required.receiver));
            let TypeId::Trait(mut interface) = self
                .catalog
                .normalize_type(&TypeId::Trait(raise_nominal_type(&required.interface)))
            else {
                return Err(invalid());
            };
            if matches!(receiver, TypeId::Trait(_))
                && Protocol::from_id(&interface.declaration).is_some_and(Protocol::iteration)
            {
                for name in ["Item", "Iter"] {
                    if name == "Iter"
                        && Protocol::from_id(&interface.declaration) != Some(Protocol::Iterable)
                    {
                        continue;
                    }
                    let member = associated_type_id(&interface.declaration, name);
                    if !interface.associated_types.contains_key(&member) {
                        let output = self.catalog.normalize_type(&TypeId::Projection {
                            receiver: Box::new(receiver.clone()),
                            interface: Box::new(interface.clone()),
                            member: member.clone(),
                            arguments: vec![],
                        });
                        interface.associated_types.insert(member, output);
                    }
                }
            }
            required.receiver = lower_type(&receiver);
            required.interface = lower_nominal_type(&interface);
            required.arguments = required
                .arguments
                .iter()
                .map(|ty| lower_type(&self.catalog.normalize_type(&raise_type(ty))))
                .collect();
            let implementation = self
                .catalog
                .concrete_interface_implementation(
                    &interface,
                    &receiver,
                    &Default::default(),
                    100_000,
                    64,
                    &self.options.cancel,
                )
                .map_err(|_| invalid())?;
            let Some((implementation, arguments)) = implementation else {
                let application = self
                    .catalog
                    .implicit_protocol_application(
                        &required,
                        &receiver,
                        &interface,
                        &self.options.cancel,
                    )
                    .map_err(|_| invalid())?
                    .ok_or_else(invalid)?;
                let id =
                    self.enqueue_selected_protocol(application.kind, &receiver, &interface, span)?;
                let instance = self.instances[id.index()].key.lower(self.options, span)?;
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
                .or_else(|| {
                    default
                        .as_ref()
                        .map(|import| import.instance.declaration.clone())
                })
                .or_else(|| {
                    (signature.default == Some(MethodDefault::Script)).then(|| {
                        let mut target = implementation.clone();
                        target
                            .path
                            .push(required.member.path.last().unwrap().clone());
                        target
                    })
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
            substitution.insert_receiver(interface.declaration.clone(), receiver.clone());
            let normalize = |ty: &TypeId| {
                lower_type(
                    &self.catalog.normalize_type(
                        &ty.instantiate(&substitution)
                            .with_associated_types(&interface),
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
                CallableImplementation::Native(binding.clone())
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
                && target.module == *self.module.lowered.source.module_identity()
            {
                self.enqueue_declaration(&target, instance_arguments.clone(), span)?;
            }
            selected.push(NativeCallableApplication {
                origin: NativeCallableOrigin::Implementation,
                requirement: required,
                instance: ConcreteFunctionIdentity {
                    declaration: target,
                    arguments: instance_arguments.iter().map(lower_type).collect(),
                },
                implementation: kind,
                signature: applied,
                effects: EffectSet::native_call(),
            });
        }
        Ok(selected)
    }
}
