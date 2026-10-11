//! Inherent methods belong to the module that owns the nominal type.
use crate::{
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        conversion::{FromKagari, IntoKagari, arguments::FromKagariArguments},
        declarations::{FunctionBuilder, MethodDecl, normalize_bounds},
        functions::NativeFunction,
        typed::NativeContext,
        types::{FunctionRef, Type},
    },
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext, table::DefinitionId};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, requirement::NativeCallableRequirement},
    ty::Ty,
};
use std::collections::BTreeMap;

pub struct InherentMethodsBuilder {
    pub(crate) context: DefinitionContext,
    pub(crate) owner: DefinitionId,
    pub(crate) receiver: Type,
    pub(crate) receiver_codec: Option<Codec>,
    pub(crate) methods: BTreeMap<DefinitionId, FnDecl<DefinitionId>>,
    pub(crate) documentation: BTreeMap<DefinitionId, String>,
    pub(crate) method_parameters: BTreeMap<DefinitionId, Vec<String>>,
    pub(crate) requirements: BTreeMap<DefinitionId, Vec<NativeCallableRequirement<DefinitionId>>>,
    pub(crate) concrete_results: BTreeMap<DefinitionId, Ty<DefinitionId>>,
    pub(crate) bindings: BTreeMap<DefinitionId, NativeBinding>,
}

impl InherentMethodsBuilder {
    pub fn define_method(&mut self, mut declaration: MethodDecl) -> NativeResult<FunctionRef> {
        let documentation = declaration.signature.documentation.take();
        let mut signature = declaration.lower(self.receiver.clone(), &self.context)?;
        let id = self
            .context
            .intern_child(self.owner, DefinitionKind::Method, &signature.name, 0)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        if self.methods.contains_key(&id) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate inherent method declaration",
            ));
        }
        if let Some(text) = documentation {
            self.documentation.insert(id, text);
        }
        signature.implementation = CallableImplementation::Native(id);
        self.methods.insert(id, signature);
        Ok(FunctionRef {
            id,
            definitions: self.context.snapshot(),
        })
    }

    /// Configure method-local parameters, bounds and selected callable requirements.
    pub fn method<T>(
        &mut self,
        method: &FunctionRef,
        configure: impl FnOnce(&mut FunctionBuilder<'_>) -> NativeResult<T>,
    ) -> NativeResult<T> {
        let signature = self.methods.get_mut(&method.id).ok_or_else(|| {
            RuntimeError::metadata_conflict("unknown inherent method declaration")
        })?;
        let result = configure(&mut FunctionBuilder {
            id: method.id,
            context: &self.context,
            concrete_results: &mut self.concrete_results,
            signature,
            requirements: self.requirements.entry(method.id).or_default(),
            names: self.method_parameters.entry(method.id).or_default(),
        })?;
        normalize_bounds(signature, &self.context.snapshot())?;
        Ok(result)
    }

    /// Bind a typed entry against the declared signature, including generic types.
    pub fn bind_typed<A: FromKagariArguments, R: IntoKagari>(
        &mut self,
        method: FunctionRef,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> NativeResult<()> {
        let signature = self.methods.get(&method.id).ok_or_else(|| {
            RuntimeError::metadata_conflict("unknown inherent method declaration")
        })?;
        let arity = signature.params.len();
        self.bind_with(method, NativeBinding::contextual(arity, entry))
    }

    /// Bind an instance entry with its receiver separate from the argument tuple.
    pub fn bind_typed_method<S: FromKagari, A: FromKagariArguments, R: IntoKagari>(
        &mut self,
        method: FunctionRef,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, S, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> NativeResult<()> {
        let signature = self.methods.get(&method.id).ok_or_else(|| {
            RuntimeError::metadata_conflict("unknown inherent method declaration")
        })?;
        if !signature
            .params
            .first()
            .is_some_and(|param| param.name == "self")
        {
            return Err(RuntimeError::metadata_conflict(
                "typed method requires a receiver",
            ));
        }
        let arity = signature.params.len();
        self.bind_with(method, NativeBinding::contextual_method(arity, entry))
    }

    pub fn bind<A, R>(
        &mut self,
        method: FunctionRef,
        entry: impl NativeFunction<A, R>,
    ) -> NativeResult<()> {
        self.bind_with(method, entry.binding())
    }

    pub fn bind_with(&mut self, method: FunctionRef, binding: NativeBinding) -> NativeResult<()> {
        let signature = self.methods.get(&method.id).ok_or_else(|| {
            RuntimeError::metadata_conflict("unknown inherent method declaration")
        })?;
        if signature
            .params
            .first()
            .is_some_and(|parameter| parameter.name == "self")
            && self.receiver_codec.as_ref().is_some_and(|codec| {
                binding
                    .arguments
                    .first()
                    .is_none_or(|method| !codec.receiver_shape_matches(method))
            })
        {
            return Err(RuntimeError::metadata_conflict(
                "binding receiver differs from the configured receiver codec",
            ));
        }
        if self.bindings.contains_key(&method.id) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate inherent method binding",
            ));
        }
        self.bindings.insert(method.id, binding);
        Ok(())
    }
}
