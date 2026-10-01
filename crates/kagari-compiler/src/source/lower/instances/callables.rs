//! Specialize declared native trait dependencies using the checked source catalog.
use crate::source::{
    lower::{MirLoweringError, instances::InstancePlanner},
    types::{raise_nominal_type, raise_type},
};
use kagari_abi::{
    callable::CallableImplementation,
    effects::EffectSet,
    native_import::{NativeImport, NativeSignature, callables::NativeCallableApplication},
    types::{ConcreteFunctionIdentity, NativeDeclaration, substitution::TypeSubstitution},
};
use kagari_common::{identity::DefinitionId, span::Span};
use kagari_hir::{
    aggregates::traits::MethodDefault,
    native::NativeBinding,
    typeck::FunctionImplementation,
    types::{
        TypeId, TypeSubstitution as HirSubstitution,
        abi::{lower_nominal_type, lower_type},
    },
};

impl InstancePlanner<'_> {
    pub(super) fn registered_native_declaration(
        &self,
        declaration: &DefinitionId,
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
        import: &NativeImport,
        span: Span,
    ) -> Result<Vec<NativeCallableApplication>, MirLoweringError> {
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
        let mut selected = vec![];
        for mut required in requirements {
            let receiver = self.catalog.normalize_type(&raise_type(&required.receiver));
            let TypeId::Trait(interface) = self
                .catalog
                .normalize_type(&TypeId::Trait(raise_nominal_type(&required.interface)))
            else {
                return Err(invalid());
            };
            required.receiver = lower_type(&receiver);
            required.interface = lower_nominal_type(&interface);
            required.arguments = required
                .arguments
                .iter()
                .map(|ty| lower_type(&self.catalog.normalize_type(&raise_type(ty))))
                .collect();
            let (implementation, arguments) = self
                .catalog
                .concrete_interface_implementation(
                    &interface,
                    &receiver,
                    &Default::default(),
                    100_000,
                    64,
                    &self.options.cancel,
                )
                .map_err(|_| invalid())?
                .ok_or_else(invalid)?;
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
            let target = contract
                .methods
                .get(&required.member)
                .cloned()
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
            let applied = NativeSignature {
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
            let instance_arguments = arguments
                .iter()
                .cloned()
                .chain(required.arguments.iter().map(raise_type))
                .collect::<Vec<_>>();
            self.record_interface(&implementation, &arguments, span)?;
            self.record_layout_root(&receiver, &Default::default(), span)?;
            if kind == CallableImplementation::Script
                && target.module == *self.module.lowered.source.module_identity()
            {
                self.enqueue_declaration(&target, instance_arguments.clone(), span)?;
            }
            selected.push(NativeCallableApplication {
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
