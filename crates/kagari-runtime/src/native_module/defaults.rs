//! Derive default members from real template signatures and explicit binder roles.
use crate::{
    error::RuntimeError,
    native_module::{DefaultMember, Method, NativeModuleBuilder, invalid, nominal, types::Scope},
};
use kagari_abi::{
    callable::{CallableImplementation, MethodPolicy, NativeDefaultApplication},
    native_api::NativeModule,
    types::{AbiType, FunctionAbi, NominalAbiType, ParameterAbi, substitution::TypeSubstitution},
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};

impl NativeModuleBuilder {
    /// Declare all default members before resolving any selected dependencies.
    pub fn default_member(
        &mut self,
        names: &[&'static str],
        mapping: DefaultMember,
        template: &Method,
    ) -> Result<(), RuntimeError> {
        self.check_names(names)?;
        let declaration = self
            .module
            .definition(DefinitionKind::Function, template.name);
        let scope = Scope {
            module: &self.module,
            catalog: &self.catalog,
            owner: declaration.clone(),
            names,
            receiver: None,
            associated: &[],
        };
        let receiver = scope.resolve(&mapping.receiver)?;
        let interface = nominal(scope.resolve(&mapping.interface)?)?;
        let index = self
            .module
            .traits
            .iter()
            .position(|contract| {
                self.module
                    .definition(DefinitionKind::Trait, &contract.name)
                    == interface.declaration
            })
            .ok_or_else(|| invalid("native defaults must extend an owned trait"))?;
        let contract = &self.module.traits[index];
        if interface.arguments.len() != contract.generic_params.len()
            || contract
                .methods
                .iter()
                .any(|method| method.name == mapping.member)
        {
            return Err(invalid(
                "duplicate default member or invalid trait arguments",
            ));
        }
        let mut arguments = vec![None; names.len()];
        let mut bind = |source: &AbiType, target: AbiType| -> Result<(), RuntimeError> {
            let AbiType::Parameter { owner, position } = source else {
                return Err(invalid(
                    "default mappings require distinct template parameters",
                ));
            };
            if *owner != declaration
                || *position >= arguments.len()
                || arguments[*position].replace(target).is_some()
            {
                return Err(invalid("ambiguous default template parameter mapping"));
            }
            Ok(())
        };
        bind(&receiver, AbiType::SelfType(interface.declaration.clone()))?;
        for (argument, parameter) in interface.arguments.iter().zip(&contract.generic_params) {
            bind(argument, parameter.as_type())?;
        }
        for (member, argument) in &interface.associated_types {
            if !contract
                .associated_types
                .iter()
                .any(|declared| declared.declaration == *member)
            {
                return Err(invalid("default mapping names an absent associated type"));
            }
            bind(
                argument,
                AbiType::Projection {
                    receiver: Box::new(AbiType::SelfType(interface.declaration.clone())),
                    interface: Box::new(NominalAbiType {
                        declaration: interface.declaration.clone(),
                        arguments: contract
                            .generic_params
                            .iter()
                            .map(|parameter| parameter.as_type())
                            .collect(),
                        associated_types: Default::default(),
                    }),
                    member: member.clone(),
                    arguments: vec![],
                },
            )?;
        }
        let arguments = arguments
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| invalid("unmapped default template parameter"))?;
        let cancel = CancellationToken::default();
        let substitution = TypeSubstitution::for_owner(&declaration, &arguments);
        let apply = |expression| {
            substitution
                .apply(&scope.resolve(expression)?, &cancel)
                .map_err(|_| invalid("invalid default template substitution"))
        };
        let mut params = template
            .params
            .iter()
            .map(|(name, expression)| {
                Ok(ParameterAbi {
                    name: (*name).into(),
                    ty: apply(expression)?,
                    mutable: false,
                })
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let first = params
            .first_mut()
            .ok_or_else(|| invalid("native defaults require a receiver"))?;
        if first.ty != AbiType::SelfType(interface.declaration.clone()) {
            return Err(invalid(
                "native default template must take its receiver first",
            ));
        }
        first.name = "self".into();
        let return_type = apply(&template.result)?;
        let function = FunctionAbi {
            name: mapping.member.into(),
            implementation: CallableImplementation::NativeDefault(NativeDefaultApplication {
                declaration,
                arguments,
            }),
            method_policy: MethodPolicy {
                override_allowed: !mapping.final_method,
            },
            generic_params: vec![],
            bounds: vec![],
            params,
            return_type,
        };
        self.module.traits[index].methods.push(function);
        self.document(
            NativeModule::method_id(&interface.declaration, mapping.member),
            template.documentation,
        );
        Ok(())
    }

    /// Register the ordinary executable template without exposing a public helper.
    pub fn default_template(
        &mut self,
        names: &[&'static str],
        template: Method,
    ) -> Result<(), RuntimeError> {
        let declaration = self
            .module
            .definition(DefinitionKind::Function, template.name);
        self.free_function(names, template)?;
        self.module.private_functions.insert(declaration);
        Ok(())
    }
}
