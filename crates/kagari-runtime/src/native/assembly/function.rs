//! Complete free-function authoring uses the same scoped FunctionBuilder as methods.
use crate::native::{
    adapter::NativeAdapter,
    assembly::authoring::{PendingBinding, SealedPreparation},
    binding::NativeResult,
    declarations::{FunctionBuilder, FunctionDecl},
    types::Type,
};
use kagari_types::declaration::host::HostCallPolicy;

#[cfg(test)]
mod tests;

type Configure = Box<dyn FnOnce(&mut FunctionBuilder<'_>) -> NativeResult<()>>;

/// One free-function signature, configuration recipe and deferred implementation.
/// A `(FunctionDecl, PendingBinding)` pair is the explicit raw submission form.
pub struct FunctionDefinition {
    pub(crate) declaration: FunctionDecl,
    pub(crate) concrete_result: Option<Type>,
    pub(crate) configure: Option<Configure>,
    pub(crate) binding: PendingBinding,
}

impl From<(FunctionDecl, PendingBinding)> for FunctionDefinition {
    fn from((declaration, binding): (FunctionDecl, PendingBinding)) -> Self {
        FunctionAuthoring::new(declaration).complete(binding)
    }
}

/// Configures generics and ordered selected-call requirements before sealing.
/// Configuration is local, once-only and need not be Send. Only a retained native
/// adapter or factory needs Send when transferring the sealed assembly.
pub struct FunctionAuthoring {
    declaration: FunctionDecl,
    concrete_result: Option<Type>,
    configure: Option<Configure>,
}

impl FunctionAuthoring {
    fn new(declaration: FunctionDecl) -> Self {
        Self {
            declaration,
            concrete_result: None,
            configure: None,
        }
    }

    pub fn documentation(mut self, text: impl Into<String>) -> Self {
        self.declaration = self.declaration.documentation(text);
        self
    }

    pub fn host_policy(mut self, policy: HostCallPolicy) -> Self {
        self.declaration = self.declaration.host_policy(policy);
        self
    }

    pub fn parameter(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.declaration = self.declaration.parameter(name, ty);
        self
    }

    pub fn returns(mut self, ty: Type) -> Self {
        self.declaration = self.declaration.returns(ty);
        self
    }

    pub fn produces(mut self, ty: Type) -> Self {
        self.concrete_result = Some(ty);
        self
    }

    /// Run configurations in insertion order after all fluent signature setters.
    /// Failure aborts this submission without publishing any function or binding.
    pub fn configure(
        mut self,
        configure: impl FnOnce(&mut FunctionBuilder<'_>) -> NativeResult<()> + 'static,
    ) -> Self {
        let previous = self.configure.take();
        self.configure = Some(Box::new(move |function| {
            if let Some(previous) = previous {
                previous(function)?;
            }
            configure(function)
        }));
        self
    }

    pub fn implementation<M, R>(
        self,
        adapter: impl NativeAdapter<M, R> + 'static,
    ) -> FunctionDefinition {
        self.complete(PendingBinding::implementation(adapter))
    }

    pub fn implementation_with<M, R, A>(
        self,
        factory: impl FnOnce(&SealedPreparation<'_>) -> NativeResult<A> + 'static,
    ) -> FunctionDefinition
    where
        A: NativeAdapter<M, R>,
    {
        self.complete(PendingBinding::implementation_with(factory))
    }

    pub fn implementation_send<M, R>(
        self,
        adapter: impl NativeAdapter<M, R> + Send + 'static,
    ) -> FunctionDefinition {
        self.complete(PendingBinding::implementation_send(adapter))
    }

    pub fn implementation_with_send<M, R, A>(
        self,
        factory: impl FnOnce(&SealedPreparation<'_>) -> NativeResult<A> + Send + 'static,
    ) -> FunctionDefinition
    where
        A: NativeAdapter<M, R>,
    {
        self.complete(PendingBinding::implementation_with_send(factory))
    }

    fn complete(self, binding: PendingBinding) -> FunctionDefinition {
        FunctionDefinition {
            declaration: self.declaration,
            concrete_result: self.concrete_result,
            configure: self.configure,
            binding,
        }
    }
}

impl FunctionDecl {
    /// Configure a free function under its actual binder before native preparation.
    pub fn configure(
        self,
        configure: impl FnOnce(&mut FunctionBuilder<'_>) -> NativeResult<()> + 'static,
    ) -> FunctionAuthoring {
        FunctionAuthoring::new(self).configure(configure)
    }

    /// Declare the concrete value returned behind an interface result.
    pub fn produces(self, ty: Type) -> FunctionAuthoring {
        FunctionAuthoring::new(self).produces(ty)
    }

    pub fn implementation<M, R>(
        self,
        adapter: impl NativeAdapter<M, R> + 'static,
    ) -> FunctionDefinition {
        FunctionAuthoring::new(self).implementation(adapter)
    }

    pub fn implementation_with<M, R, A>(
        self,
        factory: impl FnOnce(&SealedPreparation<'_>) -> NativeResult<A> + 'static,
    ) -> FunctionDefinition
    where
        A: NativeAdapter<M, R>,
    {
        FunctionAuthoring::new(self).implementation_with(factory)
    }

    pub fn implementation_send<M, R>(
        self,
        adapter: impl NativeAdapter<M, R> + Send + 'static,
    ) -> FunctionDefinition {
        FunctionAuthoring::new(self).implementation_send(adapter)
    }

    pub fn implementation_with_send<M, R, A>(
        self,
        factory: impl FnOnce(&SealedPreparation<'_>) -> NativeResult<A> + Send + 'static,
    ) -> FunctionDefinition
    where
        A: NativeAdapter<M, R>,
    {
        FunctionAuthoring::new(self).implementation_with_send(factory)
    }
}
