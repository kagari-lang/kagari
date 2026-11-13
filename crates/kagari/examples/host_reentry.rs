//! A synchronous host callback invokes the pinned script version and retains its result.
use kagari::{context::ExecutionContext, engine::KagariEngine};
use kagari_bytecode::module::CallableTarget;
use kagari_runtime::execution::reentry::reenter;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    host::HostError,
    module::entry::ProgramEntry,
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::FunctionDecl,
        host, types::Type,
    },
    session::{ExecutionEvent, ExecutionObserver},
    value::view::ValueView,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    scalar::BuiltinType,
};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct StackDepth(Vec<String>);

impl ExecutionObserver for StackDepth {
    fn observe(
        &mut self,
        _: &Runtime,
        _: ExecutionEvent,
        frames: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        if frames.len() > self.0.len() {
            self.0 = frames
                .iter()
                .map(|frame| match frame.target() {
                    CallableTarget::Script(_) => frame.function().unwrap().name.clone(),
                    CallableTarget::Native(import) => {
                        let owner = frame.loaded();
                        owner
                            .definitions()
                            .resolve(
                                owner.bytecode.native_imports[import.index()]
                                    .instance
                                    .declaration,
                            )
                            .expect("checked Native declaration")
                            .last()
                            .expect("checked Native name")
                            .name
                            .to_owned()
                    }
                })
                .collect();
        }
        Ok(())
    }
}

fn main() {
    let mut builder = KagariEngine::builder().unwrap();
    builder.install_all(stdlib::modules().unwrap()).unwrap();
    let mut module = ModuleBuilder::new("example::reentry", builder.declarations()).unwrap();
    let notify = module
        .define_function(
            FunctionDecl::new("notify")
                .parameter("message", Type::scalar(BuiltinType::String))
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        may_call_host_services: true,
                        may_trap: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    let retained = Arc::new(Mutex::new(None));
    let output = retained.clone();
    module
        .bind(
            notify,
            host::callback(move |call, _| {
                let root = call.runtime().execution_root().unwrap();
                let scratch = call
                    .convert_value(&root, |cx| {
                        let item = cx.type_for::<i32>()?;
                        cx.allocate_array(&item, &[ValueView::from(3i32)])
                    })
                    .map_err(|error| HostError::new(error.to_string()))?;
                // Resolve in the admitted program; the Host context's source
                // protects scratch across reentry without an extra root pack.
                let make = root
                    .bytecode
                    .functions
                    .iter()
                    .find(|function| function.name == "make")
                    .ok_or_else(|| HostError::new("missing pinned make entry"))?
                    .id;
                let arguments = call
                    .retain_temporaries(&[])
                    .map_err(|error| HostError::new(error.to_string()))?;
                let value = reenter(call, &root, make, arguments).map_err(|error| {
                    HostError::new(format!("script callback failed: {error:?}"))
                })?;
                call.runtime_mut().collect_garbage().unwrap();
                call.convert_value(&root, |cx| {
                    assert_eq!(cx.array_len(&scratch)?, 1);
                    assert_eq!(cx.array_element(&scratch, 0)?.unwrap().as_i32(), Some(3));
                    Ok(ValueView::unit())
                })
                .map_err(|error| HostError::new(error.to_string()))?;
                *output.lock().unwrap() = Some(value);
                Ok(ValueView::unit())
            }),
        )
        .unwrap();
    builder.install(module.finish().unwrap()).unwrap();
    let engine = builder.build().unwrap();
    let context = ExecutionContext::default();
    let program = engine
        .compile_to_program(
            SourceFile::new(
                "reentry.kgr",
                include_str!("../../../examples/host-reentry.kgr"),
            ),
            Default::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(context.clone());

    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let _selected_loaded = runtime.select_program(&loaded).unwrap();
    runtime
        .set_execution_observer(StackDepth::default())
        .unwrap();
    let mut session = runtime
        .begin_execution(&_selected_loaded, runtime.execution_options())
        .unwrap();
    session.runtime_mut().attach_execution_observer().unwrap();
    assert_eq!(
        session
            .runtime_mut()
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert_eq!(
        session
            .runtime()
            .execution_observer::<StackDepth>()
            .unwrap()
            .0,
        ["main", "notify", "make"]
    );
    assert_eq!(session.counters().current_call_depth, 0);
    assert_eq!(session.host_scope_count(), 0);
    drop(session);
    runtime.collect_garbage().unwrap();
    {
        let retained = retained.lock().unwrap();
        let mut cx = ConversionContext::new(&mut runtime, &_selected_loaded).unwrap();
        let value = cx.import(retained.as_ref().unwrap()).unwrap();
        assert_eq!(cx.array_len(&value).unwrap(), 2);
        for (index, expected) in [7, 8].into_iter().enumerate() {
            assert_eq!(
                cx.array_element(&value, index).unwrap().unwrap().as_i32(),
                Some(expected)
            );
        }
    }
    runtime.collect_garbage().unwrap();
    let before = runtime.gc_stats();
    retained.lock().unwrap().take();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, before.objects - 1);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before.root_registrations - 1
    );
    println!("host reentry returned an array; its explicit root survived collection");
}
