//! Branded protection of arguments and results for synchronous Host execution.
use crate::{
    Runtime,
    host_scope::HostResourceScope,
    native::{binding::NativeResult, conversion::arguments::ArgumentValues},
    value::{
        carriers::RootedValue,
        place::{PublishedResult, ResultPlace},
        view::ValueView,
    },
};
use std::{marker::PhantomData, rc::Rc};

/// An exclusive runtime reservation with a fresh invariant lifetime brand.
/// The runtime cannot be transferred while this scope is active. Dropping it
/// retires protected values and ends its Host reservations.
pub struct HostScope<'scope, 'host: 'scope> {
    resources: HostResourceScope<'host>,
    brand: PhantomData<(&'scope mut &'scope (), &'host mut Runtime, Rc<()>)>,
}

impl Runtime {
    /// Protect values and execute under a fresh Host scope. The closure's output is independent
    /// of that brand. Cleanup also runs when the closure returns an error or unwinds.
    ///
    /// Runtime transfer is also excluded while the scope is active:
    /// ```compile_fail
    /// use kagari_runtime::Runtime;
    /// fn transfer(mut runtime: Runtime) {
    ///     runtime.with_host_scope(|_| { drop(runtime); Ok(()) }).unwrap();
    /// }
    /// ```
    /// A scope cannot be sent to another thread:
    /// ```compile_fail
    /// use kagari_runtime::Runtime;
    /// fn transfer(runtime: &mut Runtime) {
    ///     runtime.with_host_scope(|scope| {
    ///         std::thread::scope(|threads| { threads.spawn(move || drop(scope)); });
    ///         Ok(())
    ///     }).unwrap();
    /// }
    /// ```
    pub fn with_host_scope<'host, R>(
        &'host mut self,
        f: impl for<'scope> FnOnce(&mut HostScope<'scope, 'host>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let resources = self.host_scope(&[])?;
        f(&mut HostScope {
            resources,
            brand: PhantomData,
        })
    }
}

impl<'scope, 'host> HostScope<'scope, 'host> {
    /// Import genuine Host ownership into this scope without cloning its root.
    pub fn import(&mut self, value: &RootedValue) -> NativeResult<ValueView<'scope>> {
        self.resources.import(value).map(ValueView::reborrow)
    }

    /// Copy an existing context's live view into this scope's protected lifetime.
    pub fn protect(&mut self, value: &ValueView<'_>) -> NativeResult<ValueView<'scope>> {
        self.resources.protect(value).map(ValueView::reborrow)
    }

    /// Retain a validated result beyond the scope.
    pub fn retain(&mut self, value: &ValueView<'_>) -> NativeResult<RootedValue> {
        self.resources.retain(value)
    }

    /// Invoke a backend with protected arguments and a linear result destination.
    /// The backend publishes before retiring its own scratch and returns that
    /// proof. The checked result remains local to this scope; no Host root pack
    /// bridges entry/return.
    pub fn execute(
        &mut self,
        arguments: &[ValueView<'_>],
        invoke: impl for<'result> FnOnce(
            &mut Runtime,
            ArgumentValues<'scope>,
            ResultPlace<'result>,
        ) -> NativeResult<PublishedResult<'result>>,
    ) -> NativeResult<ValueView<'scope>> {
        self.resources.execute(arguments, invoke)
    }
}
