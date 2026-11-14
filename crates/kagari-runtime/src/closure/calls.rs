//! One selected closure call carries its actual capture/body/environment sources.
use crate::{
    Runtime,
    closure::{ClosureRecord, ClosureTarget, NativeClosure},
    error::{RuntimeError, trap::TrapReason},
    execution::roots::SessionId,
    execution_metadata::managed::sources::MetadataSource,
    frame::types::TypeEnvironment,
    module::{ExecutableModule, managed::source::RevisionSource},
    resource::ResourceState,
    value::view::ValueView,
};
use kagari_bytecode::module::CallableTarget;
use std::sync::Arc;

/// A synchronous admission borrows its source rather than retaining a Host root.
/// Backends enter this selection once; queuing requires genuine retained storage.
/// Keeping this record or its facts does not keep the supplying execution alive.
pub struct SelectedClosure<'call> {
    value: ValueView<'call>,
    record: Arc<ClosureRecord>,
    owner: ExecutableModule,
    target: CallableTarget,
    origin: RevisionSource<'static>,
    environment: Option<(TypeEnvironment, MetadataSource<'static>)>,
}

impl<'call> SelectedClosure<'call> {
    pub fn owner(&self) -> &ExecutableModule {
        &self.owner
    }

    pub(crate) fn target(&self) -> CallableTarget {
        self.target
    }

    pub(crate) fn origin(&self) -> RevisionSource<'static> {
        self.origin
    }

    pub(crate) fn value(&self) -> ValueView<'call> {
        self.value
    }

    pub(crate) fn record(&self) -> &ClosureRecord {
        &self.record
    }

    pub(crate) fn environment(&self) -> Option<(TypeEnvironment, MetadataSource<'static>)> {
        self.environment.clone()
    }

    pub(crate) fn retained_record(
        &self,
        resources: &ResourceState,
    ) -> Result<Arc<ClosureRecord>, RuntimeError> {
        let mut captures = Vec::new();
        captures
            .try_reserve_exact(self.record.capture_types.len())
            .map_err(|_| RuntimeError::resource_limit("retained closure capture types"))?;
        for (index, argument) in self.record.capture_types.iter().enumerate() {
            if index.is_multiple_of(1024) {
                resources.poll_execution()?;
            }
            captures.push(argument.clone());
        }
        Ok(Arc::new(ClosureRecord {
            implementation: self.owner.clone(),
            target: match self.target {
                CallableTarget::Script(function) => ClosureTarget::Script(function),
                CallableTarget::Native(import) => ClosureTarget::Native(NativeClosure { import }),
            },
            environment: self.environment.as_ref().map(|(facts, _)| facts.clone()),
            capture_types: captures.into_boxed_slice(),
            signature: self.record.signature.clone(),
        }))
    }
}

impl Runtime {
    /// The frame/Native/activation owns the supplied execution suffix. Cross-epoch
    /// environment preparation publishes into that suffix before the callee copies
    /// its own dependencies. Ordinary inner calls still select current bodies.
    pub(crate) fn select_closure_call<'call>(
        &mut self,
        execution: SessionId,
        value: &ValueView<'call>,
    ) -> Result<SelectedClosure<'call>, RuntimeError> {
        let record = self.resolve_closure(value)?;
        let location = value
            .location()
            .ok_or_else(|| RuntimeError::from(TrapReason::InvalidClosureHandle))?;
        let selected = record
            .call_route()
            .map(|route| self.select_call_route(route))
            .transpose()?;
        let (owner, target, origin) = match &selected {
            Some(selected) => (
                selected.owner().clone(),
                selected.target(),
                self.selection_origin(selected.owner())?,
            ),
            None => {
                self.validate_executable_module(&record.implementation)?;
                (
                    record.implementation.clone(),
                    record.callable_target(),
                    RevisionSource::Closure(location),
                )
            }
        };
        let environment = match &record.environment {
            Some(facts) => {
                let source = MetadataSource::ClosureEnvironment(location);
                if let Some(selected) = selected.as_ref().filter(|selected| {
                    selected.publication_token() != record.implementation.publication_token()
                }) {
                    let environment = self
                        .copy_environment_in(execution, facts, &[source])?
                        .ok_or_else(|| {
                            RuntimeError::module_validation("closure environment source")
                        })?;
                    let environment = self.transport_environment(
                        execution,
                        &record.implementation,
                        selected,
                        &environment,
                    )?;
                    Some((
                        environment.facts().clone(),
                        MetadataSource::Environment(environment.source()),
                    ))
                } else {
                    Some((facts.clone(), source))
                }
            }
            None => None,
        };
        Ok(SelectedClosure {
            value: *value,
            record,
            owner,
            target,
            origin,
            environment,
        })
    }
}
