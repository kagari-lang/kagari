//! Promote compiler-selected evidence to a retained, checked execution entry.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::managed::{environments::EnvironmentObject, sources::MetadataSource},
    frame::types::arguments::ScopedSignature,
    module::{
        ExecutableModule, ModuleEpochRetention, dispatch::CallRoute,
        managed::source::RevisionSource,
    },
    native::{
        binding::NativeResult,
        context::{CallableOwner, LinkedCallable},
        conversion::{KagariType, arguments::KagariArguments, context::ConversionContext},
        function_handle::{Function, FunctionBinding, PreparedFunction, Target},
    },
};
use kagari_gc::root::Root;
use std::{
    marker::PhantomData,
    slice,
    sync::{Arc, OnceLock},
};

/// The already checked relationship and lexical environment, not a selected body lease.
#[derive(Debug)]
pub(super) struct SelectedBinding {
    pub(super) route: CallRoute,
    owner: ExecutableModule,
    target: LinkedCallable,
    environment: Option<Root<EnvironmentObject>>,
}

impl SelectedBinding {
    pub(super) fn prepare(&self, runtime: &mut Runtime) -> NativeResult<PreparedFunction> {
        let selected = runtime.select_call_route(&self.route)?;
        let mut target = self.target.clone();
        let environment_root = match (&target.environment, &self.environment) {
            (Some(_), Some(root))
                if self.owner.publication_token() == selected.publication_token() =>
            {
                // Immutable facts and their genuine lease already match this
                // publication. The constructor authenticates them before use.
                Some(root.clone())
            }
            (Some(facts), Some(root)) => {
                let mut conversion = ConversionContext::new(runtime, selected.owner())?;
                let execution = conversion.execution_id()?;
                let environment = conversion
                    .runtime_mut()
                    .copy_environment_in(
                        execution,
                        facts,
                        &[MetadataSource::HostEnvironment(root)],
                    )?
                    .ok_or_else(|| {
                        RuntimeError::module_validation("selected relationship environment")
                    })?;
                let environment = conversion.runtime_mut().transport_environment(
                    execution,
                    &self.owner,
                    &selected,
                    &environment,
                )?;
                let root = conversion.runtime_mut().retain_environment(&environment)?;
                target.environment = Some(environment.facts().clone());
                Some(root)
            }
            (None, None) => None,
            _ => {
                return Err(RuntimeError::module_validation(
                    "selected environment ownership",
                ));
            }
        };
        target.target = selected.target();
        target.owner = CallableOwner::Resolved(selected.owner().clone());
        let origin = runtime.selection_origin(selected.owner())?;
        PreparedFunction::selected(runtime, selected.owner(), target, origin, environment_root)
    }
}

impl<A: KagariArguments, R: KagariType> Function<A, R> {
    pub(crate) fn from_selected(
        runtime: &mut Runtime,
        caller: &ExecutableModule,
        target: &LinkedCallable,
        environment: Option<Root<EnvironmentObject>>,
    ) -> NativeResult<Self> {
        let owner = target.owner(caller)?;
        runtime.validate_executable_module(&owner)?;
        match (&target.environment, &environment) {
            (Some(facts), Some(root)) => {
                runtime.validate_environment_in(&[MetadataSource::HostEnvironment(root)], facts)?
            }
            (None, None) => {}
            _ => {
                return Err(RuntimeError::module_validation(
                    "selected environment ownership",
                ));
            }
        }
        let signature = match target.scoped_signature.get() {
            Some(signature) => signature.clone(),
            None => Arc::new(ScopedSignature {
                params: runtime.resolve_type_arguments(&owner, &target.params)?,
                result: runtime
                    .resolve_type_arguments(&owner, slice::from_ref(&target.result))?
                    .pop()
                    .ok_or_else(|| RuntimeError::module_validation("selected result scope"))?,
            }),
        };
        let cx = ConversionContext::for_mapping(runtime, &owner)?;
        A::check_types(&cx, &signature.params)?;
        cx.check_type::<R>(&signature.result)?;
        drop(cx);
        let route = owner.bind_call_route(target.target)?;
        let mut target = target.clone();
        target.owner = CallableOwner::Program(owner.slot());
        target.scoped_signature = OnceLock::from(signature);
        // This is an ordinary relationship. Its future calls select a body;
        // checking the Rust mapping creates no temporary admitted computation.
        Ok(Self {
            binding: Arc::new(FunctionBinding::Selected(SelectedBinding {
                route,
                owner,
                target,
                environment,
            })),
            mapping: PhantomData,
        })
    }
}

impl PreparedFunction {
    pub(crate) fn selected(
        runtime: &mut Runtime,
        caller: &ExecutableModule,
        mut target: LinkedCallable,
        origin: RevisionSource<'_>,
        environment: Option<Root<EnvironmentObject>>,
    ) -> NativeResult<Self> {
        let owner = target.owner(caller)?;
        runtime.validate_executable_module(&owner)?;
        match (&target.environment, &environment) {
            (Some(facts), Some(root)) => {
                runtime.validate_environment_in(&[MetadataSource::HostEnvironment(root)], facts)?;
            }
            (None, None) => {}
            _ => {
                return Err(RuntimeError::module_validation(
                    "prepared environment ownership",
                ));
            }
        }
        let signature = match target.scoped_signature.get() {
            Some(signature) => signature.clone(),
            None => Arc::new(ScopedSignature {
                params: runtime.resolve_type_arguments(&owner, &target.params)?,
                result: runtime
                    .resolve_type_arguments(&owner, slice::from_ref(&target.result))?
                    .pop()
                    .ok_or_else(|| RuntimeError::module_validation("selected result scope"))?,
            }),
        };
        let program = runtime
            .retain_program(&owner, ModuleEpochRetention::RuntimeValue, origin)
            .ok_or_else(|| RuntimeError::module_validation("selected program retention"))?;
        // The actual body/environment owners above replace any input transport's
        // legacy retention pack. The retained callable keeps pure facts only.
        target.owner = CallableOwner::Resolved(owner.clone());
        Ok(Self {
            source_entry: target.target,
            owner,
            signature,
            target: Target::Entry(target),
            _program: program,
            environment,
        })
    }
}
