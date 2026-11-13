//! Bind checked shared protocol calls without resolving trait implementations.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::SessionId,
    execution_metadata::{
        applications::{ApplicationRecord, MethodApplication},
        groups::AdmittedOperationGroup,
        managed::sources::MetadataSource,
        operation::BoundOperation,
    },
    frame::types::{
        EnvironmentRecord, TypeEnvironment,
        arguments::ScopedSignature,
        compatibility::{TypeIdentity, TypeView},
    },
    module::ExecutableModule,
    objects::calls::resolve_requirement,
};
use kagari_bytecode::module::{BytecodeFunction, CallableTarget};
use kagari_contract::callable::adapter::SharedAdapterWitness;
use kagari_types::{callable::Signature, declaration::requirement::NativeCallableRequirement};
use std::{slice, sync::Arc};

pub(super) struct AdapterApplication {
    pub(super) environment: EnvironmentRecord,
    pub(super) requirement: NativeCallableRequirement,
    pub(super) receiver_identity: Arc<TypeIdentity>,
}

impl Runtime {
    pub(super) fn bind_shared_adapter(
        &mut self,
        execution: SessionId,
        caller: &ExecutableModule,
        environment: Option<TypeEnvironment>,
        adapter: &SharedAdapterWitness,
        protected: &mut Vec<MetadataSource<'_>>,
    ) -> Result<AdmittedOperationGroup, RuntimeError> {
        let invalid = || RuntimeError::module_validation("shared structural operation application");
        let module = caller
            .definition(adapter.call.instance.declaration)?
            .module();
        let owner = caller
            .members()
            .find(|owner| &owner.bytecode.identity == module)
            .ok_or_else(invalid)?;
        let target = owner
            .bytecode
            .functions
            .iter()
            .find(|function| function.identity.as_ref() == Some(&adapter.call.instance))
            .ok_or_else(invalid)?;
        let body = target
            .metadata
            .semantic
            .generic
            .as_ref()
            .ok_or_else(invalid)?;
        let operations = self.bind_operations_in(
            execution,
            caller,
            environment.clone(),
            &adapter.call.operations,
            protected,
        )?;
        let arguments = self.type_arguments(
            caller,
            environment.as_ref().map(|env| env.types.clone()),
            &adapter.call.arguments,
        )?;
        let mut applied =
            EnvironmentRecord::new(owner.definitions(), body.parameters.clone(), arguments)?;
        applied.set_constructions(self.bind_constructions_in(
            execution,
            caller,
            environment.clone(),
            &adapter.call.constructions,
            protected,
        )?);
        applied.extend_operations(operations);
        let requirement = resolve_requirement(environment.as_ref(), &adapter.requirement)?;
        let receiver_identity = Arc::new(
            TypeView::new(
                &adapter.requirement.receiver,
                caller,
                environment.as_ref().map(|env| env.types.as_ref()),
            )
            .identity()
            .ok_or_else(invalid)?,
        );
        self.bind_adapter_application(
            execution,
            &owner,
            target,
            AdapterApplication {
                environment: applied,
                requirement,
                receiver_identity,
            },
            protected,
        )
    }

    /// Instantiate a verified structural body with separately bound component
    /// operations. Resolve their providers from actual protected inputs; the
    /// result belongs to the enclosing preparation's execution suffix.
    pub(super) fn bind_adapter_application(
        &mut self,
        execution: SessionId,
        owner: &ExecutableModule,
        target: &BytecodeFunction,
        application: AdapterApplication,
        protected: &[MetadataSource<'_>],
    ) -> Result<AdmittedOperationGroup, RuntimeError> {
        let invalid = || RuntimeError::module_validation("structural operation application");
        let AdapterApplication {
            environment,
            requirement,
            receiver_identity,
        } = application;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(
                protected
                    .len()
                    .checked_add(2)
                    .ok_or_else(|| RuntimeError::resource_limit("adapter metadata sources"))?,
            )
            .map_err(|_| RuntimeError::resource_limit("adapter metadata sources"))?;
        sources.extend_from_slice(protected);
        let applied = self.alloc_environment(execution, environment, &sources)?;
        sources.push(MetadataSource::Environment(applied.source()));
        let parameters = (0..target.metadata.params.len())
            .map(|index| target.metadata.semantic.params.get(&index).cloned())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(invalid)?;
        let params =
            self.type_arguments(owner, Some(applied.facts().types.clone()), &parameters)?;
        let result = self
            .type_arguments(
                owner,
                Some(applied.facts().types.clone()),
                slice::from_ref(
                    target
                        .metadata
                        .semantic
                        .result
                        .as_ref()
                        .ok_or_else(invalid)?,
                ),
            )?
            .pop()
            .ok_or_else(invalid)?;
        let signature = Signature {
            params: params
                .iter()
                .map(|argument| argument.ty().clone())
                .collect(),
            result: result.ty().clone(),
        };
        let application = self.alloc_application(
            execution,
            ApplicationRecord::Semantic(MethodApplication {
                signature: signature.clone(),
                scoped_signature: Some(ScopedSignature { params, result }),
                environment: Some(applied.facts().clone()),
                result_adapter: None,
            }),
            &sources,
        )?;
        sources.push(MetadataSource::Application(application.source()));
        let target = CallableTarget::Script(target.id);
        self.alloc_operation_group(
            execution,
            vec![BoundOperation {
                requirement_key: owner.identity_admission().runtime_in(
                    &requirement,
                    &self.definition_context().snapshot(),
                    &Default::default(),
                )?,
                receiver_identity,
                application: Some(application.id()),
                generic: None,
                associated_interface: requirement.interface.clone(),
                interface_key: owner.identity_admission().runtime_in(
                    &requirement.interface,
                    &self.definition_context().snapshot(),
                    &Default::default(),
                )?,
                requirement,
                slot: 0,
                primitive: None,
                owner: owner.clone(),
                target,
                signature,
            }],
            &sources,
        )
    }
}
