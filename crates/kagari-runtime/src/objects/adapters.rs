//! Bind checked shared protocol calls without resolving trait implementations.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        applications::MethodApplication, groups::OperationId, operation::BoundOperation,
    },
    frame::types::{
        EnvironmentRecord, TypeEnvironment, arguments::ScopedSignature, compatibility::TypeView,
    },
    module::LoadedModule,
    objects::calls::resolve_requirement,
};
use kagari_bytecode::module::CallableTarget;
use kagari_contract::callable::adapter::SharedAdapterWitness;
use kagari_types::callable::Signature;
use std::{slice, sync::Arc};

impl Runtime {
    pub(super) fn bind_shared_adapter(
        &self,
        caller: &LoadedModule,
        environment: Option<TypeEnvironment>,
        adapter: &SharedAdapterWitness,
    ) -> Result<OperationId, RuntimeError> {
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
        let operations =
            self.bind_operations_in(caller, environment.clone(), &adapter.call.operations)?;
        let arguments = self.type_arguments(
            caller,
            environment.as_ref().map(|env| env.types.clone()),
            &adapter.call.arguments,
        )?;
        let mut applied = EnvironmentRecord::new(
            self.definition_context(),
            body.parameters.clone(),
            arguments,
        )?;
        applied.constructions = self.construction_scope(caller, environment.as_ref())?;
        applied.extend_operations(operations);
        let applied = self.alloc_environment(applied)?;
        let parameters = (0..target.metadata.params.len())
            .map(|index| target.metadata.semantic.params.get(&index).cloned())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(invalid)?;
        let params = self.type_arguments(&owner, Some(applied.types.clone()), &parameters)?;
        let result = self
            .type_arguments(
                &owner,
                Some(applied.types.clone()),
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
        let application = self.gc.alloc_method_application(MethodApplication {
            signature: signature.clone(),
            scoped_signature: Some(ScopedSignature { params, result }),
            environment: Some(applied),
            result_adapter: None,
        })?;
        let requirement = resolve_requirement(environment.as_ref(), &adapter.requirement)?;
        let target = CallableTarget::Script(target.id);
        let receiver_identity = Arc::new(
            TypeView::new(
                &adapter.requirement.receiver,
                caller,
                environment.as_ref().map(|env| env.types.as_ref()),
            )
            .identity()
            .ok_or_else(invalid)?,
        );
        self.gc.alloc_bound_operation(BoundOperation {
            receiver_identity,
            application: Some(application),
            generic: None,
            associated_interface: requirement.interface.clone(),
            requirement,
            slot: 0,
            primitive: None,
            owner,
            target,
            signature,
        })
    }
}
