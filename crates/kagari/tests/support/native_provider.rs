//! Application-owned provider shared by source emission and an independent
//! source-free embedding consumer. No runtime-private APIs are used.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use {
    kagari_runtime::{
        error::RuntimeError,
        native::{
            binding::NativeResult,
            builder::ModuleBuilder,
            callable::{CallableHandle, StoredCallable},
            context::NativeCall,
            declarations::{FunctionDecl, MethodDecl},
            module::NativeModule,
            storage::{NativePayload, NativeStorage},
            types::Type,
            views::ValueHandle,
        },
        resource::HeapCharge,
        value::Value,
    },
    kagari_stdlib::declarations::StandardDeclarations,
};

fn fill(
    cx: &mut NativeCall<'_>,
    count: usize,
    callback: CallableHandle<'_>,
) -> NativeResult<Value> {
    let mut roots = Vec::new();
    for index in 0..count {
        let value = callback.call_values(cx, &[Value::U64(index as u64)])?;
        roots.push(
            cx.heap()
                .root_value(value)
                .ok_or_else(|| RuntimeError::module_validation("callback result root"))?,
        );
        cx.collect_garbage()?;
    }
    let values = roots
        .iter()
        .map(|root| root.value(cx.heap()).unwrap())
        .collect();
    cx.allocate_array(cx.result_type_parameter(0)?, values)
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
    value: Value,
    callback: StoredCallable,
    drops: Arc<AtomicUsize>,
}

impl NativePayload for Handler {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        visit(&self.value);
        self.callback.trace(visit);
    }

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::new(2, 0))
    }
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
            let apply = methods.define_method(MethodDecl::instance("apply").returns(item))?;
            methods.bind(
                apply,
                |cx: &mut NativeCall<'_>, receiver: ValueHandle<'_>| -> NativeResult<Value> {
                    let (value, callback) = receiver.with_payload::<Handler, _>(cx, |handler| {
                        Ok((handler.value, handler.callback.clone()))
                    })?;
                    cx.collect_garbage()?;
                    callback.call_values(cx, &[value])
                },
            )
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
        move |cx: &mut NativeCall<'_>,
              value: ValueHandle<'_>,
              callback: CallableHandle<'_>|
              -> NativeResult<Value> {
            cx.allocate_result_payload(
                Handler {
                    value: value.value(cx)?,
                    callback: callback.store(),
                    drops: drops.clone(),
                },
                &[],
            )
        },
    )
}
