//! Allocation accounting isolates prepared native invocation from frame setup and compilation.
use super::native_allocations_counter::{Counts, measured, verify_counter};
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget, Slot};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::ids::FunctionRef;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    frame::{
        cursor::kernel::RegionExit,
        driver::{ExecutionAction, ReturnAction},
    },
    module::ExecutableModule,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::{CallContext, ScriptCall},
        conversion::context::ConversionContext,
        declarations::FunctionDecl,
        module::NativeModule,
        types::Type,
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{
    catalog as foundation_catalog,
    collections::vector::{VecValues, scalar::with_argument_slice},
    declarations::StandardDeclarations,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{hint::black_box, sync::Arc, time::Duration};

fn module() -> NativeModule {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "measure::native",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
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
            |cx: &mut CallContext<'_>, a: i32, b: i32| -> NativeResult<i32> {
                assert_eq!(
                    cx.argument_type_argument(0)?.ty(),
                    &Ty::Builtin(BuiltinType::I32)
                );
                assert_eq!(
                    cx.result_type_argument()?.ty(),
                    &Ty::Builtin(BuiltinType::I32)
                );
                Ok(a + b)
            },
        )
        .unwrap();
    let sum = module
        .define_function(
            FunctionDecl::new("sum")
                .parameter("values", language.vec(Type::i32()).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            sum,
            NativeBinding::new([Codec::Value], Codec::Exact(Type::i32()), |cx| {
                assert_eq!(
                    cx.argument_type_parameter(0, 0)?.ty(),
                    &Ty::Builtin(BuiltinType::I32)
                );
                with_argument_slice::<i32, _>(cx, 0, |values| {
                    Ok(Value::I32(values.iter().copied().sum()))
                })
            }),
        )
        .unwrap();
    module.finish().unwrap()
}

fn load() -> (Runtime, ExecutableModule) {
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
            .unwrap()
            .into_iter()
            .chain([Arc::new(module.to_declaration())])
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut runtime = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
    let loaded = runtime.load_program("allocation", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    (runtime, loaded)
}

struct CallSite {
    function: FunctionRef,
    pc: usize,
    arguments: Vec<Slot>,
    destination: Option<Slot>,
}

impl CallSite {
    fn find(loaded: &ExecutableModule, name: &str) -> Self {
        let function = loaded
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        let (pc, arguments, destination) = function
            .instructions
            .iter()
            .enumerate()
            .find_map(|(pc, instruction)| match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::Native(_),
                    args,
                    dst,
                } => Some((pc, args.clone(), *dst)),
                _ => None,
            })
            .expect("ordinary direct native call");
        Self {
            function: function.id,
            pc,
            arguments,
            destination,
        }
    }
}

fn unexpected_callback(
    _: &Runtime,
    _: &ExecutableModule,
    _: ScriptCall<'_>,
    _: &[Value],
) -> NativeResult<Value> {
    panic!("the measured scalar and bulk entries have no script callbacks")
}

fn sample(
    runtime: &Runtime,
    loaded: &ExecutableModule,
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
            .write_slot(runtime, *register, *value)
            .unwrap();
    }
    let invoke = || {
        stack
            .current_mut()
            .unwrap()
            .jump_to(runtime, site.pc)
            .unwrap();
        assert!(matches!(
            stack
                .execute_region(runtime, &mut Some(1), unexpected_callback)
                .unwrap(),
            RegionExit::Call(ExecutionAction::NativeEntry)
        ));
        assert_eq!(
            stack
                .start_native_entry(runtime, unexpected_callback)
                .unwrap(),
            ExecutionAction::NativeReturn
        );
        assert_eq!(
            stack.finish_native_return(runtime).unwrap(),
            ReturnAction::Continue(ExecutionAction::Script)
        );
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
            .read_slot(runtime, site.destination.unwrap())
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
        let root = ConversionContext::new(&runtime, &loaded)
            .unwrap()
            .encode(VecValues(vec![1i32; length]))
            .unwrap();
        let value = root.value(runtime.gc()).unwrap();
        drop(root);
        let (counts, elapsed) = sample(&runtime, &loaded, &bulk, &[value], 1_000, length as i32);
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
    let (pc, module, callee, arguments, destination) = caller
        .instructions
        .iter()
        .enumerate()
        .find_map(|(pc, instruction)| match instruction {
            BytecodeInstruction::Call {
                callee: CallTarget::ModuleFunction { module, function },
                args,
                dst,
            } => Some((pc, *module, *function, args.as_slice(), *dst)),
            BytecodeInstruction::Call {
                callee: CallTarget::Function(function),
                args,
                dst,
            } => Some((pc, loaded.slot(), *function, args.as_slice(), *dst)),
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
    assert!(matches!(
        stack
            .execute_region(&runtime, &mut None, unexpected_callback)
            .unwrap(),
        RegionExit::Call(ExecutionAction::Script)
    ));
    for (index, expected) in [11, 20, 11].into_iter().enumerate() {
        assert_eq!(
            stack
                .current()
                .unwrap()
                .read_slot(&runtime, Slot::new(index))
                .unwrap(),
            Value::I32(expected)
        );
    }
    let RegionExit::Return(value) = stack
        .execute_region(&runtime, &mut None, unexpected_callback)
        .unwrap()
    else {
        panic!("concrete scalar callee must remain in its prepared region");
    };
    stack.finish_return(&runtime, value).unwrap();
    let external = [Value::I32(11), Value::I32(20), Value::I32(11)];
    stack
        .push(&runtime, module, callee, &external, None)
        .unwrap();
    stack.pop().unwrap();
    let enter = || {
        stack.current_mut().unwrap().jump_to(&runtime, pc).unwrap();
        assert!(matches!(
            stack
                .execute_region(black_box(&runtime), &mut None, unexpected_callback)
                .unwrap(),
            RegionExit::Call(ExecutionAction::Script)
        ));
    };
    let (window, _) = measured(|| {
        for _ in 0..1000 {
            enter();
            stack.pop().unwrap();
        }
    });
    let (returned, _) = measured(|| {
        for _ in 0..1000 {
            for (register, value) in arguments.iter().zip(&external) {
                stack
                    .current_mut()
                    .unwrap()
                    .write_slot(&runtime, *register, *value)
                    .unwrap();
            }
            enter();
            let RegionExit::Return(value) = stack
                .execute_region(&runtime, &mut None, unexpected_callback)
                .unwrap()
            else {
                panic!("scalar return");
            };
            assert_eq!(
                stack.finish_return(&runtime, value).unwrap(),
                ReturnAction::Continue(ExecutionAction::Script)
            );
            assert_eq!(
                stack
                    .current()
                    .unwrap()
                    .read_slot(&runtime, destination.unwrap())
                    .unwrap(),
                Value::I32(42)
            );
        }
    });
    assert_eq!(
        returned,
        Counts::default(),
        "warm scalar entry, execution and return must not allocate"
    );
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
