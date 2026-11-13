//! Applied call facts belong to the linked program, independently of receiver values.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::SessionId,
    execution_metadata::{
        application_key::{ApplicationArguments, ApplicationKey},
        applications::{ApplicationRecord, MethodApplication},
        groups::AdmittedOperationGroup,
        managed::sources::MetadataSource,
    },
    frame::types::{
        EnvironmentRecord,
        arguments::{ScopedSignature, TypeArgument},
        construction::ConstructionScope,
        operations::OperationBindings,
    },
    objects::invocation::MethodInvocation,
};
use kagari_types::callable::Signature;
use std::slice;

/// Cohesive checked application input. Witnesses keep their own actual Host or
/// execution sources through semantic application publication.
pub(crate) struct MethodApplicationInput<'input> {
    pub(crate) arguments: &'input [TypeArgument],
    pub(crate) operations: OperationBindings,
    pub(crate) constructions: ConstructionScope,
    pub(crate) sources: &'input [MetadataSource<'input>],
}

impl<'input> MethodApplicationInput<'input> {
    pub(crate) fn plain(arguments: &'input [TypeArgument]) -> Self {
        Self {
            arguments,
            operations: OperationBindings::default(),
            constructions: ConstructionScope::default(),
            sources: &[],
        }
    }
}

impl Runtime {
    pub(crate) fn apply_method_invocation(
        &mut self,
        execution: SessionId,
        mut method: MethodInvocation,
        arguments: &ApplicationArguments,
        operations: OperationBindings,
        protected: &mut Vec<MetadataSource<'_>>,
    ) -> Result<MethodInvocation, RuntimeError> {
        let view = method.facts(self, protected)?;
        if arguments.values().len() != view.type_parameters().len() {
            return Err(RuntimeError::module_validation(
                "interface method type arguments",
            ));
        }
        for argument in arguments.values() {
            argument.validate(self)?;
        }
        if view.type_parameters().is_empty()
            && view.entry_parameters().is_empty()
            && view.receiver_environment().is_none()
            && operations.is_empty()
        {
            drop(view);
            return Ok(method);
        }
        let owner = view.proof_implementation().clone();
        let identity = view.identity();
        drop(view);
        let receiver_operations = method.receiver_operations(self, execution, protected)?;
        if let Some(group) = &receiver_operations {
            protected
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("method dictionary sources"))?;
            protected.push(MetadataSource::Operations(group.source()));
        }
        let key = ApplicationKey::new(
            identity,
            arguments,
            receiver_operations
                .as_ref()
                .map(|group| group.record().id()),
            &operations,
        );
        let cached = self
            .modules
            .method_application(&owner, &key)
            .map(|id| self.copy_application_in(execution, id, protected))
            .transpose()?
            .flatten();
        let application = if let Some(prepared) = cached {
            if !matches!(prepared.record().as_ref(), ApplicationRecord::Semantic(_)) {
                return Err(RuntimeError::module_validation(
                    "semantic method application cache",
                ));
            }
            prepared
        } else {
            let prepared = self.prepare_method_application(
                execution,
                &method,
                arguments,
                operations,
                receiver_operations.as_ref(),
                protected,
            )?;
            let prepared = self.alloc_application(
                execution,
                ApplicationRecord::Semantic(prepared),
                protected,
            )?;
            protected
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("method application sources"))?;
            protected.push(MetadataSource::Application(prepared.source()));
            self.publish_method_application(&owner, key, &prepared, protected)?;
            prepared
        };
        method.application = Some(application);
        Ok(method)
    }

    fn prepare_method_application(
        &mut self,
        execution: SessionId,
        method: &MethodInvocation,
        arguments: &ApplicationArguments,
        operations: OperationBindings,
        receiver_operations: Option<&AdmittedOperationGroup>,
        protected: &mut Vec<MetadataSource<'_>>,
    ) -> Result<MethodApplication, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::MethodPreparation);
        let view = method.facts(self, protected)?;
        let receiver = view
            .receiver_environment()
            .map(|environment| self.environment_snapshot_in(protected, environment))
            .transpose()?;
        let mut supplied = receiver
            .as_ref()
            .map(|snapshot| snapshot.record.operations().clone())
            .unwrap_or_default();
        supplied.extend(operations);
        let operations = supplied;
        let mut binders = EnvironmentRecord::new(
            view.proof_implementation().definitions(),
            view.type_parameters().to_vec(),
            arguments.values().to_vec(),
        )?;
        let mut constructions = ConstructionScope::new(view.proof_implementation());
        if let Some(receiver) = &receiver {
            constructions.extend(&receiver.record.constructions);
        }
        constructions.extend(arguments.constructions());
        binders.set_constructions(constructions.clone());
        binders.include(view.receiver_environment().cloned())?;
        if let Some(group) = receiver_operations {
            binders.add_receiver(group.record())?;
        }
        // Associated results need selected output facts while the signature is
        // resolved, before publishing the executable application.
        binders.extend_operations(operations.clone());
        protected
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("method binder sources"))?;
        let binders = self.alloc_environment(execution, binders, protected)?;
        protected.push(MetadataSource::Environment(binders.source()));
        let result_adapter = view.result_adapter().map(|adapter| {
            let mut adapter = adapter.clone();
            adapter.environment = Some(binders.facts().clone());
            adapter
        });
        let params = self.type_arguments(
            view.proof_implementation(),
            Some(binders.facts().types.clone()),
            view.parameter_types(),
        )?;
        let result = self
            .type_arguments(
                view.proof_implementation(),
                Some(binders.facts().types.clone()),
                slice::from_ref(view.return_type()),
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("interface return type"))?;
        let signature = Signature {
            params: params
                .iter()
                .map(|argument| argument.ty().clone())
                .collect(),
            result: result.ty().clone(),
        };
        let scoped_signature = (result.has_origin() || params.iter().any(TypeArgument::has_origin))
            .then_some(ScopedSignature { params, result });
        let environment = if view.entry_parameters().is_empty() {
            None
        } else {
            let mut environment = EnvironmentRecord::new(
                view.proof_implementation().definitions(),
                view.entry_parameters().to_vec(),
                self.type_arguments(
                    view.proof_implementation(),
                    Some(binders.facts().types.clone()),
                    view.entry_arguments(),
                )?,
            )?;
            environment.set_constructions(constructions);
            if let Some(group) = receiver_operations {
                environment.add_receiver(group.record())?;
            }
            environment.extend_operations(operations);
            {
                protected
                    .try_reserve(1)
                    .map_err(|_| RuntimeError::resource_limit("method entry sources"))?;
                let environment = self.alloc_environment(execution, environment, protected)?;
                protected.push(MetadataSource::Environment(environment.source()));
                Some(environment.facts().clone())
            }
        };
        Ok(MethodApplication {
            signature,
            scoped_signature,
            environment,
            result_adapter,
        })
    }
}

#[cfg(test)]
mod tests;
