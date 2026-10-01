//! Independent application algorithms use the same checked calls and preparation.
use self::inputs::Inputs;
use kagari_native_macros::native_module;
use kagari_runtime::{
    error::RuntimeError,
    native::{
        api::NativeApi, array_api::array, catalog::NativeCatalog, cmp_api::cmp, ops_api::ops,
        option_api::option, result_api::result, string_api::string,
    },
};

pub fn api() -> Result<NativeApi, RuntimeError> {
    api_with_inputs(&Inputs::default())
}
pub fn api_with_inputs(inputs: &Inputs) -> Result<NativeApi, RuntimeError> {
    let ops = ops::native_api()?;
    let cmp = cmp::native_api()?;
    let catalog = NativeCatalog::from_apis(&[&ops, &cmp])?;
    let array = array::native_api(&catalog)?;
    let order = order::native_api(&cmp.catalog())?;
    NativeApi::combine(vec![
        ops,
        cmp,
        array,
        order,
        option::native_api()?,
        result::native_api()?,
        string::native_api()?,
        inputs::api(inputs)?,
    ])
}

pub mod inputs {
    use kagari_abi::{
        callable::CallableImplementation,
        native_api::NativeModule,
        native_import::binding_id,
        scalar::BuiltinType,
        types::{AbiType, FunctionAbi, ParameterAbi},
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
        },
        value::Value,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    /// Real host retention supplies an array; counters contain no script handles.
    #[derive(Default)]
    pub struct Inputs {
        pub values: Rc<RefCell<Option<RootedValue>>>,
        pub effects: Rc<Cell<i32>>,
        pub root_peak: Rc<Cell<usize>>,
        pub comparisons: Rc<Cell<usize>>,
        pub cancel_after: Rc<Cell<Option<usize>>>,
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
        let values = binding_id(&module.identity, "values");
        let record = binding_id(&module.identity, "record");
        let probe = binding_id(&module.identity, "probe");
        module.functions.push(FunctionAbi {
            name: "probe".into(),
            implementation: CallableImplementation::Native(probe.clone()),
            method_policy: Default::default(),
            generic_params: vec![],
            bounds: vec![],
            params: vec![],
            return_type: AbiType::Builtin(BuiltinType::Unit),
        });
        module.functions.push(FunctionAbi {
            name: "values".into(),
            implementation: CallableImplementation::Native(values.clone()),
            method_policy: Default::default(),
            generic_params: vec![],
            bounds: vec![],
            params: vec![],
            return_type: AbiType::Array(
                Box::new(AbiType::Builtin(BuiltinType::I32)),
                CollectionAccess::Mutable,
            ),
        });
        module.functions.push(FunctionAbi {
            name: "record".into(),
            implementation: CallableImplementation::Native(record.clone()),
            method_policy: Default::default(),
            generic_params: vec![],
            bounds: vec![],
            params: vec![ParameterAbi {
                name: "value".into(),
                ty: AbiType::Builtin(BuiltinType::I32),
                mutable: false,
            }],
            return_type: AbiType::Builtin(BuiltinType::Unit),
        });
        let captured = inputs.values.clone();
        let effects = inputs.effects.clone();
        let root_peak = inputs.root_peak.clone();
        let comparisons = inputs.comparisons.clone();
        let cancel_after = inputs.cancel_after.clone();
        let cancellation = inputs.cancellation.clone();
        NativeApi::new(
            vec![module],
            vec![
                NativeHandler::new(probe, 0, move |context| {
                    root_peak.set(root_peak.get().max(context.heap().active_roots()));
                    comparisons.set(comparisons.get() + 1);
                    if cancel_after.get() == Some(comparisons.get()) {
                        cancellation.cancel();
                    }
                    Ok(Box::new(Complete {
                        value: Value::Unit,
                        _root: None,
                    }))
                }),
                NativeHandler::new(values, 0, move |_| {
                    let root = captured.borrow().as_ref().cloned().ok_or_else(|| {
                        RuntimeError::module_validation("host array input is absent")
                    })?;
                    Ok(Box::new(Complete {
                        value: root.value(),
                        _root: Some(root),
                    }))
                }),
                NativeHandler::new(record, 0, move |context| {
                    let Some(Value::I32(value)) = context.argument(0) else {
                        return Err(RuntimeError::module_validation("host record argument"));
                    };
                    effects.set(value);
                    Ok(Box::new(Complete {
                        value: Value::Unit,
                        _root: None,
                    }))
                }),
            ],
            Default::default(),
        )
    }
}
#[native_module("game::order", catalog)]
pub mod order {
    use kagari_runtime::{
        native::{NativeAction, NativeContext, NativeInvocationState, sorting},
        native_value::{
            NativeResult, NativeValue,
            array::NativeArray,
            continuation::{NativeContinuation, NativeFn},
            reorder::NativeReorder,
            selected::NativeSelected,
        },
        value::Value,
    };
    use std::cmp::Ordering;

    #[native]
    pub fn arrange<T: NativeValue>(
        array: NativeArray<T>,
        #[selected(T: std::cmp::Ord::cmp)] compare: NativeSelected<(T, T), Ordering>,
    ) -> NativeResult<NativeContinuation<()>> {
        sorting::stable_sort(&array, compare)
    }
    #[native]
    pub fn arrange_by<T: NativeValue>(
        array: NativeArray<T>,
        compare: NativeFn<(T, T), Ordering>,
    ) -> NativeResult<NativeContinuation<()>> {
        sorting::stable_sort(&array, compare)
    }
    /// A separate algorithm proves that preparation belongs to storage, not sort IDs.
    #[native]
    pub fn reverse<T: NativeValue>(array: NativeArray<T>) -> NativeResult<NativeContinuation<()>> {
        Ok(NativeContinuation::new(Reverse {
            count: array.len(),
            preparation: Some(array.prepare_reorder()?),
            allocated: false,
        }))
    }
    struct Reverse<T: NativeValue> {
        count: usize,
        preparation: Option<NativeReorder<T>>,
        allocated: bool,
    }
    impl<T: NativeValue> NativeInvocationState for Reverse<T> {
        fn advance(&mut self, _: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            if !self.allocated {
                self.preparation
                    .as_ref()
                    .expect("active preparation")
                    .allocate_input()?;
                self.allocated = true;
            } else if self.count != 0 {
                self.count -= 1;
                let preparation = self.preparation.as_ref().expect("active preparation");
                preparation.push_input(preparation.read_original(self.count)?)?;
            } else {
                self.preparation.take().expect("one commit").commit()?;
                return Ok(NativeAction::Complete(Value::Unit));
            }
            Ok(NativeAction::Continue)
        }
    }
}
