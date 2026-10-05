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
use kagari_common::identity::DefinitionPath;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, module::ModuleDecl, requirement::NativeCallableRequirement},
    ty::Ty,
};
use std::collections::BTreeMap;

pub struct InherentMethodsBuilder {
    pub(crate) owner: DefinitionPath,
    pub(crate) receiver: Type,
    pub(crate) receiver_codec: Option<Codec>,
    pub(crate) methods: BTreeMap<DefinitionPath, FnDecl>,
    pub(crate) documentation: BTreeMap<DefinitionPath, String>,
    pub(crate) method_parameters: BTreeMap<DefinitionPath, Vec<String>>,
    pub(crate) requirements: BTreeMap<DefinitionPath, Vec<NativeCallableRequirement>>,
    pub(crate) concrete_results: BTreeMap<DefinitionPath, Ty>,
    pub(crate) bindings: BTreeMap<DefinitionPath, NativeBinding>,
}

impl InherentMethodsBuilder {
    pub fn define_method(&mut self, mut declaration: MethodDecl) -> NativeResult<FunctionRef> {
        let documentation = declaration.signature.documentation.take();
        let mut signature = declaration.lower(self.receiver.clone());
        let id = ModuleDecl::method_id(&self.owner, &signature.name);
        if self.methods.contains_key(&id) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate inherent method declaration",
            ));
        }
        if let Some(text) = documentation {
            self.documentation.insert(id.clone(), text);
        }
        signature.implementation = CallableImplementation::Native(id.clone());
        self.methods.insert(id.clone(), signature);
        Ok(FunctionRef { id })
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
            id: method.id.clone(),
            concrete_results: &mut self.concrete_results,
            signature,
            requirements: self.requirements.entry(method.id.clone()).or_default(),
            names: self.method_parameters.entry(method.id.clone()).or_default(),
        })?;
        normalize_bounds(signature);
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
