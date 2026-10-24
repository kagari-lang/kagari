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
    /// Bind a public callable and validate its argument/result types once.
    /// Calls select the current implementation of the originally bound declaration.
    pub fn function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        module: &Module,
        name: &str,
    ) -> RunResult<Function<A, R>> {
        self.runtime()
            .bind_function(module, name)
            .map_err(Into::into)
    }

    /// Bind a callable returning exactly one installed `Future<R>` layer.
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

    /// Resolve a public, nongeneric object type in this live module.
    /// Applied generic types remain available through retained Object descriptors.
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
