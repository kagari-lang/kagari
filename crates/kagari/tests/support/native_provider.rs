//! Application-owned provider shared by source emission and an independent
//! source-free embedding consumer. No runtime-private APIs are used.

use kagari_gc::{
    error::GcError,
    object::{Edge, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use {
    kagari_runtime::{
        error::RuntimeError,
        native::{
            binding::{Codec, NativeBinding, NativeResult},
            builder::ModuleBuilder,
            callable::{CallableHandle, StoredCallable},
            context::NativeCall,
            declarations::{FunctionDecl, MethodDecl},
            module::NativeModule,
            storage::NativeStorage,
            types::Type,
            views::ValueHandle,
        },
        value::{carriers::StoredValue, view::ValueView},
    },
    kagari_stdlib::declarations::StandardDeclarations,
};

fn fill<'call>(
    cx: &mut NativeCall<'call>,
    count: usize,
    callback: CallableHandle<'call>,
) -> NativeResult<ValueView<'call>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| RuntimeError::resource_limit("callback output capacity"))?;
    for index in 0..count {
        let value = callback.call_values(cx, &[ValueView::from(index as u64)])?;
        values.push(value);
        cx.collect_garbage()?;
    }
    cx.allocate_array(cx.result_type_parameter(0)?, &values)
}

pub fn module(drops: Arc<AtomicUsize>) -> NativeModule {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "external::fixture",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let function = module
        .define_function(FunctionDecl::new("from_fn"))
        .unwrap();
    module
        .function(&function, |function| {
            let item = function.type_parameter("T")?;
            function.parameter("count", Type::u64())?;
            function.parameter("callback", Type::function([Type::u64()], item.ty())?)?;
            function.returns(Type::array(item.ty())?)?;
            Ok(())
        })
        .unwrap();
    module.bind(function, fill).unwrap();
    register_handler(&mut module, drops).unwrap();
    module.finish().unwrap()
}

#[derive(Debug)]
struct Handler {
    value: StoredValue,
    callback: Edge<StoredCallable>,
    drops: Arc<AtomicUsize>,
}

// SAFETY: construction imports both initialized fields into this provider. The
// fields never change; pending positions are retried before advancement. Drop
// touches only the external probe and never reads a managed peer.
unsafe impl GcObject for Handler {
    type ScanState = usize;
    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> usize {
        0
    }

    fn scan(
        &self,
        next: &mut usize,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while *next < 2 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: this admitted provider owns both initialized fields.
            let status = unsafe {
                if *next == 0 {
                    self.value.scan(budget, visitor)?
                } else {
                    visitor.try_visit_stored(&self.callback, budget)?
                }
            };
            if status == ScanStatus::Pending {
                return Ok(status);
            }
            *next += 1;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { units: 2, bytes: 0 }
    }
}

fn apply<'call>(
    cx: &mut NativeCall<'call>,
    receiver: ValueHandle<'call>,
) -> NativeResult<ValueView<'call>> {
    let receiver = receiver.value(cx)?;
    let value = cx.value_payload::<Handler>(&receiver, |scope, handler| {
        StoredValue::get_field(scope, handler, |handler| Some(&handler.value))
    })?;
    let callback = cx
        .callable_payload::<Handler>(&receiver, |scope, handler| {
            scope.follow(handler, |handler| Some(&handler.callback))
        })?
        .ok_or_else(|| RuntimeError::module_validation("missing stored callback"))?;
    cx.collect_garbage()?;
    callback.call_values(cx, &[value])
}

impl Drop for Handler {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

fn register_handler(module: &mut ModuleBuilder, drops: Arc<AtomicUsize>) -> NativeResult<()> {
    let mut declaration = module.define_type("Handler")?;
    declaration.type_parameter("T")?;
    declaration.native_storage(NativeStorage::payload::<Handler>())?;
    let handler = declaration.finish()?;
    module.implement(handler.clone(), |group| {
        let item = group.parameter("T")?.ty();
        group.inherent_impl(|methods| {
            let method = methods.define_method(MethodDecl::instance("apply").returns(item))?;
            methods.bind(method, apply)
        })
    })?;
    let hold = module.define_function(FunctionDecl::new("hold"))?;
    module.function(&hold, |function| {
        let item = function.type_parameter("T")?.ty();
        function.parameter("value", item.clone())?;
        function.parameter("callback", Type::function([item.clone()], item.clone())?)?;
        function.returns(handler.apply([item])?)?;
        Ok(())
    })?;
    module.bind(
        hold,
        NativeBinding::new(
            vec![Codec::Value, Codec::Callable],
            Codec::Value,
            move |cx| {
                let value = cx.argument(0)?;
                let callback = cx.callable(1)?;
                let drops = drops.clone();
                cx.allocate_result_with(&[], |native| {
                    let value = native.value(&value)?;
                    let callback = callback.store(native)?;
                    // SAFETY: same-brand admitted fields belong only to the returned
                    // Handler, which scans both completely and reads no peers on Drop.
                    unsafe {
                        native.scope().alloc_with(|mut init| {
                            Ok(Handler {
                                value: StoredValue::initialize(&mut init, value)?,
                                callback: init.edge_scoped(callback)?,
                                drops,
                            })
                        })
                    }
                })
            },
        ),
    )
}
