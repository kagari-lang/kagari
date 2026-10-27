//! Share applied interface descriptions while keeping receiver views independent.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{interfaces::InterfaceDescriptionKey, links::MetadataCache},
    frame::types::{EnvironmentRecord, TypeEnvironment, arguments::TypeArgument},
    gc::interfaces::{
        InterfaceDescription, InterfaceMethodBinding, InterfaceParentBinding,
        InterfaceResultBinding, InterfaceValueSnapshot,
    },
    module::ExecutableModule,
    objects::calls,
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::DefinitionKind;
use kagari_contract::types::{self as abi, PublicItem};
use kagari_types::ty::{Ty, substitution::TypeSubstitution};

impl Runtime {
    pub(super) fn prepare_interface_snapshot(
        &self,
        implementation: &ExecutableModule,
        table_index: usize,
        arguments: &[TypeArgument],
        construction_environment: Option<&TypeEnvironment>,
        data: Value,
        use_view: bool,
    ) -> Result<InterfaceValueSnapshot, RuntimeError> {
        let invalid = || RuntimeError::module_validation("invalid interface implementation table");
        if !implementation.belongs_to(self.host.owner()) {
            return Err(invalid());
        }
        for argument in arguments {
            argument.validate(self)?;
        }
        let concrete_arguments = arguments
            .iter()
            .map(|argument| argument.ty().clone())
            .collect::<Vec<_>>();
        let linked = implementation
            .bytecode
            .interface_tables
            .get(table_index)
            .ok_or_else(invalid)?;
        if linked.arguments.iter().all(Ty::is_concrete) && linked.arguments != concrete_arguments {
            return Err(invalid());
        }
        let template = implementation
            .bytecode
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                    Some(table.as_ref())
                }
                _ => None,
            })
            .ok_or_else(invalid)?;
        let constructions = if template.generic_params.is_empty() {
            None
        } else {
            Some(self.construction_scope(implementation, construction_environment)?)
        };
        let key = InterfaceDescriptionKey {
            owner: implementation.key(),
            table: table_index,
            view: use_view,
            arguments: arguments
                .iter()
                .map(|argument| argument.identity(implementation).ok_or_else(invalid))
                .collect::<Result<_, _>>()?,
            constructions: constructions
                .as_ref()
                .map(|scope| scope.identity())
                .unwrap_or_default(),
        };
        if let Some(description) = self.gc.interface_description(&key) {
            return Ok(InterfaceValueSnapshot::new(description, data));
        }
        let table = template
            .instantiate(&concrete_arguments, &[], implementation.definitions())
            .ok_or_else(invalid)?;
        let environment = if template.generic_params.is_empty() {
            None
        } else {
            let mut environment = EnvironmentRecord::new(
                self.definition_context(),
                template.generic_params.clone(),
                arguments.to_vec(),
            )?;
            environment.set_constructions(constructions.expect("generic construction scope"));
            if let Some(parent) = construction_environment {
                environment.extend_operations(
                    self.gc
                        .environment(parent.id)
                        .ok_or_else(invalid)?
                        .operations()
                        .clone(),
                );
            }
            Some(self.alloc_environment(environment)?)
        };
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in template.generic_params.iter().zip(&concrete_arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let concrete_type = table.for_type.clone();
        if !concrete_type.is_concrete() {
            return Err(invalid());
        }
        let Ty::Trait(interface_type) = &table.trait_type else {
            return Err(invalid());
        };
        if !table.trait_type.is_concrete() {
            return Err(invalid());
        }
        let view = if use_view {
            Some(linked.view.as_ref().ok_or_else(invalid)?)
        } else {
            None
        };
        let view_interface = view
            .map(|view| substitution.apply_nominal(&view.interface, &Default::default()))
            .transpose()
            .map_err(|_| invalid())?;
        let interface_type = view_interface.as_ref().unwrap_or(interface_type);
        let module = implementation
            .definition(interface_type.declaration)?
            .module();
        let trait_contract = implementation
            .members()
            .find(|member| &member.bytecode.identity == module)
            .and_then(|member| {
                abi::trait_contract(
                    &member.bytecode.identity,
                    &member.bytecode.public_items,
                    &member.bytecode.trait_contracts,
                    &interface_type.declaration,
                    member.definitions(),
                )
                .cloned()
            })
            .ok_or_else(invalid)?;
        if table.methods.iter().any(|method| {
            !trait_contract
                .methods
                .iter()
                .any(|declared| declared.name == method.name)
        }) {
            return Err(invalid());
        }
        let mut methods = Vec::with_capacity(trait_contract.methods.len());
        for declared in &trait_contract.methods {
            let method = template
                .methods
                .iter()
                .find(|method| method.name == declared.name);
            let Some(method) = method else {
                return Err(invalid());
            };
            let method_id = implementation
                .definitions()
                .lookup_child(
                    interface_type.declaration,
                    DefinitionKind::Method,
                    &method.name,
                    0,
                )
                .ok_or_else(invalid)?;
            let mut candidates = linked
                .methods
                .iter()
                .filter(|slot| slot.method == method_id);
            let Some(slot) = candidates.next() else {
                return Err(invalid());
            };
            if candidates.next().is_some() {
                return Err(invalid());
            }
            let result_adapter = view
                .and_then(|view| {
                    view.results
                        .iter()
                        .find(|adapter| adapter.method == method_id)
                })
                .map(|adapter| {
                    calls::interface_binding(
                        implementation,
                        &adapter.implementation,
                        environment.clone(),
                    )
                })
                .transpose()?;
            methods.push(Some(InterfaceMethodBinding {
                identity: Default::default(),
                receiver_operations: MetadataCache::new(),
                parameters: method.generic_params.clone(),
                entry_parameters: match slot.target {
                    CallableTarget::Script(target) => implementation.bytecode.functions
                        [target.index()]
                    .metadata
                    .semantic
                    .generic
                    .as_ref(),
                    CallableTarget::Native(target) => implementation.bytecode.native_imports
                        [target.index()]
                    .generic
                    .as_ref(),
                }
                .map(|body| body.parameters.clone())
                .unwrap_or_default(),
                entry_arguments: slot.arguments.clone(),
                result_adapter,
                method: method_id,
                target: slot.target,
                parameter_types: method.params.iter().map(|param| param.ty.clone()).collect(),
                return_type: method.return_type.clone(),
            }));
        }
        let receiver_table = InterfaceResultBinding {
            owner: implementation.clone(),
            table: table_index,
            arguments: template
                .generic_params
                .iter()
                .map(|parameter| parameter.as_type())
                .collect(),
            environment: environment.clone(),
        };
        let interface_type = Ty::Trait(interface_type.clone());
        if !interface_type.is_concrete() {
            return Err(invalid());
        }
        let parents = linked
            .parents
            .iter()
            .map(|parent| {
                Ok(InterfaceParentBinding {
                    interface: substitution
                        .apply_nominal(&parent.interface, &Default::default())
                        .map_err(|_| invalid())?,
                    binding: calls::interface_binding(
                        implementation,
                        &parent.implementation,
                        environment.clone(),
                    )?,
                    view: parent.view,
                })
            })
            .collect::<Result<_, RuntimeError>>()?;
        let interface_expression = match view {
            Some(view) => Ty::Trait(view.interface.clone()),
            None => match &template.trait_type {
                Ty::Trait(_) => template.trait_type.clone(),
                _ => return Err(invalid()),
            },
        };
        let description = self
            .gc
            .admit_interface_description(self.modules(), |scan_id| InterfaceDescription {
                scan_id,
                receiver_operations: MetadataCache::new(),
                receiver_table,
                parents,
                concrete_type,
                concrete_expression: template.for_type.clone(),
                interface_type,
                interface_expression,
                environment,
                implementation: implementation.clone(),
                methods,
            })?;
        self.gc.cache_interface_description(key, &description)?;
        Ok(InterfaceValueSnapshot::new(description, data))
    }
}
