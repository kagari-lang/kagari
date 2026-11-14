//! Nested Native work borrows one invocation and retires its temporary suffix.
use crate::{
    execution::roots::ExecutionRoots,
    native::{
        binding::NativeResult,
        context::{NativeCall, result::ReturnSource},
        conversion::context::preserve_value,
    },
    value::view::ValueView,
};
use std::ops::ControlFlow;

impl<'call> NativeCall<'call> {
    /// Run nested work without allowing temporary views to escape. All exits,
    /// including unwind, retire its local, return-token and metadata suffix.
    /// Arguments, binding preparation and the pending result remain borrowed
    /// from this invocation; this registers no Host root or new Native entry.
    ///
    /// ```compile_fail,E0521
    /// use kagari_runtime::{native::{binding::NativeResult, context::NativeCall},
    ///     value::view::ValueView};
    /// fn escape<'call>(call: &mut NativeCall<'call>) -> NativeResult<ValueView<'call>> {
    ///     call.with_temporaries(|temporary| temporary.argument(0))
    /// }
    /// ```
    pub fn with_temporaries<R>(
        &mut self,
        run: impl for<'scope> FnOnce(&mut NativeCall<'scope>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let mut temporary = self.scratch()?;
        run(&mut temporary)
    }

    /// Keep at most one completed view in this invocation's source. Everything
    /// else created by the nested work retires before this method returns.
    pub fn with_optional_value_temporaries(
        &mut self,
        run: impl for<'scope> FnOnce(&mut NativeCall<'scope>) -> NativeResult<Option<ValueView<'scope>>>,
    ) -> NativeResult<Option<ValueView<'call>>> {
        let mut temporary = self.scratch()?;
        let Some(value) = run(&mut temporary)? else {
            return Ok(None);
        };
        let mark = temporary.locals.as_mut().expect("Native scratch suffix");
        let value = preserve_value(temporary.runtime, mark, value)
            .map_err(|error| temporary.gc_error(error))?;
        Ok(Some(value.reborrow()))
    }

    /// Repeated work replaces one protected state cell instead of accumulating
    /// completed intermediate results. Each step receives a fresh short lend;
    /// its views cannot escape except as the next state or completed result.
    /// Cleanup and cancellation use the enclosing invocation's existing owners.
    pub fn with_value_loop(
        &mut self,
        initial: &ValueView<'_>,
        mut step: impl for<'scope> FnMut(
            &mut NativeCall<'scope>,
            ValueView<'scope>,
        ) -> NativeResult<
            ControlFlow<ValueView<'scope>, ValueView<'scope>>,
        >,
    ) -> NativeResult<ValueView<'call>> {
        let mut region = self.scratch()?;
        let execution = region.pending.execution();
        let (location, mut current) = region
            .runtime
            .memory
            .scope(&region.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = initial.get(slots)?;
                    let location = slots.push_local(execution, value)?;
                    ValueView::protected(value, Some(location)).map(|view| (location, view))
                })
            })
            .map_err(|error| region.gc_error(error))?;
        loop {
            let completed = {
                let mut temporary = region.scratch()?;
                let (completed, value) = match step(&mut temporary, current.reborrow())? {
                    ControlFlow::Break(value) => (true, value),
                    ControlFlow::Continue(value) => (false, value),
                };
                let next = temporary
                    .runtime
                    .memory
                    .scope(&temporary.runtime.resources, |cx, roots| {
                        ExecutionRoots::with(roots, cx, |slots| {
                            let value = value.get(slots)?;
                            slots.set(location, value)?;
                            ValueView::protected(value, Some(location))
                        })
                    })
                    .map_err(|error| temporary.gc_error(error))?;
                // The state cell precedes this step's scratch and belongs to
                // region. Publish before the temporary source can retire.
                current = next.reborrow();
                completed
            };
            if completed {
                let mark = region.locals.as_mut().expect("Native state suffix");
                let value = preserve_value(region.runtime, mark, current)
                    .map_err(|error| region.gc_error(error))?;
                return Ok(value.reborrow());
            }
        }
    }

    fn scratch(&mut self) -> NativeResult<NativeCall<'_>> {
        self.poll()?;
        let execution = self.pending.execution();
        let mark = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| slots.mark_locals(execution))
            })
            .map_err(|error| self.gc_error(error))?;
        Ok(NativeCall {
            _thread: Default::default(),
            pending: ReturnSource::Borrowed(&mut self.pending),
            locals: Some(mark),
            native_stack: None,
            runtime: self.runtime,
            owner: self.owner,
            function: self.function,
            arguments: self.arguments.reborrow(),
            invoke_script: self.invoke_script,
        })
    }
}
