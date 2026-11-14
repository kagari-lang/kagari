//! Share applied interface descriptions while keeping receiver views independent.
pub(crate) mod cache;
pub(crate) mod description;
mod operations;
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::SessionId,
    execution_metadata::{environments::AdmittedEnvironment, managed::sources::MetadataSource},
    frame::types::{
        EnvironmentRecord, TypeEnvironment, arguments::TypeArgument,
        construction::ConstructionScope,
    },
    module::ExecutableModule,
    objects::{
        calls,
        interfaces::{
            cache::DescriptionKey,
            description::{
                InterfaceDescription, InterfaceMethodBinding, InterfaceParentBinding,
                InterfaceResultBinding,
            },
        },
    },
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::DefinitionKind;
use kagari_contract::types::{self as abi, PublicItem};
use kagari_types::ty::{Ty, substitution::TypeSubstitution};
use std::sync::Arc;

/// Checked table application facts. The enclosing source supplies all actual
/// program/environment backing; this borrowed request retains no graph on its own.
pub(crate) struct InterfaceRequest<'table> {
    pub(crate) implementation: &'table ExecutableModule,
    pub(crate) table: usize,
    pub(crate) arguments: &'table [TypeArgument],
    pub(crate) environment: Option<&'table TypeEnvironment>,
    pub(crate) view: bool,
}

pub(crate) struct PreparedInterface {
    pub(crate) description: Arc<InterfaceDescription>,
    pub(crate) environment: Option<AdmittedEnvironment>,
}

impl Runtime {
    pub(crate) fn prepare_interface_description(
        &mut self,
        execution: SessionId,
        request: InterfaceRequest<'_>,
        protected: &[MetadataSource<'_>],
    ) -> Result<PreparedInterface, RuntimeError> {
        let InterfaceRequest {
            implementation,
            table: table_index,
            arguments,
            environment: construction_environment,
            view: use_view,
        } = request;
        self.resources.poll_execution()?;
        let invalid = || RuntimeError::module_validation("invalid interface implementation table");
        self.validate_executable_module(implementation)?;
        for (index, argument) in arguments.iter().enumerate() {
            if index.is_multiple_of(1024) {
                self.resources.poll_execution()?;
            }
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
        if linked.arguments.iter().all(Ty::is_concrete)
            && !implementation
                .identity_admission()
                .equivalent(&linked.arguments, &concrete_arguments)
        {
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
        let parent = if template.generic_params.is_empty() {
            None
        } else {
            construction_environment
                .map(|environment| self.environment_snapshot_in(protected, environment))
                .transpose()?
        };
        let constructions = (!template.generic_params.is_empty()).then(|| {
            let mut scope = ConstructionScope::new(implementation);
            if let Some(parent) = &parent {
                scope.extend(&parent.record.constructions);
            }
            scope
        });
        let operations = parent
            .as_ref()
            .map(|parent| parent.record.operations().clone())
            .unwrap_or_default();
        let key = DescriptionKey {
            owner: implementation.key(),
            table: table_index,
            view: use_view,
            arguments: arguments
                .iter()
                .map(|argument| argument.identity(implementation).ok_or_else(invalid))
                .collect::<Result<_, _>>()?,
            operations: operations.identity(),
            constructions: constructions
                .as_ref()
                .map(|scope| scope.identity())
                .unwrap_or_default(),
        };
        if let Some(description) = self.interface_descriptions.get(&key) {
            match &description.environment {
                None => {
                    return Ok(PreparedInterface {
                        description,
                        environment: None,
                    });
                }
                Some(facts) => {
                    if let Some(environment) =
                        self.copy_environment_in(execution, facts, protected)?
                    {
                        return Ok(PreparedInterface {
                            description,
                            environment: Some(environment),
                        });
                    }
                    // The weak cache retained facts, not this environment's
                    // managed backing. Rebuild from the real incoming sources.
                }
            }
        }
        let closed_definitions = self.definition_context().snapshot();
        let applied_template = implementation.identity_admission().runtime_in(
            template,
            &closed_definitions,
            &Default::default(),
        )?;
        let table = applied_template
            .instantiate(&concrete_arguments, &[], &closed_definitions)
            .ok_or_else(invalid)?;
        let admitted_environment = if template.generic_params.is_empty() {
            None
        } else {
            let mut environment = EnvironmentRecord::new(
                implementation.definitions(),
                template.generic_params.clone(),
                arguments.to_vec(),
            )?;
            environment.set_constructions(constructions.expect("generic construction scope"));
            environment.extend_operations(operations);
            Some(self.alloc_environment(execution, environment, protected)?)
        };
        let environment = admitted_environment
            .as_ref()
            .map(|environment| environment.facts().clone());
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
            .map(|view| {
                let interface = substitution
                    .apply_nominal(&view.interface, &Default::default())
                    .map_err(|_| invalid())?;
                implementation.identity_admission().runtime_in(
                    &interface,
                    &closed_definitions,
                    &Default::default(),
                )
            })
            .transpose()
            .map_err(|_| invalid())?;
        let interface_type = view_interface.as_ref().unwrap_or(interface_type);
        let interface_declaration = implementation
            .identity_admission()
            .lexical_id(interface_type.declaration)
            .ok_or_else(invalid)?;
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
                    &interface_declaration,
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
        let mut methods = Vec::new();
        methods
            .try_reserve_exact(trait_contract.methods.len())
            .map_err(|_| RuntimeError::resource_limit("interface method facts"))?;
        for (index, declared) in trait_contract.methods.iter().enumerate() {
            if index.is_multiple_of(1024) {
                self.resources.poll_execution()?;
            }
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
                    interface_declaration,
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
                requires_receiver_operations: Self::receiver_operations_required(
                    implementation,
                    slot.target,
                ),
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
        let description = Arc::new(InterfaceDescription {
            receiver_table,
            parents: linked
                .parents
                .iter()
                .enumerate()
                .map(|(index, parent)| {
                    if index.is_multiple_of(1024) {
                        self.resources.poll_execution()?;
                    }
                    Ok(InterfaceParentBinding {
                        interface: implementation.identity_admission().runtime_in(
                            &substitution
                                .apply_nominal(&parent.interface, &Default::default())
                                .map_err(|_| invalid())?,
                            &closed_definitions,
                            &Default::default(),
                        )?,
                        binding: calls::interface_binding(
                            implementation,
                            &parent.implementation,
                            environment.clone(),
                        )?,
                        view: parent.view,
                    })
                })
                .collect::<Result<_, RuntimeError>>()?,
            concrete_type,
            concrete_expression: template.for_type.clone(),
            interface_type,
            interface_expression: match view {
                Some(view) => Ty::Trait(view.interface.clone()),
                None => match &template.trait_type {
                    Ty::Trait(_) => template.trait_type.clone(),
                    _ => return Err(invalid()),
                },
            },
            environment,
            implementation: implementation.clone(),
            methods,
        });
        self.interface_descriptions
            .insert(key, &description)
            .map_err(|_| RuntimeError::resource_limit("interface description cache"))?;
        Ok(PreparedInterface {
            description,
            environment: admitted_environment,
        })
    }
}

#[cfg(test)]
mod tests;
