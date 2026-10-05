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
            context::CallContext,
            declarations::{FunctionDecl, MethodDecl},
            module::NativeModule,
            storage::{NativePayload, NativeStorage},
            types::Type,
            views::ValueHandle,
        },
        value::Value,
    },
    kagari_stdlib::declarations::StandardDeclarations,
};

fn fill(
    cx: &mut CallContext<'_>,
    count: usize,
    callback: CallableHandle<'_>,
) -> NativeResult<Value> {
    let value = cx.allocate_result()?;
    let output = cx
        .heap()
        .root_value(value)
        .ok_or_else(|| RuntimeError::module_validation("output root"))?;
    let Value::Array(array) = output.value(cx.heap()).unwrap() else {
        return Err(RuntimeError::module_validation("array output"));
    };
    for index in 0..count {
        let value = callback.call_values(cx, &[Value::U64(index as u64)])?;
        // Protect a heap result before any allocation or further script reentry.
        let value = cx
            .heap()
            .root_value(value)
            .ok_or_else(|| RuntimeError::module_validation("callback result root"))?;
        cx.collect_garbage()?;
        cx.heap()
            .array_push(array, value.value(cx.heap()).unwrap())?;
    }
    Ok(output.value(cx.heap()).unwrap())
}

pub fn module(drops: Arc<AtomicUsize>) -> NativeModule {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "external::fixture",
        &language.catalog().expect("explicit standard providers"),
    );
    let function = module
        .define_function(FunctionDecl::new("from_fn"))
        .unwrap();
    module
        .function(&function, |function| {
            let item = function.type_parameter("T")?;
            function.parameter("count", Type::usize());
            function.parameter("callback", Type::function([Type::usize()], item.ty()));
            function.returns(language.vec(item.ty()));
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

    fn units(&self) -> usize {
        2
    }
}

impl Drop for Handler {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

fn register_handler(module: &mut ModuleBuilder, drops: Arc<AtomicUsize>) -> NativeResult<()> {
    let mut declaration = module.define_type("Handler");
    declaration.type_parameter("T")?;
    declaration.native_storage(NativeStorage::payload::<Handler>())?;
    let handler = declaration.finish()?;
    module.implement(handler.clone(), |group| {
        let item = group.parameter("T")?.ty();
        group.inherent_impl(|methods| {
            let apply = methods.define_method(MethodDecl::instance("apply").returns(item))?;
            methods.bind(
                apply,
                |cx: &mut CallContext<'_>, receiver: ValueHandle<'_>| -> NativeResult<Value> {
                    let (value, callback) = receiver.with_payload::<Handler, _>(|handler| {
                        Ok((handler.value.clone(), handler.callback.clone()))
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
        function.parameter("value", item.clone());
        function.parameter("callback", Type::function([item.clone()], item.clone()));
        function.returns(handler.apply([item])?);
        Ok(())
    })?;
    module.bind(
        hold,
        move |cx: &mut CallContext<'_>,
              value: ValueHandle<'_>,
              callback: CallableHandle<'_>|
              -> NativeResult<Value> {
            cx.allocate_result_payload(Handler {
                value: value.value(),
                callback: callback.store(),
                drops: drops.clone(),
            })
        },
    )
}
