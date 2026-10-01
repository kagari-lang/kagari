//! Actual application registrations return traced cursors and checked map state.
use self::inputs::Inputs;
use kagari_native_macros::native_module;
use kagari_runtime::{
    error::RuntimeError,
    native::{api::NativeApi, packages::standard_library},
};

pub fn api() -> Result<NativeApi, RuntimeError> {
    api_with_inputs(&Inputs::default())
}
pub fn api_with_inputs(inputs: &Inputs) -> Result<NativeApi, RuntimeError> {
    NativeApi::combine(vec![
        standard_library(),
        stream::native_api()?,
        inputs::api(inputs)?,
    ])
}

pub mod inputs {
    use kagari_abi::{
        callable::CallableImplementation,
        native_api::NativeModule,
        native_import::binding_id,
        scalar::BuiltinType,
        types::{AbiType, FunctionAbi},
    };
    use kagari_common::{
        cancellation::CancellationToken,
        collection::CollectionAccess,
        identity::{ModuleIdentity, PackageId},
    };
    use kagari_runtime::{
        error::RuntimeError,
        gc::RootedValue,
        native::{
            NativeAction, NativeContext, NativeInvocationState,
            api::{NativeApi, NativeHandler},
            catalog::NativeCatalog,
        },
        value::Value,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    /// Host roots retain values between executions; observations hold no handles.
    #[derive(Default)]
    pub struct Inputs {
        pub cursor: Rc<RefCell<Option<RootedValue>>>,
        pub values: Rc<RefCell<Option<RootedValue>>>,
        pub effects: Rc<Cell<usize>>,
        pub root_peak: Rc<Cell<usize>>,
        pub cancel: Rc<Cell<bool>>,
        pub cancellation: CancellationToken,
    }
    struct Complete {
        value: Value,
        _root: Option<RootedValue>,
    }
    impl NativeInvocationState for Complete {
        fn advance(&mut self, _: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
            Ok(NativeAction::Complete(self.value.clone()))
        }
    }
    pub fn api(inputs: &Inputs) -> Result<NativeApi, RuntimeError> {
        let mut module = NativeModule::new(ModuleIdentity {
            package: PackageId("game".into()),
            path: vec!["input".into()],
        });
        let cursor = binding_id(&module.identity, "cursor");
        let values = binding_id(&module.identity, "values");
        let probe = binding_id(&module.identity, "probe");
        let item = AbiType::Builtin(BuiltinType::I32);
        for (name, binding, return_type) in [
            (
                "cursor",
                cursor.clone(),
                AbiType::Iter(Box::new(item.clone())),
            ),
            (
                "values",
                values.clone(),
                AbiType::Array(Box::new(item), CollectionAccess::Mutable),
            ),
            ("probe", probe.clone(), AbiType::Builtin(BuiltinType::Unit)),
        ] {
            module.functions.push(FunctionAbi {
                name: name.into(),
                implementation: CallableImplementation::Native(binding),
                method_policy: Default::default(),
                generic_params: vec![],
                bounds: vec![],
                params: vec![],
                return_type,
            });
        }
        let cursor_input = inputs.cursor.clone();
        let values_input = inputs.values.clone();
        let effects = inputs.effects.clone();
        let root_peak = inputs.root_peak.clone();
        let cancel = inputs.cancel.clone();
        let cancellation = inputs.cancellation.clone();
        NativeApi::new(
            vec![module],
            vec![
                NativeHandler::new(cursor, 0, move |_| retained(&cursor_input)),
                NativeHandler::new(values, 0, move |_| retained(&values_input)),
                NativeHandler::new(probe, 0, move |context| {
                    effects.set(effects.get() + 1);
                    root_peak.set(root_peak.get().max(context.heap().active_roots()));
                    if cancel.get() {
                        cancellation.cancel();
                    }
                    Ok(Box::new(Complete {
                        value: Value::Unit,
                        _root: None,
                    }))
                }),
            ],
            NativeCatalog::default(),
        )
    }
    fn retained(
        input: &RefCell<Option<RootedValue>>,
    ) -> Result<Box<dyn NativeInvocationState>, RuntimeError> {
        let root = input
            .borrow()
            .as_ref()
            .cloned()
            .ok_or_else(|| RuntimeError::module_validation("host input is absent"))?;
        Ok(Box::new(Complete {
            value: root.value(),
            _root: Some(root),
        }))
    }
}

#[native_module("game::stream")]
pub mod stream {
    use kagari_runtime::{
        error::RuntimeError,
        native::{NativeAction, NativeContext, NativeInvocationState},
        native_value::{
            NativeCall, NativeResult, NativeValue,
            array::NativeArray,
            continuation::{NativeContinuation, NativeFn},
            iterator::{NativeIterator, NativeStateCall, NativeStateDependency},
            selected::NativeSelected,
        },
        value::Value,
    };
    use std::{cell::RefCell, marker::PhantomData};

    /// Shared progress with GC-traced captures and version-pinned native entries.
    #[native_type]
    pub struct Cursor<T: NativeValue>(NativeIterator<T>);

    /// A source may return None and later yield again; mapping preserves that policy.
    #[native_trait]
    pub trait Source {
        type Item: NativeValue;
        fn next(&self) -> NativeResult<NativeContinuation<Option<Self::Item>>>;
    }

    #[native_impl]
    impl<T: NativeValue> Cursor<T> {
        /// Construct lazily; the source is checked and protected during each step.
        #[native]
        pub fn from_array(
            #[context] call: &NativeCall,
            values: NativeArray<T>,
        ) -> NativeResult<Self> {
            let cursor = NativeIterator::new(
                call,
                0usize,
                &[NativeStateDependency::Collection(0)],
                array_step::<T>,
            )?;
            drop(values);
            Ok(Self(cursor))
        }
    }
    #[native_impl]
    impl<T: NativeValue> Source for Cursor<T> {
        type Item = T;
        #[native]
        fn next(&self) -> NativeResult<NativeContinuation<Option<T>>> {
            self.0.next()
        }
    }
    #[native]
    pub fn map<S: NativeValue, T: NativeValue, U: NativeValue>(
        #[context] call: &NativeCall,
        source: S,
        make: NativeFn<(T,), U>,
        #[selected(S: Source<Item = T>::next)] next: NativeSelected<(S,), Option<T>>,
    ) -> NativeResult<Cursor<U>> {
        let cursor = NativeIterator::new(
            call,
            (),
            &[NativeStateDependency::OptionalCollection(0)],
            map_step::<S, T, U>,
        )?;
        drop((source, make, next));
        Ok(Cursor(cursor))
    }
    #[native]
    pub fn counter(#[context] call: &NativeCall, seed: i32) -> NativeResult<Cursor<i32>> {
        let _ = seed;
        Ok(Cursor(NativeIterator::new(call, (), &[], counter_step)?))
    }
    fn counter_step(state: NativeStateCall<()>) -> NativeResult<NativeContinuation<Option<i32>>> {
        let seed = state.call()?.argument::<i32>(0)?;
        let next = seed.checked_add(1).expect("small fixture counter");
        let value = Some(seed).write(state.call()?, state.call()?.result_type())?;
        state.set_argument(0, next)?;
        Ok(NativeContinuation::new(Complete(value)))
    }
    #[native]
    pub fn invalid_capture(#[context] call: &NativeCall, seed: i32) -> NativeResult<Cursor<i32>> {
        let _ = seed;
        Ok(Cursor(NativeIterator::new(
            call,
            (),
            &[],
            invalid_capture_step,
        )?))
    }
    fn invalid_capture_step(
        state: NativeStateCall<()>,
    ) -> NativeResult<NativeContinuation<Option<i32>>> {
        state.set_argument(0, true)?;
        Ok(NativeContinuation::new(Complete(Value::Unit)))
    }
    #[native]
    pub fn invalid_result(#[context] call: &NativeCall) -> NativeResult<Cursor<i32>> {
        Ok(Cursor(NativeIterator::new(
            call,
            (),
            &[],
            invalid_result_step,
        )?))
    }
    fn invalid_result_step(
        _: NativeStateCall<()>,
    ) -> NativeResult<NativeContinuation<Option<i32>>> {
        Ok(NativeContinuation::new(Complete(Value::Bool(true))))
    }
    struct Complete(Value);
    impl NativeInvocationState for Complete {
        fn advance(&mut self, _: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            Ok(NativeAction::Complete(self.0.clone()))
        }
    }
    thread_local! {
        // An adversarial trusted host deliberately retains a step access. This
        // observes lease validation, rather than implementing iterator storage.
        static ESCAPED: RefCell<Option<NativeStateCall<usize>>> = const { RefCell::new(None) };
    }
    pub fn clear_escaped() {
        ESCAPED.with(|slot| {
            slot.borrow_mut().take();
        });
    }
    pub fn escaped_is_invalid() -> bool {
        ESCAPED.with(|slot| {
            slot.borrow().as_ref().is_some_and(|state| {
                state.data().is_err() && state.set_data(999).is_err() && state.call().is_err()
            })
        })
    }
    #[native]
    pub fn lease_probe(#[context] call: &NativeCall) -> NativeResult<Cursor<i32>> {
        Ok(Cursor(NativeIterator::new(call, 0usize, &[], lease_step)?))
    }
    #[native]
    pub fn lease_factory_error(#[context] call: &NativeCall) -> NativeResult<Cursor<i32>> {
        Ok(Cursor(NativeIterator::new(
            call,
            0usize,
            &[],
            lease_error_step,
        )?))
    }
    fn lease_error_step(
        state: NativeStateCall<usize>,
    ) -> NativeResult<NativeContinuation<Option<i32>>> {
        if state.data()? == 0 {
            state.set_data(1)?;
            ESCAPED.with(|slot| {
                *slot.borrow_mut() = Some(state);
            });
            return Err(RuntimeError::module_validation(
                "fixture state factory failed",
            ));
        }
        lease_step(state)
    }
    fn lease_step(state: NativeStateCall<usize>) -> NativeResult<NativeContinuation<Option<i32>>> {
        ESCAPED.with(|slot| {
            if let Some(old) = slot.borrow_mut().take() {
                if old.data().is_ok() || old.set_data(999).is_ok() || old.call().is_ok() {
                    return Err(RuntimeError::module_validation(
                        "old lease accessed newer invocation",
                    ));
                }
                // Dropping an old epoch cannot invalidate the current one.
                drop(old);
            }
            let index = state.data()?;
            let value = Some(index as i32).write(state.call()?, state.call()?.result_type())?;
            state.set_data(index + 1)?;
            *slot.borrow_mut() = Some(state);
            Ok(NativeContinuation::new(Complete(value)))
        })
    }
    fn array_step<T: NativeValue>(
        state: NativeStateCall<usize>,
    ) -> NativeResult<NativeContinuation<Option<T>>> {
        Ok(NativeContinuation::new(ArrayStep::<T> {
            state,
            _item: PhantomData,
        }))
    }
    struct ArrayStep<T: NativeValue> {
        state: NativeStateCall<usize>,
        _item: PhantomData<T>,
    }
    impl<T: NativeValue> NativeInvocationState for ArrayStep<T> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            let index = self.state.data()?;
            let source = self.state.call()?.argument::<NativeArray<T>>(0)?;
            let next = source.get(index)?;
            let value = next.write(self.state.call()?, self.state.call()?.result_type())?;
            // Allocation/conversion precedes the source cursor commit.
            if index < source.len() {
                self.state
                    .set_data(index.checked_add(1).expect("bounded array position"))?;
            }
            context.retain(0, value.clone())?;
            Ok(NativeAction::Complete(value))
        }
    }
    fn map_step<S: NativeValue, T: NativeValue, U: NativeValue>(
        state: NativeStateCall<()>,
    ) -> NativeResult<NativeContinuation<Option<U>>> {
        let source = state.call()?.argument::<S>(0)?;
        let make = state.call()?.argument::<NativeFn<(T,), U>>(1)?;
        let next = state.call()?.selected::<(S,), Option<T>>(0)?;
        Ok(NativeContinuation::new(MapStep {
            state,
            source: Some(source),
            make,
            next,
            phase: Phase::Source,
        }))
    }
    enum Phase {
        Source,
        SourceResult,
        MappedResult,
        Complete,
    }
    struct MapStep<S: NativeValue, T: NativeValue, U: NativeValue> {
        state: NativeStateCall<()>,
        source: Option<S>,
        make: NativeFn<(T,), U>,
        next: NativeSelected<(S,), Option<T>>,
        phase: Phase,
    }
    impl<S: NativeValue, T: NativeValue, U: NativeValue> NativeInvocationState for MapStep<S, T, U> {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            self.phase = Phase::SourceResult;
            Ok(NativeAction::Callback(self.next.request(
                context,
                (self.source.take().expect("one source request"),),
            )?))
        }
        fn receive(
            &mut self,
            context: &mut NativeContext<'_>,
            value: Value,
        ) -> NativeResult<NativeAction> {
            match self.phase {
                Phase::SourceResult => match self.next.result(context, value)? {
                    Some(item) => {
                        self.phase = Phase::MappedResult;
                        Ok(NativeAction::Callback(self.make.request(context, (item,))?))
                    }
                    None => {
                        self.phase = Phase::Complete;
                        Ok(NativeAction::Complete(Option::<U>::None.write(
                            self.state.call()?,
                            self.state.call()?.result_type(),
                        )?))
                    }
                },
                Phase::MappedResult => {
                    self.phase = Phase::Complete;
                    let item = self.make.result(context, value)?;
                    Ok(NativeAction::Complete(Some(item).write(
                        self.state.call()?,
                        self.state.call()?.result_type(),
                    )?))
                }
                _ => unreachable!("driver validates callback state"),
            }
        }
    }
}
