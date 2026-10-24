//! Bind installed public callable contracts and checked member evidence.
use crate::{RunResult, runtime::Runtime};
use kagari_runtime::{
    module::Module,
    native::{
        conversion::{KagariType, arguments::KagariArguments},
        function_handle::{Function, asynchronous::AsyncFunction},
        interfaces::{Interface, asynchronous::AsyncInterfaceMethod, binding::InterfaceMethod},
        methods::{InherentType, Method, asynchronous::AsyncMethod},
        objects::{ObjectType, fields::Field},
    },
};

impl Runtime {
    /// Bind a public callable relative to `module` in its latest publication.
    /// `name` is a public `::`-separated path, including module and item aliases.
    /// Argument/result types are checked without executing script. Calls select the
    /// current implementation of the originally bound declaration; retargeting an
    /// alias affects fresh lookup, not a retained `Function`.
    pub fn function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        module: &Module,
        name: &str,
    ) -> RunResult<Function<A, R>> {
        self.runtime()
            .bind_function(module, name)
            .map_err(Into::into)
    }

    /// Bind a public callable relative to `module`, returning one `Future<R>` layer.
    /// Public paths and retained declaration identity follow [`Self::function`].
    /// Rejects a mismatched signature or missing executable evidence without running
    /// script. `R` names the awaited output, not a Rust or script Future handle.
    pub fn async_function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        module: &Module,
        name: &str,
    ) -> RunResult<AsyncFunction<A, R>> {
        self.runtime()
            .bind_async_function(module, name)
            .map_err(Into::into)
    }

    /// Resolve a public script object path relative to this live module.
    /// Supports nongeneric structs, re-exports and closed aliases (including aliases
    /// to concrete generic applications). The returned descriptor retains the exact
    /// layout selected from the latest publication; it does not run initialization.
    /// Open aliases and types requiring arguments are rejected. Explicit generic
    /// applications remain available through core `Runtime::bind_type` and
    /// `Runtime::bind_type_declaration`; non-object type leaves are not object types.
    pub fn object_type(&self, module: &Module, name: &str) -> RunResult<ObjectType> {
        self.runtime()
            .bind_type(module, name, &[])
            .map_err(Into::into)
    }

    pub fn field<T: KagariType + 'static>(
        &self,
        ty: &ObjectType,
        name: &str,
    ) -> RunResult<Field<T>> {
        self.runtime().bind_field(ty, name).map_err(Into::into)
    }

    pub fn method<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        ty: &impl InherentType,
        name: &str,
    ) -> RunResult<Method<A, R>> {
        self.runtime().bind_method(ty, name).map_err(Into::into)
    }

    pub fn associated_function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        ty: &impl InherentType,
        name: &str,
    ) -> RunResult<Function<A, R>> {
        self.runtime()
            .bind_associated_function(ty, name)
            .map_err(Into::into)
    }

    pub fn async_method<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        ty: &impl InherentType,
        name: &str,
    ) -> RunResult<AsyncMethod<A, R>> {
        self.runtime()
            .bind_async_method(ty, name)
            .map_err(Into::into)
    }

    pub fn async_associated_function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        ty: &impl InherentType,
        name: &str,
    ) -> RunResult<AsyncFunction<A, R>> {
        self.runtime()
            .bind_async_associated_function(ty, name)
            .map_err(Into::into)
    }

    pub fn interface_method<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        interface: &Interface<impl Sized>,
        name: &str,
    ) -> RunResult<InterfaceMethod<A, R>> {
        let member = interface.member(self.runtime(), name)?;
        self.runtime()
            .bind_interface_method(&member)
            .map_err(Into::into)
    }

    pub fn async_interface_method<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        interface: &Interface<impl Sized>,
        name: &str,
    ) -> RunResult<AsyncInterfaceMethod<A, R>> {
        let member = interface.member(self.runtime(), name)?;
        self.runtime()
            .bind_async_interface_method(&member)
            .map_err(Into::into)
    }
}
