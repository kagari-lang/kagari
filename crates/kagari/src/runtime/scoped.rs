//! Scoped external lending uses the common runtime owner and explicit SDK policy.
use crate::{RunResult, context::ExecutionContext, error::EmbeddingError, runtime::Runtime};
use kagari_runtime::{
    host_scope::{scoped::HostScope, value::ScopedValue},
    module::{Program, entry::ProgramEntry},
};
use kagari_vm::vm::Vm;

impl Runtime {
    /// Reserve host objects under an invariant lifetime brand. Native errors use
    /// the ordinary SDK execution mapping; errors/unwinds revoke all scoped views.
    ///
    /// Scoped views cannot escape through the SDK facade:
    /// ```compile_fail
    /// use kagari::runtime::Runtime;
    /// use kagari_runtime::native::rust_type::RustType;
    /// fn escape(runtime: &mut Runtime, ty: &RustType<i32>, value: &mut i32) {
    ///     let view = runtime.with_host_scope(|scope| {
    ///         Ok(scope.borrow_mut(ty, value)?)
    ///     }).unwrap();
    ///     drop(view);
    /// }
    /// ```
    pub fn with_host_scope<'host, R>(
        &'host mut self,
        access: impl for<'scope> FnOnce(&mut HostScope<'scope, 'host>) -> RunResult<R>,
    ) -> RunResult<R> {
        self.vm
            .runtime_mut()
            .with_host_scope(|scope| Ok(access(scope)))
            .map_err(EmbeddingError::from)?
    }

    /// Execute a closed scoped entry on the interpreter using explicit cancellation,
    /// deterministic inputs and observation policy. JIT policies reject before entry.
    /// The result stays protected by the scope. Use retain() on an ordinary
    /// script result to keep it after scope exit; external capabilities remain scoped.
    pub fn execute_scoped<'entry, 'scope>(
        scope: &HostScope<'scope, '_>,
        program: &Program,
        entry: impl Into<ProgramEntry<'entry>>,
        arguments: &[ScopedValue<'scope>],
        context: &ExecutionContext,
    ) -> RunResult<ScopedValue<'scope>> {
        let entry = entry.into();
        context.validate_for_execute(entry.label())?;
        Vm::execute_scoped(scope, program, entry, arguments, context.runtime_options())
            .map_err(EmbeddingError::from)
    }
}
