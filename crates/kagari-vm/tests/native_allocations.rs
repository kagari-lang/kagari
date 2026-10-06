//! Allocation accounting isolates prepared native invocation from frame setup and compilation.
use kagari_bytecode::instruction::{
    BytecodeInstruction, CallTarget, LocalSlot, NativeImportId, Register,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::ids::FunctionRef;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    module::LoadedModule,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        context::{CallContext, ScriptCall},
        declarations::FunctionDecl,
        module::NativeModule,
        types::Type,
        views::SequenceHandle,
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{catalog as foundation_catalog, declarations::StandardDeclarations};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use native_allocations_counter::{Counts, measured, verify_counter};
use std::{hint::black_box, sync::Arc, time::Duration};

mod native_allocations_counter;

fn module() -> NativeModule {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "measure::native",
        &language.catalog().expect("explicit standard providers"),
    );
    let add = module
        .define_function(
            FunctionDecl::new("add")
                .parameter("a", Type::i32())
                .parameter("b", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            add,
            |_cx: &mut CallContext<'_>, a: i32, b: i32| -> NativeResult<i32> { Ok(a + b) },
        )
        .unwrap();
    let sum = module
        .define_function(
            FunctionDecl::new("sum")
                .parameter("values", language.vec(Type::i32()))
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            sum,
            |_cx: &mut CallContext<'_>, values: SequenceHandle<'_>| -> NativeResult<i32> {
                values.with_slice::<i32, _>(|values| Ok(values.iter().copied().sum()))
            },
        )
        .unwrap();
    module.finish().unwrap()
}

fn load() -> (Runtime, LoadedModule) {
    let module = module();
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "allocation.kgr",
            r#"
        use measure::native::{add, sum};
        fn scalar(a: i32, b: i32) -> i32 { add(a, b) }
        fn bulk(values: Vec<i32>) -> i32 { sum(values) }
        fn script_add(a: i32, b: i32, c: i32) -> i32 { a + b + c }
        fn script_caller(a: i32, b: i32) -> i32 { script_add(b, a, b) }
    "#
            .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation_catalog::shared()
            .into_iter()
            .chain([Arc::new(module.to_declaration().unwrap())])
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module.install(&mut runtime).unwrap();
    let loaded = runtime.load_program("allocation", program).unwrap();
    (runtime, loaded)
}

struct CallSite {
    function: FunctionRef,
    import: NativeImportId,
    arguments: Vec<Register>,
    destination: Option<Register>,
}

impl CallSite {
    fn find(loaded: &LoadedModule, name: &str) -> Self {
        let function = loaded
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        let (import, arguments, destination) = function
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::Native(import),
                    args,
                    dst,
                } => Some((*import, args.clone(), *dst)),
                _ => None,
            })
            .expect("ordinary direct native call");
        Self {
            function: function.id,
            import,
            arguments,
            destination,
        }
    }
}

fn unexpected_callback(
    _: &Runtime,
    _: &LoadedModule,
    _: ScriptCall<'_>,
    _: &[Value],
) -> NativeResult<Value> {
    panic!("the measured scalar and bulk entries have no script callbacks")
}

fn sample(
    runtime: &Runtime,
    loaded: &LoadedModule,
    site: &CallSite,
    arguments: &[Value],
    calls: usize,
    expected: i32,
) -> (Counts, Duration) {
    let stack = runtime.enter_execution_stack(loaded).unwrap();
    stack
        .push(runtime, loaded.slot(), site.function, arguments, None)
        .unwrap();
    for (register, value) in site.arguments.iter().zip(arguments) {
        stack
            .current_mut()
            .unwrap()
            .write_register(runtime, *register, value.clone())
            .unwrap();
    }
    let invoke = || {
        stack
            .invoke_native(
                runtime,
                site.import,
                &site.arguments,
                site.destination,
                unexpected_callback,
            )
            .unwrap()
    };
    // Warm the boundary and initialize every thread-local/cache outside accounting.
    for _ in 0..128 {
        invoke();
    }
    let roots = runtime.gc().active_roots();
    let occupancy = runtime.gc().stats().current_heap_units;
    let sample = measured(|| {
        for _ in 0..calls {
            black_box(&invoke)();
        }
    });
    assert_eq!(
        sample.0,
        Counts::default(),
        "prepared boundary must not allocate or release Rust heap storage"
    );
    assert_eq!(runtime.gc().active_roots(), roots);
    assert_eq!(runtime.gc().stats().current_heap_units, occupancy);
    assert_eq!(
        stack
            .current()
            .unwrap()
            .read_register(runtime, site.destination.unwrap())
            .unwrap(),
        Value::I32(expected)
    );
    sample
}

#[test]
fn warmed_scalar_and_contiguous_i32_boundaries_add_no_allocations() {
    verify_counter();
    let (runtime, loaded) = load();
    let scalar = CallSite::find(&loaded, "scalar");
    let (counts, elapsed) = sample(
        &runtime,
        &loaded,
        &scalar,
        &[Value::I32(20), Value::I32(22)],
        100_000,
        42,
    );
    println!(
        "scalar,calls=100000,{counts:?},elapsed_ns={}",
        elapsed.as_nanos()
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    let bulk = CallSite::find(&loaded, "bulk");
    for length in [0, 16, 16_384] {
        let array = runtime
            .alloc_array(
                &loaded,
                Ty::Builtin(BuiltinType::I32),
                vec![Value::I32(1); length],
            )
            .unwrap();
        let (counts, elapsed) = sample(
            &runtime,
            &loaded,
            &bulk,
            &[Value::Array(array)],
            1_000,
            length as i32,
        );
        println!(
            "bulk_i32,elements={length},calls=1000,{counts:?},elapsed_ns={}",
            elapsed.as_nanos()
        );
        assert_eq!(runtime.gc().active_roots(), 0);
        assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    }
}

#[test]
fn warmed_script_argument_windows_match_borrowed_slice_allocation_cost() {
    verify_counter();
    let (runtime, loaded) = load();
    let caller = loaded
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "script_caller")
        .unwrap();
    let (module, callee, arguments) = caller
        .instructions
        .iter()
        .find_map(|instruction| match instruction {
            BytecodeInstruction::Call {
                callee: CallTarget::ModuleFunction { module, function },
                args,
                ..
            } => Some((*module, *function, args.as_slice())),
            BytecodeInstruction::Call {
                callee: CallTarget::Function(function),
                args,
                ..
            } => Some((loaded.slot(), *function, args.as_slice())),
            _ => None,
        })
        .unwrap();
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    stack
        .push(
            &runtime,
            loaded.slot(),
            caller.id,
            &[Value::I32(20), Value::I32(11)],
            None,
        )
        .unwrap();
    for instruction in &caller.instructions {
        if let BytecodeInstruction::LoadLocal { dst, local } = instruction {
            let value = stack
                .current()
                .unwrap()
                .read_local(&runtime, *local)
                .unwrap();
            stack
                .current_mut()
                .unwrap()
                .write_register(&runtime, *dst, value)
                .unwrap();
        }
    }
    stack
        .push_registers(&runtime, module, callee, arguments, None)
        .unwrap();
    for (index, expected) in [11, 20, 11].into_iter().enumerate() {
        assert_eq!(
            stack
                .current()
                .unwrap()
                .read_local(&runtime, LocalSlot::new(index))
                .unwrap(),
            Value::I32(expected)
        );
    }
    stack.pop().unwrap();
    let external = [Value::I32(11), Value::I32(20), Value::I32(11)];
    stack
        .push(&runtime, module, callee, &external, None)
        .unwrap();
    stack.pop().unwrap();
    let (window, _) = measured(|| {
        for _ in 0..1000 {
            stack
                .push_registers(&runtime, module, callee, black_box(arguments), None)
                .unwrap();
            stack.pop().unwrap();
        }
    });
    let (slice, _) = measured(|| {
        for _ in 0..1000 {
            stack
                .push(&runtime, module, callee, black_box(&external), None)
                .unwrap();
            stack.pop().unwrap();
        }
    });
    assert_eq!(
        window, slice,
        "register-to-window copying must add no packing allocations"
    );
    assert_eq!(
        window,
        Counts::default(),
        "warmed direct scalar frame entry must not allocate"
    );
    println!("SCRIPT_FRAME_ALLOCATIONS,calls=1000,window={window:?},borrowed_slice={slice:?}");
    drop(stack);
    assert_eq!(runtime.gc().active_roots(), 0);
}
